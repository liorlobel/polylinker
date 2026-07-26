//! SHA-1, per RFC 3174.
//!
//! Hand-written because `pl-core` takes no dependencies, and because the one
//! thing this is used for — SEGUID checksums — is the project's correctness
//! primitive. A hash that is subtly wrong would agree with itself forever and
//! disagree with every other tool in the field, which is the worst failure mode
//! available. It is therefore checked against the RFC's own vectors below.
//!
//! SHA-1 is used here **because the SEGUID specification says so**, not as a
//! security choice. These are content identifiers, not signatures; collision
//! resistance against an adversary is not a property anything here relies on.

const H0: [u32; 5] = [
    0x6745_2301,
    0xEFCD_AB89,
    0x98BA_DCFE,
    0x1032_5476,
    0xC3D2_E1F0,
];

/// SHA-1 digest of `data`, as 20 bytes.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h = H0;

    // The message is padded to a multiple of 64 bytes: a 0x80 byte, then zeros,
    // then the original length in bits as a big-endian u64.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut tail = Vec::with_capacity(128);
    tail.push(0x80u8);
    // Pad so that (data.len() + 1 + zeros + 8) % 64 == 0.
    let rem = (data.len() + 1) % 64;
    let zeros = if rem <= 56 { 56 - rem } else { 120 - rem };
    tail.extend(std::iter::repeat_n(0u8, zeros));
    tail.extend_from_slice(&bit_len.to_be_bytes());

    let mut block = [0u8; 64];
    let mut filled = 0usize;
    let feed = |byte: u8, h: &mut [u32; 5], block: &mut [u8; 64], filled: &mut usize| {
        block[*filled] = byte;
        *filled += 1;
        if *filled == 64 {
            compress(h, block);
            *filled = 0;
        }
    };
    for &b in data {
        feed(b, &mut h, &mut block, &mut filled);
    }
    for &b in &tail {
        feed(b, &mut h, &mut block, &mut filled);
    }
    debug_assert_eq!(filled, 0, "padding must land on a block boundary");

    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn compress(h: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[i * 4],
            block[i * 4 + 1],
            block[i * 4 + 2],
            block[i * 4 + 3],
        ]);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }

    let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
    for (i, &wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5A82_7999u32),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let temp = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = temp;
    }

    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(d: &[u8]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn rfc3174_vectors() {
        // The four test cases given in RFC 3174 section 7.3.
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            hex(&sha1(&b"a".repeat(1_000_000))),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
        assert_eq!(
            hex(&sha1(
                &b"0123456701234567012345670123456701234567012345670123456701234567".repeat(10)
            )),
            "dea356a2cddd90c7a7ecedc5ebb563934f460452"
        );
    }

    #[test]
    fn empty_input() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn block_boundaries_are_handled() {
        // 55, 56 and 64 bytes exercise the three padding branches: room for the
        // length field, no room so an extra block is needed, and exactly full.
        for n in [54usize, 55, 56, 57, 63, 64, 65, 119, 120, 128] {
            let d = sha1(&b"x".repeat(n));
            assert_eq!(d.len(), 20, "n={n}");
        }
        // Values either side of the awkward boundary, taken from Python's
        // hashlib rather than from memory — the first draft of this test had
        // two of them wrong, and a hash test with invented expectations is
        // worse than no test at all.
        assert_eq!(
            hex(&sha1(&b"a".repeat(55))),
            "c1c8bbdc22796e28c0e15163d20899b65621d65a"
        );
        assert_eq!(
            hex(&sha1(&b"a".repeat(56))),
            "c2db330f6083854c99d4b5bfb6e8f29f201be699"
        );
        assert_eq!(
            hex(&sha1(&b"a".repeat(64))),
            "0098ba824b5c16427bd7a1122a5a442a25ec644d"
        );
    }
}
