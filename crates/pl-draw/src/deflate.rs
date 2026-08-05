//! DEFLATE, zlib, CRC-32 and Adler-32 — the compression a PNG needs.
//!
//! Hand-written because everything under `crates/` takes no dependencies, and
//! because a PNG without a real compressor is not a PNG anybody can use. That
//! second half is measured rather than assumed. On a 2880 × 2880 map — an 89 mm
//! figure at 822 dpi — the scanlines are 24.89 MB. Stored (BTYPE 0) blocks are
//! legal DEFLATE and would ship all 24.89 MB. The same bytes under a real
//! encoder are 144 kB. A 173× file is not a "simpler implementation", it is a
//! broken feature, so LZ77 and Huffman are not optional here.
//!
//! 822 dpi and not "a little over 600", which is what this said until
//! 2026-08-04: 89 mm is 3.5039 in, so 600 dpi is the 2102 px the speed table
//! below names and 2880 px is 822.
//!
//! # Why dynamic Huffman, and not just the fixed tables
//!
//! Fixed (BTYPE 1) blocks need no code-length alphabet, no canonical-code
//! construction and no depth limiting — perhaps 120 lines less than what is
//! here. Measured on this crate's own output, through `zlib`'s `Z_FIXED`:
//!
//! | figure | fixed | dynamic |
//! |---|---|---|
//! | 2880 px map | 272.9 kB | 144.2 kB |
//! | 5760 px dense map | 1391.7 kB | 893.3 kB |
//!
//! 1.56× to 1.89× on the workload this exists to serve. Both ends, because
//! that is what the two rows divide to and they are 21% apart: this line
//! called a flat 1.9× the consistent answer until 2026-08-04, over a table it
//! sits directly under whose second row is 1391.7/893.3. Why the denser figure
//! gains less has not been established and is not guessed at here. Either end
//! is worth the lines for a format whose entire purpose is being attached to a
//! manuscript, which is the argument this paragraph is making;
//! `the_fixed_versus_dynamic_summary_is_the_ratio_its_table_gives` divides the
//! rows above and checks this sentence against them, so the two cannot drift
//! again.
//!
//! # Speed
//!
//! Measured in release on this machine, compressing figure-shaped scanlines:
//!
//! | figure | scanlines | time |
//! |---|---|---|
//! | 1051 px — 89 mm at 300 dpi | 3.3 MB | 32 ms |
//! | 2102 px — 89 mm at 600 dpi | 13.3 MB | 105 ms |
//! | 2880 px | 24.9 MB | 181 ms |
//! | 5760 px | 99.5 MB | 636 ms |
//!
//! About 137 MB/s. An export is not a wait, which is the only bar this had to
//! clear — nothing here is on an interactive path.
//!
//! # Determinism
//!
//! Every figure this crate emits has to be byte-identical on Windows, macOS and
//! Linux — that is the project's stated selling point, and a compressor is
//! exactly where platform drift would hide. So: no floating point anywhere in
//! this file, no hash map iteration, and every sort that decides a code length
//! is by `(frequency, symbol)` so that ties cannot resolve two ways.
//!
//! # How this is checked, and how much each check is worth
//!
//! There is no decompressor in this module, and there is one in its tests. The
//! two are not the same claim and the difference matters.
//!
//! `deflate/tests.rs` carries an `inflate` written from RFC 1951 rather than
//! from the encoder above — it reads a code-length alphabet where the encoder
//! writes one, so a round trip catches a wrong bit order, a missing extra-bit
//! field, an off-by-one in a length base, a tree that is not prefix-free. What
//! it cannot catch is the two of them **misreading the spec the same way**,
//! because one author read it once.
//!
//! So the oracle is Python's `zlib` — CPython's binding to the reference
//! implementation, which is also what will actually open these files. It runs
//! in `tools/ci.ps1` over `crates/pl-draw/tests/zstream.rs`, one-shot and then
//! a byte at a time, because a stream can decode correctly in one call and
//! still be malformed for a reader that consumes it incrementally. That is a
//! gate step rather than a unit test only because it needs Python.
//!
//! Against `zlib -9` over that corpus the totals are within 1% — 202,870 bytes
//! against 202,698, +0.085% — and this encoder comes out smaller on 3 of the 10
//! cases: `one-symbol` (28 against 29), `window-edge` (453 against 479) and
//! `map-scanlines` (1896 against 1897).
//!
//! That said "two of the ten" until 2026-08-04, and nothing was looking: the
//! comparison needs a reference DEFLATE encoder and there is none under
//! `crates/` by design, so no test here could make it. The miscount has the
//! shape you would predict from that — `map-scanlines` is the win
//! [`MAX_CHAIN`]'s table already claims by name, so it was counted there and
//! not here. `xcheck_deflate.py` now re-derives every figure in this paragraph
//! from the same streams, so it goes red rather than stale.

/// The window DEFLATE can reference backwards, in bytes (RFC 1951 §3.2.5).
const WINDOW: usize = 32768;
/// Shortest run worth encoding as a match rather than as literals.
const MIN_MATCH: usize = 3;
/// Longest run a single length code can express.
const MAX_MATCH: usize = 258;
/// How far down one hash chain to walk before taking the best match so far.
///
/// The whole quality/speed dial, and it was measured rather than picked. On the
/// map-like scanlines in `tests.rs`, against `zlib -9`'s 1,897 bytes:
///
/// | `MAX_CHAIN` | bytes |
/// |---|---|
/// | 128 | 2,880 |
/// | 512 | 2,966 |
/// | 2,048 | 1,896 |
/// | 8,192 | 1,896 |
///
/// Two things to read off that. **512 is worse than 128** — a deeper search
/// finds a longer match here and there, and a longer match is not always a
/// better parse, because it displaces the next one. Greedy-plus-lazy parsing is
/// not monotone in search depth and anyone tuning this number should expect
/// that rather than assume a regression. And **2,048 is where it stops paying**,
/// landing one byte under the reference encoder, with 8,192 buying nothing.
///
/// The flat colour a plasmid map is mostly made of is what needs the depth: the
/// chain for a run like `FF FF FF` holds every position in every white region
/// of the figure, and the match that pays is the one a row back, not the twenty
/// nearby ones a short walk finds first.
const MAX_CHAIN: usize = 2048;
/// Symbols per block.
///
/// It does NOT bound the symbol buffer, and this line said it did until
/// 2026-08-04. [`deflate`] calls [`lz77`] over the whole input before any
/// chunking, so every allocation below is sized by the input and none of them
/// by this constant.
///
/// # What an `lz77` costs, per byte of input
///
/// | allocation | bytes per input byte |
/// |---|---|
/// | `prev = vec![usize::MAX; n]` | 8 |
/// | `syms`, reserved at `n / 3` six-byte `Sym` | 2 |
/// | **total, over a fixed 256 KB `head`** | **10** |
///
/// `prev` is the one that matters and the one the previous version of this
/// comment left out entirely: it is four times the symbol reservation, and it
/// is `usize::MAX`-filled, so it is touched, not merely reserved. The caller's
/// input is live throughout on top of that, making 11 bytes held per byte
/// handed in. `deflate/tests.rs` pins the 10.
///
/// For a PNG that input is the filtered scanlines — `3n + h` bytes for an
/// `n`-pixel, `h`-row image — so a 5760 px square map hands in 99.5 MB and
/// `lz77` allocates 995 MB against it. Measured end to end, a PNG export peaks
/// at 36.75 bytes per pixel, of which 30 are this function's; see
/// `crate::PNG_BYTES_PER_PIXEL` for the table.
///
/// **Nothing here caps any of it, and until 2026-08-04 this comment claimed
/// the callers did.** They do not: `bins/pl` bands `--mm` 5..=500 and `--dpi`
/// 72..=2400 one flag at a time, the pixel count is their product, and the
/// `None` branch of `crate::png_at` never reads `--mm` at all — there the
/// count comes from `--width`/`--height` against the 72 pt inch. The real
/// bound is `crate::MAX_PIXELS`, checked by `crate::png_budget` before any of
/// this is reached. It is upstream of this file on purpose: a refusal is only
/// useful where the dpi that caused it is still known.
///
/// What this constant does do is let the tables re-fit as the picture changes.
/// The window is continuous across blocks, so this costs no matches.
const BLOCK_SYMS: usize = 1 << 16;
/// The longest Huffman code DEFLATE permits (RFC 1951 §3.2.7).
const MAX_BITS: usize = 15;

/// Base length for each length code 257..=285.
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
/// Extra bits for each length code 257..=285.
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Base distance for each distance code 0..=29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
/// Extra bits for each distance code 0..=29.
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// The order the code-length code lengths are written in (RFC 1951 §3.2.7).
///
/// Not sorted, and not arbitrary: the lengths most often zero sit last, so
/// `HCLEN` can stop early.
const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Bits into bytes, least-significant bit first.
///
/// DEFLATE packs its own bit fields low-to-high but writes each **Huffman code
/// most-significant bit first** (RFC 1951 §3.1.1). That single asymmetry is the
/// classic way to produce a stream that decodes to almost the right thing, so
/// the two operations are separate methods with the difference stated on each.
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn new() -> Self {
        Bits {
            out: Vec::new(),
            acc: 0,
            n: 0,
        }
    }

    /// `n` bits of `v`, low bit first. For header fields and extra bits.
    fn put(&mut self, v: u32, n: u32) {
        debug_assert!(n <= 24 && (n == 32 || v < (1 << n)));
        self.acc |= v << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push((self.acc & 0xFF) as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code: `n` bits of `v`, **high bit first**.
    fn put_code(&mut self, v: u16, n: u8) {
        for i in (0..n).rev() {
            self.put(((v >> i) & 1) as u32, 1);
        }
    }

    /// Pad to a byte boundary with zeros. Only valid before a stored block.
    fn align(&mut self) {
        if self.n > 0 {
            self.put(0, 8 - self.n);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        self.align();
        self.out
    }
}

/// Canonical Huffman code lengths for `freq`, none longer than [`MAX_BITS`].
///
/// The construction is the ordinary one — repeatedly merge the two least
/// frequent nodes — with two things pinned down.
///
/// **Ties resolve by symbol.** Two symbols of equal frequency must always merge
/// in the same order or the same input produces two different files on two
/// machines, which is the one thing this crate promises never to do.
///
/// **Depth is limited by halving.** If any code comes out longer than 15 bits
/// the stream is invalid, so the frequencies are flattened (`(f + 1) / 2`) and
/// the whole thing is rebuilt. Each pass strictly shrinks the spread between
/// the largest and smallest frequency, so it terminates — in the limit every
/// symbol has frequency 1 and the tree is balanced at ⌈log2 n⌉ ≤ 9 bits for our
/// 286-symbol alphabet. Package-merge would give the optimal length-limited
/// code; this gives a legal one, and the difference is under a byte per block
/// on real data because 15 bits is only reached by wildly skewed input.
fn lengths(freq: &[u32], limit: usize) -> Vec<u8> {
    let n = freq.len();
    let mut f: Vec<u32> = freq.to_vec();
    loop {
        let out = build(&f, n);
        if out.iter().all(|&l| (l as usize) <= limit) {
            return out;
        }
        for v in f.iter_mut() {
            if *v > 0 {
                *v = (*v).div_ceil(2);
            }
        }
    }
}

/// One Huffman pass, without the depth limit.
///
/// The two-queue construction, not a heap: the leaves are sorted once, and each
/// merge appends to a FIFO whose frequencies are already nondecreasing, so the
/// smallest node is always at the front of one queue or the other. That is
/// O(n log n) once rather than a re-sort per merge.
///
/// HOW OFTEN THAT HAPPENS, measured rather than divided out. On the figure
/// `crates/pl-draw/tests/render.rs` draws, rasterized to 2880 px: a scanline
/// buffer of 24,886,080 bytes; [`lz77`] turns it into 206,631 symbols, which
/// chunks into 4 blocks of [`BLOCK_SYMS`], three trees each — and emphatically
/// not the byte-based 380 this comment quoted until 2026-08-04. That 380 was
/// the BYTE count over [`BLOCK_SYMS`], but [`deflate`] chunks `lz77(data)`, a
/// stream of symbols, and a symbol on this figure covers 120 bytes of flat
/// colour on average. Twelve trees is not the cost the old figure made it out
/// to be; the property that has to hold either way is the tie order below.
/// `the_block_count_in_the_two_queue_comment_is_a_symbol_count` re-divides both
/// figures so the arithmetic cannot come back.
///
/// Ties break **leaf before internal**, and equal-frequency leaves break by
/// symbol. Both are arbitrary; both are pinned, because an unpinned tie is how
/// one machine's figure stops being byte-identical to another's.
fn build(freq: &[u32], n: usize) -> Vec<u8> {
    let mut leaves: Vec<(u32, usize)> = (0..n)
        .filter(|&s| freq[s] > 0)
        .map(|s| (freq[s], s))
        .collect();
    let mut out = vec![0u8; n];
    match leaves.len() {
        // A block can legally use one symbol; it still needs a code, and a
        // one-bit code is the shortest legal one.
        0 => return out,
        1 => {
            out[leaves[0].1] = 1;
            return out;
        }
        _ => {}
    }
    leaves.sort_unstable();

    // Node ids: leaves are 0..k, internals are k.., so k leaves make k - 1
    // internals and 2k - 1 nodes in all.
    let k = leaves.len();
    let mut parent: Vec<u32> = vec![u32::MAX; 2 * k - 1];
    let mut ifreq: Vec<u32> = Vec::with_capacity(k - 1);
    let (mut li, mut ii) = (0usize, 0usize);

    let take = |li: &mut usize, ii: &mut usize, leaves: &[(u32, usize)], ifreq: &[u32]| {
        let l = (*li < leaves.len()).then(|| leaves[*li].0);
        let i = (*ii < ifreq.len()).then(|| ifreq[*ii]);
        match (l, i) {
            (Some(lf), Some(nf)) if lf <= nf => {
                *li += 1;
                (lf, *li - 1)
            }
            (Some(lf), None) => {
                *li += 1;
                (lf, *li - 1)
            }
            (_, Some(nf)) => {
                *ii += 1;
                (nf, k + *ii - 1)
            }
            (None, None) => unreachable!("the queues cannot both empty before one node remains"),
        }
    };

    for m in 0..(k - 1) {
        let (fa, a) = take(&mut li, &mut ii, &leaves, &ifreq);
        let (fb, b) = take(&mut li, &mut ii, &leaves, &ifreq);
        let node = k + m;
        parent[a] = node as u32;
        parent[b] = node as u32;
        ifreq.push(fa + fb);
    }

    for (i, &(_, s)) in leaves.iter().enumerate() {
        let mut d = 0u32;
        let mut p = parent[i];
        while p != u32::MAX {
            d += 1;
            p = parent[p as usize];
        }
        out[s] = d.min(255) as u8;
    }
    out
}

/// Canonical codes for a set of lengths (RFC 1951 §3.2.2).
fn codes(len: &[u8]) -> Vec<u16> {
    let mut count = [0u16; MAX_BITS + 1];
    for &l in len {
        if l > 0 {
            count[l as usize] += 1;
        }
    }
    let mut next = [0u16; MAX_BITS + 2];
    let mut code = 0u16;
    for b in 1..=MAX_BITS {
        code = (code + count[b - 1]) << 1;
        next[b] = code;
    }
    len.iter()
        .map(|&l| {
            if l == 0 {
                0
            } else {
                let c = next[l as usize];
                next[l as usize] += 1;
                c
            }
        })
        .collect()
}

/// One emitted symbol: a literal, or a match.
#[derive(Clone, Copy)]
enum Sym {
    Lit(u8),
    Match { len: u16, dist: u16 },
}

/// Which length code covers `len`, and how far above its base it sits.
fn len_code(len: u16) -> (usize, u16, u8) {
    let mut i = LEN_BASE.len() - 1;
    while LEN_BASE[i] > len {
        i -= 1;
    }
    (257 + i, len - LEN_BASE[i], LEN_EXTRA[i])
}

/// Which distance code covers `dist`, and how far above its base it sits.
fn dist_code(dist: u16) -> (usize, u16, u8) {
    let mut i = DIST_BASE.len() - 1;
    while DIST_BASE[i] > dist {
        i -= 1;
    }
    (i, dist - DIST_BASE[i], DIST_EXTRA[i])
}

/// LZ77 over the whole input, as a flat symbol stream.
///
/// Hash-chained on three bytes, walking at most [`MAX_CHAIN`] candidates, with
/// **lazy matching**: having found a match at `i`, look again at `i + 1`, and if
/// that one is longer, emit `data[i]` as a literal instead. It is the single
/// cheapest quality win in an LZ77 encoder and costs one comparison.
fn lz77(data: &[u8]) -> Vec<Sym> {
    let n = data.len();
    let mut syms = Vec::with_capacity(n / 3);
    if n < MIN_MATCH {
        syms.extend(data.iter().map(|&b| Sym::Lit(b)));
        return syms;
    }
    // 15-bit hash of three bytes. Fixed constants, so the chain order — and
    // therefore the output — is identical everywhere.
    const HBITS: u32 = 15;
    const HSIZE: usize = 1 << HBITS;
    let h3 = |d: &[u8], i: usize| -> usize {
        ((d[i] as usize) << 10 ^ (d[i + 1] as usize) << 5 ^ (d[i + 2] as usize)) & (HSIZE - 1)
    };
    let mut head = vec![usize::MAX; HSIZE];
    let mut prev = vec![usize::MAX; n];

    let find = |i: usize, head: &[usize], prev: &[usize]| -> (usize, usize) {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        if i + MIN_MATCH > n {
            return (0, 0);
        }
        let mut cand = head[h3(data, i)];
        let floor = i.saturating_sub(WINDOW);
        let mut chain = MAX_CHAIN;
        // How long a match could possibly be from here. Bounded by the input's
        // end as well as by MAX_MATCH, and the probe below reads
        // `data[i + best_len]`, so `best_len < cap` has to hold on entry or
        // that read is off the end of the buffer.
        let cap = MAX_MATCH.min(n - i);
        while cand != usize::MAX && cand >= floor && chain > 0 && best_len < cap {
            chain -= 1;
            // Cheap reject before the byte-by-byte loop: the one byte that
            // would have to match for this candidate to beat the best so far.
            // In range because `best_len < cap` and `cand < i`.
            if best_len == 0 || data[cand + best_len] == data[i + best_len] {
                let mut l = 0usize;
                while l < cap && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                }
            }
            cand = prev[cand];
        }
        if best_len >= MIN_MATCH {
            (best_len, best_dist)
        } else {
            (0, 0)
        }
    };

    let mut i = 0usize;
    while i < n {
        if i + MIN_MATCH > n {
            syms.push(Sym::Lit(data[i]));
            i += 1;
            continue;
        }
        let (mut l, mut d) = find(i, &head, &prev);
        // Insert i into its chain before looking at i + 1, so the lazy probe
        // can see it.
        let h = h3(data, i);
        prev[i] = head[h];
        head[h] = i;

        if l >= MIN_MATCH && i + 1 + MIN_MATCH <= n {
            let (l2, d2) = find(i + 1, &head, &prev);
            if l2 > l {
                syms.push(Sym::Lit(data[i]));
                i += 1;
                l = l2;
                d = d2;
                let h = h3(data, i);
                prev[i] = head[h];
                head[h] = i;
            }
        }

        if l >= MIN_MATCH {
            syms.push(Sym::Match {
                len: l as u16,
                dist: d as u16,
            });
            // Every position a match skipped over still has to enter its hash
            // chain, or later positions cannot find it. Indexed rather than
            // iterated because the body touches three different arrays at `k`
            // and the position itself is the value being stored.
            #[allow(clippy::needless_range_loop)]
            for k in (i + 1)..(i + l) {
                if k + MIN_MATCH <= n {
                    let h = h3(data, k);
                    prev[k] = head[h];
                    head[h] = k;
                }
            }
            i += l;
        } else {
            syms.push(Sym::Lit(data[i]));
            i += 1;
        }
    }
    syms
}

/// The code-length alphabet for a run of lengths, run-length encoded.
///
/// Returns `(symbol, extra_bits_value, extra_bit_count)` triples using codes
/// 16 (repeat previous 3–6), 17 (zero 3–10) and 18 (zero 11–138).
fn rle_lengths(len: &[u8]) -> Vec<(u8, u8, u8)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < len.len() {
        let v = len[i];
        let mut run = 1usize;
        while i + run < len.len() && len[i + run] == v {
            run += 1;
        }
        if v == 0 {
            while run >= 11 {
                let take = run.min(138);
                out.push((18u8, (take - 11) as u8, 7u8));
                run -= take;
                i += take;
            }
            while run >= 3 {
                let take = run.min(10);
                out.push((17u8, (take - 3) as u8, 3u8));
                run -= take;
                i += take;
            }
            for _ in 0..run {
                out.push((0, 0, 0));
                i += 1;
            }
        } else {
            out.push((v, 0, 0));
            i += 1;
            run -= 1;
            while run >= 3 {
                let take = run.min(6);
                out.push((16u8, (take - 3) as u8, 2u8));
                run -= take;
                i += take;
            }
            for _ in 0..run {
                out.push((v, 0, 0));
                i += 1;
            }
        }
    }
    out
}

/// Raw DEFLATE (RFC 1951) for `data`.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let syms = lz77(data);
    let mut w = Bits::new();
    // A zero-length input still needs one final block, or the stream never
    // terminates and every decoder reports truncation.
    if syms.is_empty() {
        w.put(1, 1);
        w.put(0, 2);
        w.align();
        w.out.extend_from_slice(&[0, 0, 0xFF, 0xFF]);
        return w.finish();
    }
    for (bi, chunk) in syms.chunks(BLOCK_SYMS).enumerate() {
        let last = (bi + 1) * BLOCK_SYMS >= syms.len();
        block(&mut w, chunk, last);
    }
    w.finish()
}

/// One dynamic-Huffman block.
fn block(w: &mut Bits, syms: &[Sym], last: bool) {
    let mut lf = [0u32; 286];
    let mut df = [0u32; 30];
    for s in syms {
        match *s {
            Sym::Lit(b) => lf[b as usize] += 1,
            Sym::Match { len, dist } => {
                lf[len_code(len).0] += 1;
                df[dist_code(dist).0] += 1;
            }
        }
    }
    lf[256] += 1; // end of block

    let ll = lengths(&lf, MAX_BITS);
    let mut dl = lengths(&df, MAX_BITS);
    // A block with no matches still has to declare a distance alphabet. One
    // code of one bit is the smallest legal declaration; omitting it makes the
    // header unparseable rather than merely wasteful.
    if dl.iter().all(|&l| l == 0) {
        dl[0] = 1;
    }
    let lc = codes(&ll);
    let dc = codes(&dl);

    let hlit = (257..=286).rev().find(|&i| ll[i - 1] != 0).unwrap_or(257);
    let hdist = (1..=30).rev().find(|&i| dl[i - 1] != 0).unwrap_or(1);

    let mut both: Vec<u8> = ll[..hlit].to_vec();
    both.extend_from_slice(&dl[..hdist]);
    let rle = rle_lengths(&both);

    let mut cf = [0u32; 19];
    for &(s, _, _) in &rle {
        cf[s as usize] += 1;
    }
    // The code-length alphabet is itself Huffman-coded, and its own lengths are
    // written in 3 bits each, so 7 is the hard ceiling here rather than 15.
    let cl = lengths(&cf, 7);
    let cc = codes(&cl);
    let hclen = (4..=19)
        .rev()
        .find(|&i| cl[CL_ORDER[i - 1]] != 0)
        .unwrap_or(4);

    w.put(u32::from(last), 1);
    w.put(2, 2); // BTYPE = dynamic
    w.put((hlit - 257) as u32, 5);
    w.put((hdist - 1) as u32, 5);
    w.put((hclen - 4) as u32, 4);
    for &i in CL_ORDER.iter().take(hclen) {
        w.put(u32::from(cl[i]), 3);
    }
    for &(s, extra, bits) in &rle {
        w.put_code(cc[s as usize], cl[s as usize]);
        if bits > 0 {
            w.put(u32::from(extra), u32::from(bits));
        }
    }
    for s in syms {
        match *s {
            Sym::Lit(b) => w.put_code(lc[b as usize], ll[b as usize]),
            Sym::Match { len, dist } => {
                let (sym, extra, bits) = len_code(len);
                w.put_code(lc[sym], ll[sym]);
                if bits > 0 {
                    w.put(u32::from(extra), u32::from(bits));
                }
                let (dsym, dextra, dbits) = dist_code(dist);
                w.put_code(dc[dsym], dl[dsym]);
                if dbits > 0 {
                    w.put(u32::from(dextra), u32::from(dbits));
                }
            }
        }
    }
    w.put_code(lc[256], ll[256]);
}

/// A zlib stream (RFC 1950) wrapping [`deflate`].
///
/// The header is `0x78 0x9C`: deflate, 32 KB window, default compression, no
/// preset dictionary, and `(0x78 << 8 | 0x9C) % 31 == 0` as §2.2 requires.
pub fn zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x9C];
    out.extend(deflate(data));
    out.extend(adler32(data).to_be_bytes());
    out
}

/// Adler-32 (RFC 1950 §9), the zlib trailer.
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 is the most bytes that cannot overflow a u32 accumulator, so the
    // modulo runs once per chunk instead of once per byte.
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// CRC-32 (ISO 3309 / ITU-T V.42), which every PNG chunk carries.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            // 0xEDB88320 is 0x04C11DB7 bit-reversed: this is the reflected
            // algorithm, which is what PNG specifies.
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

// `pub(crate)` so `png`'s tests can decode an IDAT with the same decoder,
// rather than a second one written to the same misreading.
#[cfg(test)]
pub(crate) mod tests;
