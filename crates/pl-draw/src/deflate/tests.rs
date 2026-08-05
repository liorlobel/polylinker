//! Tests for the DEFLATE encoder.
//!
//! # The oracle problem, stated rather than glossed
//!
//! An encoder with no decoder cannot check itself. There are two answers here
//! and they are not the same strength.
//!
//! **`inflate` below is written from RFC 1951, not from the encoder above.** It
//! reads a code-length alphabet and builds a decoding table; the encoder builds
//! a Huffman tree and writes one. They are different algorithms over the same
//! specification, so a round trip catches essentially every encoder bug — a
//! wrong bit order, a missing extra-bit field, an off-by-one in a length base,
//! a tree whose codes are not prefix-free. What it cannot catch is a
//! **shared misreading of the spec**: if both read `HDIST` as the count rather
//! than the count minus one, both agree and both are wrong.
//!
//! **The independent oracle is Python's `zlib`**, in `tools/ci.ps1`. That is a
//! different implementation by different people, and it is the thing that
//! actually proves the file this crate writes is a file the world can read. It
//! is a gate step rather than a unit test because it needs Python, and a
//! contributor with only a Rust toolchain should still get a meaningful
//! `cargo test`.
//!
//! The published CRC-32 and Adler-32 check values are exact and independent of
//! both.

use super::*;

/// Decode a DEFLATE stream, from RFC 1951 and not from the encoder above.
///
/// Deliberately the plain table-free implementation — walk the code bit by bit
/// against the canonical `(first_code, first_index)` per length — because the
/// point is to share as little reasoning with the encoder as possible.
pub(crate) fn inflate(src: &[u8]) -> Result<Vec<u8>, String> {
    struct R<'a> {
        d: &'a [u8],
        pos: usize,
        bit: u32,
    }
    impl R<'_> {
        fn bits(&mut self, n: u32) -> Result<u32, String> {
            let mut v = 0u32;
            for i in 0..n {
                if self.pos >= self.d.len() {
                    return Err("ran off the end".into());
                }
                let b = (self.d[self.pos] >> self.bit) & 1;
                v |= u32::from(b) << i;
                self.bit += 1;
                if self.bit == 8 {
                    self.bit = 0;
                    self.pos += 1;
                }
            }
            Ok(v)
        }
        /// One Huffman symbol, most-significant bit of the code first.
        fn sym(&mut self, len: &[u8]) -> Result<usize, String> {
            let mut count = [0usize; MAX_BITS + 1];
            for &l in len {
                if l > 0 {
                    count[l as usize] += 1;
                }
            }
            let mut code = 0i32;
            let mut first = 0i32;
            let mut index = 0usize;
            for (b, &at) in count.iter().enumerate().skip(1) {
                code |= self.bits(1)? as i32;
                let n = at as i32;
                if code - first < n {
                    let want = index + (code - first) as usize;
                    let mut seen = 0usize;
                    for (s, &l) in len.iter().enumerate() {
                        if l as usize == b {
                            if seen == want - index {
                                return Ok(s);
                            }
                            seen += 1;
                        }
                    }
                    return Err("no symbol for that code".into());
                }
                index += n as usize;
                first = (first + n) << 1;
                code <<= 1;
            }
            Err("code longer than 15 bits".into())
        }
    }

    let mut r = R {
        d: src,
        pos: 0,
        bit: 0,
    };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = r.bits(1)?;
        let btype = r.bits(2)?;
        match btype {
            0 => {
                if r.bit > 0 {
                    r.bit = 0;
                    r.pos += 1;
                }
                if r.pos + 4 > r.d.len() {
                    return Err("stored header truncated".into());
                }
                let n = u16::from_le_bytes([r.d[r.pos], r.d[r.pos + 1]]) as usize;
                let nn = u16::from_le_bytes([r.d[r.pos + 2], r.d[r.pos + 3]]);
                if nn != !(n as u16) {
                    return Err("stored LEN/NLEN disagree".into());
                }
                r.pos += 4;
                out.extend_from_slice(&r.d[r.pos..r.pos + n]);
                r.pos += n;
            }
            1 | 2 => {
                let (ll, dl) = if btype == 1 {
                    let mut ll = vec![8u8; 288];
                    ll[144..256].iter_mut().for_each(|l| *l = 9);
                    ll[256..280].iter_mut().for_each(|l| *l = 7);
                    (ll, vec![5u8; 30])
                } else {
                    let hlit = r.bits(5)? as usize + 257;
                    let hdist = r.bits(5)? as usize + 1;
                    let hclen = r.bits(4)? as usize + 4;
                    let mut cl = [0u8; 19];
                    for &i in CL_ORDER.iter().take(hclen) {
                        cl[i] = r.bits(3)? as u8;
                    }
                    let mut all: Vec<u8> = Vec::with_capacity(hlit + hdist);
                    while all.len() < hlit + hdist {
                        let s = r.sym(&cl)?;
                        match s {
                            0..=15 => all.push(s as u8),
                            16 => {
                                let prev = *all.last().ok_or("repeat with nothing to repeat")?;
                                let n = r.bits(2)? + 3;
                                for _ in 0..n {
                                    all.push(prev);
                                }
                            }
                            17 => {
                                let n = r.bits(3)? as usize + 3;
                                all.resize(all.len() + n, 0);
                            }
                            _ => {
                                let n = r.bits(7)? as usize + 11;
                                all.resize(all.len() + n, 0);
                            }
                        }
                    }
                    if all.len() != hlit + hdist {
                        return Err("code lengths overran their declared count".into());
                    }
                    (all[..hlit].to_vec(), all[hlit..].to_vec())
                };
                loop {
                    let s = r.sym(&ll)?;
                    if s == 256 {
                        break;
                    }
                    if s < 256 {
                        out.push(s as u8);
                        continue;
                    }
                    let i = s - 257;
                    if i >= LEN_BASE.len() {
                        return Err("length code out of range".into());
                    }
                    let len = LEN_BASE[i] as usize + r.bits(u32::from(LEN_EXTRA[i]))? as usize;
                    let ds = r.sym(&dl)?;
                    if ds >= DIST_BASE.len() {
                        return Err("distance code out of range".into());
                    }
                    let dist = DIST_BASE[ds] as usize + r.bits(u32::from(DIST_EXTRA[ds]))? as usize;
                    if dist > out.len() {
                        return Err("distance points before the start of the output".into());
                    }
                    let from = out.len() - dist;
                    for k in 0..len {
                        let b = out[from + k];
                        out.push(b);
                    }
                }
            }
            _ => return Err("reserved block type 3".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

/// The inputs that break compressors, and why each is here.
fn corpus() -> Vec<(&'static str, Vec<u8>)> {
    let mut v: Vec<(&'static str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("one byte", vec![0x42]),
        // Shorter than MIN_MATCH: the whole match path is skipped.
        ("two bytes", vec![1, 2]),
        // Exactly MIN_MATCH, the shortest thing that may become a match.
        ("three bytes", vec![7, 7, 7]),
        // One symbol only. The Huffman tree degenerates to a single leaf, which
        // is the case that has no natural code length and needs the explicit
        // one-bit answer in `build`.
        ("one symbol, long", vec![0xAB; 5000]),
        // No matches at all, so the distance alphabet is empty and the block
        // header still has to declare one.
        ("256 distinct bytes", (0..=255u8).collect()),
    ];
    // A run longer than MAX_MATCH, so one match cannot cover it.
    v.push(("run past 258", vec![0x5A; 258 * 3 + 7]));
    // A match at exactly the window edge: the last distance code, 24577..32768.
    let mut edge = vec![0u8; WINDOW];
    for (i, b) in edge.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
    let head = edge[..64].to_vec();
    edge.extend_from_slice(&head);
    v.push(("window-edge match", edge));
    // Deterministic pseudo-random: nearly incompressible, so the encoder is
    // pushed towards its worst case rather than its best.
    let mut noise = Vec::with_capacity(40000);
    let mut x: u32 = 0x1234_5678;
    for _ in 0..40000 {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        noise.push((x >> 24) as u8);
    }
    v.push(("noise", noise));
    // More symbols than one block holds, so the multi-block path and its
    // BFINAL bookkeeping run.
    let mut many = Vec::with_capacity(400_000);
    let mut y: u32 = 99;
    for _ in 0..400_000 {
        y = y.wrapping_mul(48271) % 0x7FFF_FFFF;
        many.push((y % 7) as u8);
    }
    v.push(("many blocks", many));
    // The shape this crate actually compresses: flat colour, long runs, with
    // antialiased edges between them.
    let mut scan = Vec::with_capacity(3 * 900 * 300);
    for y in 0..300i32 {
        scan.push(0u8); // PNG filter byte
        for x in 0..900i32 {
            let edge = (x - 300 - y / 4).unsigned_abs();
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
    v.push(("map-like scanlines", scan));
    v
}

/// Everything this encoder writes must decode back to what went in.
///
/// The round trip is the whole test: a compressor that loses a byte, transposes
/// a bit field or writes a code the tables cannot express is not "slightly
/// wrong", it produces a file nobody can open. Each corpus entry names the
/// encoder path it exists to reach.
#[test]
fn every_input_survives_the_round_trip() {
    for (name, data) in corpus() {
        let z = deflate(&data);
        match inflate(&z) {
            Ok(back) => assert!(
                back == data,
                "{name}: {} bytes in, {} bytes out, first difference at {:?}",
                data.len(),
                back.len(),
                data.iter().zip(&back).position(|(a, b)| a != b)
            ),
            Err(e) => panic!("{name}: {} bytes would not decode: {e}", data.len()),
        }
    }
}

/// The zlib wrapper must carry a header the spec accepts and the right trailer.
#[test]
fn the_zlib_wrapper_is_well_formed() {
    let data = b"the quick brown fox jumps over the lazy dog, twice; the quick brown fox";
    let z = zlib(data);
    assert_eq!(&z[..2], &[0x78, 0x9C], "not the CM/CINFO/FLG this claims");
    // RFC 1950 s2.2: the first two bytes, as a big-endian 16-bit number, must
    // be a multiple of 31. A header that fails this is rejected outright.
    assert_eq!(
        (u16::from_be_bytes([z[0], z[1]]) % 31),
        0,
        "the header check value is wrong, so every decoder refuses the stream"
    );
    let tail = u32::from_be_bytes([
        z[z.len() - 4],
        z[z.len() - 3],
        z[z.len() - 2],
        z[z.len() - 1],
    ]);
    assert_eq!(
        tail,
        adler32(data),
        "the trailer is not the input's Adler-32"
    );
    assert_eq!(
        inflate(&z[2..z.len() - 4]).expect("the payload decodes"),
        data,
        "the payload between the header and the trailer is not the input"
    );
}

/// CRC-32 and Adler-32 against their published check values.
///
/// Exact, and independent of everything else here: both are the documented
/// result for the string `123456789`, which is the standard check vector for
/// CRC-32/ISO-HDLC and is quoted in RFC 1950 s9's lineage. A checksum that is
/// self-consistently wrong would sail through a round-trip test and produce a
/// PNG every reader rejects at the first chunk.
#[test]
fn the_checksums_match_their_published_check_values() {
    assert_eq!(
        crc32(b"123456789"),
        0xCBF4_3926,
        "CRC-32/ISO-HDLC check value"
    );
    assert_eq!(adler32(b"123456789"), 0x091E_01DE, "Adler-32 check value");
    // The degenerate inputs, where an implementation that forgets its seed
    // still looks right on ordinary data.
    assert_eq!(crc32(b""), 0, "CRC-32 of nothing");
    assert_eq!(adler32(b""), 1, "Adler-32 of nothing is 1, not 0");
}

/// Compression has to actually happen, and by roughly the measured amount.
///
/// A round trip passes just as happily on stored blocks, which would ship the
/// 24.89 MB this file's module comment is about. So the ratio is asserted, not
/// assumed — loosely, because the exact number depends on the match finder, but
/// tightly enough that falling back to literals-only would fail here.
#[test]
fn a_figures_scanlines_actually_compress() {
    let (_, scan) = corpus()
        .into_iter()
        .find(|(n, _)| *n == "map-like scanlines")
        .expect("the map-like case");
    let z = deflate(&scan);
    let ratio = scan.len() as f64 / z.len() as f64;
    assert!(
        ratio > 20.0,
        "flat colour compressed only {ratio:.1}x ({} -> {} bytes); a real \
         encoder gets two orders of magnitude on this and stored blocks get 1x",
        scan.len(),
        z.len()
    );
    // ...and noise must NOT balloon. An encoder that emits a match it cannot
    // afford, or mis-sizes its tables, grows incompressible input.
    let (_, noise) = corpus()
        .into_iter()
        .find(|(n, _)| *n == "noise")
        .expect("the noise case");
    let zn = deflate(&noise);
    assert!(
        zn.len() < noise.len() * 11 / 10,
        "incompressible input grew by more than 10% ({} -> {})",
        noise.len(),
        zn.len()
    );
}

/// The same bytes in must give the same bytes out, every time.
///
/// The crate's promise is byte-identical figures across platforms, and a
/// compressor is where a hash order or a tie broken two ways would hide. This
/// cannot see across machines, but it does catch an encoder whose output
/// depends on anything other than its input.
#[test]
fn compression_is_deterministic() {
    for (name, data) in corpus() {
        let a = deflate(&data);
        let b = deflate(&data);
        assert!(a == b, "{name}: two runs produced different streams");
    }
}

/// `deflate.rs`'s own prose, with the comment markers and the line wrapping
/// taken out, so a claim that spans two comment lines can be searched for as
/// one sentence.
///
/// Reading the module's source back is the only join available here. Every
/// number in that header is a measurement, and a measurement recorded in prose
/// beside code that contradicts it stays green forever — which is what happened
/// to the three claims the tests below now pin.
fn module_prose() -> String {
    const SRC: &str = include_str!("../deflate.rs");
    let mut out = String::new();
    for line in SRC.lines() {
        let t = line.trim_start();
        let Some(body) = t.strip_prefix("//!").or_else(|| t.strip_prefix("///")) else {
            continue;
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(body.trim());
    }
    // Collapse the runs the join above can leave, so an anchor never has to
    // guess how many spaces a table cell or a wrap point produced.
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out
}

/// The number that follows `anchor` in the module's prose.
///
/// Panics naming the anchor when it is absent, because an anchor that silently
/// matched nothing is how a prose test stops being able to fail.
fn number_after(prose: &str, anchor: &str) -> u64 {
    let (_, rest) = prose
        .split_once(anchor)
        .unwrap_or_else(|| panic!("deflate.rs no longer says {anchor:?}"));
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .filter(|c| *c != ',')
        .collect();
    assert!(
        !digits.is_empty(),
        "{anchor:?} in deflate.rs is not followed by a number"
    );
    digits.parse().expect("a decimal integer")
}

/// The fixed-versus-dynamic summary has to be the ratio its own table gives.
///
/// PROVEN TO FAIL against the prose in the working tree before 2026-08-04,
/// which read "1.9×, consistently" directly under a two-row table whose second
/// row is 1391.7/893.3 = 1.56. Observed failure, before the sentence was
/// corrected: `the table gives 1.56× to 1.89×, and the sentence under it does
/// not say so`. The second arm is the belt to that brace — a range that is
/// correct and still called consistent would satisfy the first.
///
/// The ratios are divided out of the table rather than written into this file,
/// so re-measuring a cell and not the sentence under it fails here. The cells
/// themselves are measurements against `zlib`'s `Z_FIXED` that nothing in this
/// repository can re-derive; what this pins is that the summary describes them.
#[test]
fn the_fixed_versus_dynamic_summary_is_the_ratio_its_table_gives() {
    let prose = module_prose();
    // The two rows, read as they are written: `| name | fixed kB | dynamic kB |`.
    let mut ratios: Vec<f64> = Vec::new();
    for row in ["| 2880 px map |", "| 5760 px dense map |"] {
        let (_, rest) = prose
            .split_once(row)
            .unwrap_or_else(|| panic!("deflate.rs no longer carries the table row {row:?}"));
        let cells: Vec<f64> = rest
            .split('|')
            .take(2)
            .map(|c| {
                c.trim()
                    .trim_end_matches(" kB")
                    .parse::<f64>()
                    .unwrap_or_else(|_| panic!("{row} {c:?} is not a kB figure"))
            })
            .collect();
        assert_eq!(cells.len(), 2, "{row} has no fixed and dynamic cell");
        ratios.push(cells[0] / cells[1]);
    }
    let lo = ratios.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = ratios.iter().copied().fold(0.0f64, f64::max);
    // Two decimals, because the spread this exists to record — 1.56 against
    // 1.89 — rounds away at one.
    let claim = format!("{lo:.2}× to {hi:.2}×");
    assert!(
        prose.contains(&claim),
        "the table gives {claim}, and the sentence under it does not say so"
    );
    // The word that made the old sentence wrong rather than merely imprecise:
    // a 21% spread between two rows is not "consistently" anything.
    assert!(
        !prose.contains("consistently"),
        "deflate.rs still calls a {lo:.2}×-to-{hi:.2}× spread consistent"
    );
}

/// The block count in [`build`]'s doc has to be a count of SYMBOLS.
///
/// PROVEN TO FAIL against the prose in the working tree before 2026-08-04,
/// which read "a 25 MB scanline buffer is about 380 blocks". 380 is
/// 24,886,080/65,536 — BYTES divided by [`BLOCK_SYMS`] — but [`deflate`] chunks
/// `lz77(data)`, and `lz77` emits one symbol per match, not one per byte. The
/// failure there is `deflate.rs no longer says "scanline buffer of "`; the
/// arithmetic arms then reject any future sentence that divides bytes again.
///
/// Cheap on purpose. It re-derives the block count from the two figures the
/// comment states rather than re-rendering a 2880 px figure, which is seconds
/// in release and minutes in a debug test binary. The measurement itself is
/// recorded in the comment together with the fixture it was taken on.
#[test]
fn the_block_count_in_the_two_queue_comment_is_a_symbol_count() {
    let prose = module_prose();
    let bytes = number_after(&prose, "scanline buffer of ");
    let syms = number_after(&prose, "turns it into ");
    let blocks = number_after(&prose, "chunks into ");
    let byte_based = number_after(&prose, "not the byte-based ");

    // One filter byte plus three bytes per pixel, per row: what `png::encode`
    // builds, and what the module header's 24.89 MB is.
    assert_eq!(
        bytes,
        2880 * (2880 * 3 + 1),
        "the byte count is not a 2880 × 2880 RGB scanline buffer"
    );
    assert_eq!(
        blocks,
        syms.div_ceil(BLOCK_SYMS as u64),
        "{syms} symbols is {} blocks of {BLOCK_SYMS}, and the comment says {blocks}",
        syms.div_ceil(BLOCK_SYMS as u64)
    );
    assert_eq!(
        byte_based,
        bytes.div_ceil(BLOCK_SYMS as u64),
        "the number the comment quotes as the byte-based one is not {}",
        bytes.div_ceil(BLOCK_SYMS as u64)
    );
    assert_ne!(
        blocks, byte_based,
        "the comment is dividing the byte count by BLOCK_SYMS again"
    );
}

/// Both dpi figures in the module header have to be the dpi they describe.
///
/// PROVEN TO FAIL against the prose in the working tree before 2026-08-04: the
/// header called a 2880 px, 89 mm figure "a little over 600 dpi" while stating
/// the correct 2102 px = 89 mm at 600 dpi twenty-five lines below it, so the
/// file disagreed with itself by 37%. The failure there is `deflate.rs no
/// longer says "an 89 mm figure at "`.
///
/// 89 mm is the single-column width most of `page::PRESETS` uses, so this is
/// the correspondence a reader checks an exported figure against.
#[test]
fn the_dpi_figures_in_the_module_header_are_the_dpi_of_their_pixel_counts() {
    const MM_PER_INCH: f64 = 25.4;
    const COLUMN_MM: f64 = 89.0;
    let prose = module_prose();

    // 2880 px across 89 mm, the figure the whole header is about.
    let dpi = number_after(&prose, "an 89 mm figure at ");
    let want = (2880.0 * MM_PER_INCH / COLUMN_MM).round();
    assert_eq!(
        dpi as f64, want,
        "2880 px across {COLUMN_MM} mm is {want} dpi, and the header says {dpi}"
    );

    // ...and the speed table's row, which was right all along and is what made
    // the two visibly disagree.
    let table_dpi = number_after(&prose, "2102 px — 89 mm at ");
    let px = (table_dpi as f64 * COLUMN_MM / MM_PER_INCH).round();
    assert_eq!(
        px, 2102.0,
        "89 mm at {table_dpi} dpi is {px} px, not the 2102 the table names"
    );
}

/// `BLOCK_SYMS`'s doc states what `lz77` costs per byte of input. This is that
/// arithmetic, taken from the types themselves.
///
/// PROVEN TO FAIL by putting `#[repr(align(8))]` on `Sym`, which takes it to 8
/// bytes without touching a field: `a Sym is not 6 bytes, so the n / 3
/// reservation is not 2 bytes per input byte / left: 8 / right: 6`. Alignment
/// rather than wider fields, because `len_code` and `dist_code` take `u16` and
/// widening the fields does not compile — which is worth knowing in itself: the
/// realistic way this number goes stale is a layout change nobody is looking
/// at.
///
/// That is the failure mode the doc has. The comment before this one accounted
/// for the symbol reservation, left `prev` out entirely — understating the
/// total by four times — and nothing in the suite noticed.
///
/// Not a memory measurement: `tests/memory.rs` is, and it counts the bytes an
/// export really asks the allocator for. This one only holds the two `size_of`
/// values the prose is derived from, so a change to either fails here with the
/// line to edit in the message.
#[test]
fn lz77_holds_the_bytes_per_input_byte_that_block_syms_documents() {
    assert_eq!(
        std::mem::size_of::<Sym>(),
        6,
        "a Sym is not 6 bytes, so the `n / 3` reservation is not 2 bytes per input byte"
    );
    // `prev` is one `usize` per input byte. 32-bit is not a target this ships
    // for, and on one the doc's 8 would be 4 — say so rather than assert it.
    #[cfg(target_pointer_width = "64")]
    {
        let per_byte = std::mem::size_of::<usize>() + std::mem::size_of::<Sym>() / 3;
        assert_eq!(
            per_byte, 10,
            "lz77 holds {per_byte} bytes per input byte, and BLOCK_SYMS's doc says 10"
        );
    }
}
