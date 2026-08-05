//! Glyph outlines, read out of a TrueType file.
//!
//! Enough of the format to fill printable ASCII and Latin-1 — the range
//! `pdf::encode` passes through — and no more: `head`, `maxp`,
//! `loca`, `glyf`, `cmap` format 4, and `hmtx` — the last only so a test can
//! check the face against the advance tables the layout already uses. No
//! hinting, no kerning, no shaping, no `GSUB`, no `GPOS`. A plasmid map draws
//! feature names and coordinates; none of it needs a shaper, and the vector
//! back ends do not use one either.
//!
//! # The pen advances by `pdf::HELVETICA`, not by this file's `hmtx`
//!
//! Deliberate, and the reason is the whole point of the face choice. Every
//! label's width, the truncation, the `viewBox` crop and every `Anchor::End`
//! placement in this crate come from [`crate::pdf::text_width_in`], which is
//! Helvetica's advances rounded to 1/1000 em. If the raster advanced by
//! Liberation's own `hmtx` instead, the pen would drift from the layout by up
//! to 0.238/1000 em per character — 0.0029 pt at 12 pt, and cumulative — and a
//! `Anchor::Middle` string would end up off-centre against the same string in
//! the PDF. Tiny, but it is drift the vector formats do not have, and this
//! crate's claim is that its back ends agree.
//!
//! The face is here for the **shapes**. That the two agree to rounding is what
//! makes using it honest, and it is asserted in `tests.rs` rather than trusted.
//!
//! # The implied on-curve point
//!
//! TrueType contours are quadratic B-splines in which consecutive off-curve
//! points imply an on-curve point at their midpoint. Skipping that rule does
//! not produce a visibly broken glyph — it produces a slightly wrong one, worth
//! about half a pixel at 9 pt and 300 dpi, which no reviewer would catch by
//! looking. It is handled in [`Face::outline`] and checked against fontTools.

/// One step of a glyph contour, in font units, y up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve {
    Move(f64, f64),
    Line(f64, f64),
    /// A quadratic Bézier: one control point, then the end point.
    Quad(f64, f64, f64, f64),
    Close,
}

/// A parsed TrueType face.
pub struct Face<'a> {
    data: &'a [u8],
    /// Font design units per em, from `head`.
    pub units_per_em: f64,
    loca: Vec<u32>,
    glyf: (usize, usize),
    cmap4: usize,
    hmtx: (usize, usize),
    num_h_metrics: usize,
}

fn u16_at(d: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(i)?, *d.get(i + 1)?]))
}

fn i16_at(d: &[u8], i: usize) -> Option<i16> {
    u16_at(d, i).map(|v| v as i16)
}

fn u32_at(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *d.get(i)?,
        *d.get(i + 1)?,
        *d.get(i + 2)?,
        *d.get(i + 3)?,
    ]))
}

impl<'a> Face<'a> {
    /// Parse the tables this crate needs.
    ///
    /// `None` rather than a panic on anything malformed: the only faces that
    /// reach here are the ones committed beside this file, so a failure is a
    /// broken checkout rather than user input — but a rasterizer that panics
    /// takes the export with it.
    pub fn parse(data: &'a [u8]) -> Option<Face<'a>> {
        let num_tables = u16_at(data, 4)? as usize;
        let find = |tag: &[u8; 4]| -> Option<(usize, usize)> {
            (0..num_tables).find_map(|i| {
                let rec = 12 + i * 16;
                (data.get(rec..rec + 4)? == tag).then_some(())?;
                let off = u32_at(data, rec + 8)? as usize;
                let len = u32_at(data, rec + 12)? as usize;
                (off + len <= data.len()).then_some((off, len))
            })
        };

        let (head, _) = find(b"head")?;
        let units_per_em = f64::from(u16_at(data, head + 18)?);
        let long_loca = i16_at(data, head + 50)? != 0;

        let (maxp, _) = find(b"maxp")?;
        let num_glyphs = u16_at(data, maxp + 4)? as usize;

        let (loca_off, _) = find(b"loca")?;
        let mut loca = Vec::with_capacity(num_glyphs + 1);
        for i in 0..=num_glyphs {
            loca.push(if long_loca {
                u32_at(data, loca_off + i * 4)?
            } else {
                // The short form stores offsets divided by two, which is the
                // one place a reader silently halves every glyph.
                u32::from(u16_at(data, loca_off + i * 2)?) * 2
            });
        }

        let glyf = find(b"glyf")?;
        let hmtx = find(b"hmtx")?;
        let (hhea, _) = find(b"hhea")?;
        let num_h_metrics = u16_at(data, hhea + 34)? as usize;

        // cmap: the Windows BMP subtable, format 4. Both committed faces carry
        // one at (3, 1); the (0, 3) Unicode subtable is the same format and the
        // same content, and (1, 0) is a Macintosh format 6 that covers less.
        let (cmap, _) = find(b"cmap")?;
        let n = u16_at(data, cmap + 2)? as usize;
        let mut cmap4 = 0;
        for i in 0..n {
            let rec = cmap + 4 + i * 8;
            let plat = u16_at(data, rec)?;
            let enc = u16_at(data, rec + 2)?;
            let off = cmap + u32_at(data, rec + 4)? as usize;
            if u16_at(data, off)? == 4 && ((plat == 3 && enc == 1) || (plat == 0)) {
                cmap4 = off;
                if plat == 3 {
                    break;
                }
            }
        }
        (cmap4 != 0).then_some(())?;

        Some(Face {
            data,
            units_per_em,
            loca,
            glyf,
            cmap4,
            hmtx,
            num_h_metrics,
        })
    }

    /// The glyph index for a character, via `cmap` format 4.
    pub fn glyph(&self, c: char) -> Option<u16> {
        let cp = u32::from(c);
        if cp > 0xFFFF {
            return None;
        }
        let cp = cp as u16;
        let d = self.data;
        let t = self.cmap4;
        let seg2 = u16_at(d, t + 6)? as usize;
        let segs = seg2 / 2;
        let ends = t + 14;
        let starts = ends + seg2 + 2;
        let deltas = starts + seg2;
        let ranges = deltas + seg2;
        for s in 0..segs {
            if u16_at(d, ends + s * 2)? < cp {
                continue;
            }
            let start = u16_at(d, starts + s * 2)?;
            if start > cp {
                return Some(0);
            }
            let delta = u16_at(d, deltas + s * 2)?;
            let range = u16_at(d, ranges + s * 2)?;
            if range == 0 {
                return Some(cp.wrapping_add(delta));
            }
            // The famous one: idRangeOffset is a byte offset *from its own
            // slot* into glyphIdArray, not an index.
            let at = ranges + s * 2 + range as usize + (cp - start) as usize * 2;
            let g = u16_at(d, at)?;
            return Some(if g == 0 { 0 } else { g.wrapping_add(delta) });
        }
        Some(0)
    }

    /// The advance width of a glyph in font units. For tests only — see the
    /// module comment on why the pen does not use it.
    pub fn advance(&self, gid: u16) -> Option<f64> {
        let i = (gid as usize).min(self.num_h_metrics.saturating_sub(1));
        Some(f64::from(u16_at(self.data, self.hmtx.0 + i * 4)?))
    }

    /// A glyph's contours, in font units.
    pub fn outline(&self, gid: u16) -> Vec<Curve> {
        let mut out = Vec::new();
        self.outline_into(gid, 0.0, 0.0, 1.0, 1.0, 0, &mut out);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn outline_into(
        &self,
        gid: u16,
        dx: f64,
        dy: f64,
        sx: f64,
        sy: f64,
        depth: u32,
        out: &mut Vec<Curve>,
    ) {
        // A composite glyph may reference a composite glyph. Bounded because a
        // malformed or hostile font can make that a cycle.
        if depth > 5 {
            return;
        }
        let Some(&start) = self.loca.get(gid as usize) else {
            return;
        };
        let Some(&end) = self.loca.get(gid as usize + 1) else {
            return;
        };
        if end <= start {
            return; // an empty glyph, such as the space
        }
        let g = self.glyf.0 + start as usize;
        let Some(n) = i16_at(self.data, g) else {
            return;
        };
        if n >= 0 {
            self.simple(g, n as usize, dx, dy, sx, sy, out);
        } else {
            self.composite(g, dx, dy, sx, sy, depth, out);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn simple(
        &self,
        g: usize,
        contours: usize,
        dx: f64,
        dy: f64,
        sx: f64,
        sy: f64,
        out: &mut Vec<Curve>,
    ) {
        let d = self.data;
        let ends_at = g + 10;
        // The point count is the last contour's end index plus one; there is no
        // separate field for it.
        let Some(total) = contours
            .checked_sub(1)
            .and_then(|k| u16_at(d, ends_at + k * 2))
            .map(|v| v as usize + 1)
        else {
            return;
        };
        let Some(instr_len) = u16_at(d, ends_at + contours * 2) else {
            return;
        };
        let mut p = ends_at + contours * 2 + 2 + instr_len as usize;

        // Flags, run-length encoded by the REPEAT bit.
        let mut flags = Vec::with_capacity(total);
        while flags.len() < total {
            let Some(&f) = d.get(p) else { return };
            p += 1;
            flags.push(f);
            if f & 0x08 != 0 {
                let Some(&r) = d.get(p) else { return };
                p += 1;
                for _ in 0..r {
                    flags.push(f);
                }
            }
        }
        flags.truncate(total);

        // Coordinates: deltas, each 1 or 2 bytes depending on its flag, and
        // the "same" bit doubles as the sign bit for the short form.
        let read = |short: u8, same: u8, p: &mut usize| -> Option<Vec<f64>> {
            let mut v = Vec::with_capacity(total);
            let mut acc = 0i32;
            for &f in &flags {
                if f & short != 0 {
                    let b = i32::from(*d.get(*p)?);
                    *p += 1;
                    acc += if f & same != 0 { b } else { -b };
                } else if f & same == 0 {
                    acc += i32::from(i16_at(d, *p)?);
                    *p += 2;
                }
                v.push(f64::from(acc));
            }
            Some(v)
        };
        let Some(xs) = read(0x02, 0x10, &mut p) else {
            return;
        };
        let Some(ys) = read(0x04, 0x20, &mut p) else {
            return;
        };

        let at = |i: usize| (dx + xs[i] * sx, dy + ys[i] * sy);
        let on = |i: usize| flags[i] & 1 != 0;
        let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);

        let mut first = 0usize;
        for c in 0..contours {
            let Some(last) = u16_at(d, ends_at + c * 2).map(|v| v as usize) else {
                return;
            };
            if last < first || last >= total {
                return;
            }
            let pts: Vec<usize> = (first..=last).collect();
            first = last + 1;
            if pts.is_empty() {
                continue;
            }

            // Start on an on-curve point. A contour may have none at all, in
            // which case the start is the midpoint of the last and first
            // control points and every point is a control point.
            let s = pts.iter().position(|&i| on(i));
            let startp = match s {
                Some(k) => at(pts[k]),
                None => mid(at(pts[pts.len() - 1]), at(pts[0])),
            };
            out.push(Curve::Move(startp.0, startp.1));

            // Which points remain to be consumed, and it differs by case.
            //
            // Starting ON-curve at index k, that point is already the `Move`,
            // so the remaining m - 1 points follow it and the segment back to
            // it is implied by `Close`.
            //
            // Starting off-curve — a contour with no on-curve point at all —
            // the `Move` went to a midpoint that is not one of the points, so
            // every one of the m points is still to come.
            let m = pts.len();
            let seq: Vec<usize> = match s {
                Some(k) => (1..m).map(|i| pts[(k + i) % m]).collect(),
                None => pts.clone(),
            };
            let mut ctrl: Option<(f64, f64)> = None;
            for idx in seq {
                let p = at(idx);
                if on(idx) {
                    match ctrl.take() {
                        Some(c) => out.push(Curve::Quad(c.0, c.1, p.0, p.1)),
                        None => out.push(Curve::Line(p.0, p.1)),
                    }
                } else if let Some(c) = ctrl.replace(p) {
                    // TWO CONTROL POINTS IN A ROW: an on-curve point is
                    // implied at their midpoint. Dropping this rule leaves a
                    // glyph that looks almost right.
                    let implied = mid(c, p);
                    out.push(Curve::Quad(c.0, c.1, implied.0, implied.1));
                }
            }
            if let Some(c) = ctrl {
                out.push(Curve::Quad(c.0, c.1, startp.0, startp.1));
            }
            // No closing `Line` back to the start. `Close` implies it, which is
            // what the pen protocol every other implementation speaks does --
            // fontTools reported exactly one extra command per contour against
            // an earlier version of this that emitted one, on 131 of the 190
            // glyph-face pairs.
            out.push(Curve::Close);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn composite(
        &self,
        g: usize,
        dx: f64,
        dy: f64,
        sx: f64,
        sy: f64,
        depth: u32,
        out: &mut Vec<Curve>,
    ) {
        let d = self.data;
        let mut p = g + 10;
        loop {
            let (Some(flags), Some(idx)) = (u16_at(d, p), u16_at(d, p + 2)) else {
                return;
            };
            p += 4;
            let words = flags & 0x0001 != 0;
            let xy = flags & 0x0002 != 0;
            let (a1, a2) = if words {
                let (Some(a), Some(b)) = (i16_at(d, p), i16_at(d, p + 2)) else {
                    return;
                };
                p += 4;
                (f64::from(a), f64::from(b))
            } else {
                let (Some(&a), Some(&b)) = (d.get(p), d.get(p + 1)) else {
                    return;
                };
                p += 2;
                (f64::from(a as i8), f64::from(b as i8))
            };
            // F2Dot14: a signed 16-bit fixed-point value with two integer bits.
            let f2 = |v: i16| f64::from(v) / 16384.0;
            let (mut csx, mut csy) = (1.0, 1.0);
            if flags & 0x0008 != 0 {
                let Some(s) = i16_at(d, p) else { return };
                p += 2;
                csx = f2(s);
                csy = csx;
            } else if flags & 0x0040 != 0 {
                let (Some(a), Some(b)) = (i16_at(d, p), i16_at(d, p + 2)) else {
                    return;
                };
                p += 4;
                csx = f2(a);
                csy = f2(b);
            } else if flags & 0x0080 != 0 {
                // A full 2x2, of which ONLY THE DIAGONAL IS HONOURED. An
                // off-diagonal term is a rotation or a shear, and a component
                // drawn without it is a wrong glyph that looks like a right one.
                //
                // Saying so here is all this function can do about it: there is
                // no report channel out of `outline` — it returns a
                // `Vec<Curve>` and cannot fail — so the limit is stated, not
                // signalled. An earlier version of this comment claimed the
                // case was "recorded rather than hidden", which was true of
                // nothing: no report, no log, no return value says so.
                //
                // NO COMMITTED FACE REACHES THIS BRANCH, or either of the two
                // above it. Of 2,131 components across 1,076 composite glyphs
                // in Liberation Sans Regular, and 2,149 across 1,092 in Bold,
                // zero set 0x0008, 0x0040 or 0x0080: all 4,280 are a plain
                // offset (measured by walking `glyf` directly, 2026-08-04). A
                // font swap is what would first run this code, so
                // `tests::the_component_transforms_decode` exercises all three
                // branches against a face built byte by byte inside the test
                // rather than against a real one — which is also the only way
                // to pin the F2Dot14 divisor above.
                let (Some(a), Some(_b), Some(_c), Some(dd)) = (
                    i16_at(d, p),
                    i16_at(d, p + 2),
                    i16_at(d, p + 4),
                    i16_at(d, p + 6),
                ) else {
                    return;
                };
                p += 8;
                csx = f2(a);
                csy = f2(dd);
            }
            // ARGS_ARE_XY_VALUES clear means "match point n of this glyph to
            // point m of the composite", which nothing here uses.
            let (ox, oy) = if xy { (a1, a2) } else { (0.0, 0.0) };
            // The offset takes the PARENT's accumulated scale and not this
            // component's own — UNSCALED_COMPONENT_OFFSET (0x1000), which every
            // one of those 4,280 real components sets. The opposite flag,
            // SCALED_COMPONENT_OFFSET (0x0800), is not read: it only differs
            // when a component also carries a scale, and none here does.
            self.outline_into(
                idx,
                dx + ox * sx,
                dy + oy * sy,
                sx * csx,
                sy * csy,
                depth + 1,
                out,
            );
            if flags & 0x0020 == 0 {
                return;
            }
        }
    }
}

/// The regular face this crate rasterizes with.
pub const REGULAR: &[u8] = include_bytes!("../fonts/LiberationSans-Regular.ttf");
/// The bold face. Selected for `Item::Text { bold: true }`, which the SVG emits
/// as `font-weight="600"` and the PDF draws in Helvetica-Bold — CSS font
/// matching resolves 600 to 700 when a family offers only 400 and 700, which is
/// every face in the chain, so all three land on the same weight.
pub const BOLD: &[u8] = include_bytes!("../fonts/LiberationSans-Bold.ttf");

#[cfg(test)]
mod tests;
