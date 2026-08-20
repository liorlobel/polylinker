//! SHA-256, per FIPS 180-4.
//!
//! Hand-written for the same reason [`crate::sha1`] is: `pl-core` takes no
//! dependencies, and adding one to compute a well-specified function with
//! published test vectors would be the wrong trade. It is therefore checked
//! against the standard's own vectors below rather than against itself.
//!
//! # What it is for, and what it is not
//!
//! One caller: the curator sign-off digest in `pl-features`, which hashes the
//! semantic content of a feature row so that a human's approval lapses when the
//! thing they approved changes. That is **stale-approval detection, not
//! authentication** — the builder computes the same digest from the same
//! content and could trivially write a valid-looking one. Nothing here supplies
//! a signature, and `features/SIGNOFF.tsv` says so in its own preamble.
//!
//! SHA-256 rather than the SHA-1 already in this crate because the digest is
//! written into a committed file that outlives the code, and a content hash
//! whose collision resistance is already broken is a bad thing to publish under
//! a name like `content_sha256`. It is also the algorithm the build's fetch
//! cache already records upstream bytes under, so the repository has one hash
//! name meaning one thing.

const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The first 32 bits of the fractional parts of the cube roots of the first 64
/// primes, per FIPS 180-4 §4.2.2.
// Kept eight rows of eight rather than one constant per line, which is what
// rustfmt does with it. The table is transcribed from a specification and the
// only realistic way to check it is to read it against the printed one.
#[rustfmt::skip]
const K: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5,
    0x3956_c25b, 0x59f1_11f1, 0x923f_82a4, 0xab1c_5ed5,
    0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3,
    0x72be_5d74, 0x80de_b1fe, 0x9bdc_06a7, 0xc19b_f174,
    0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc,
    0x2de9_2c6f, 0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da,
    0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
    0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967,
    0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc, 0x5338_0d13,
    0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85,
    0xa2bf_e8a1, 0xa81a_664b, 0xc24b_8b70, 0xc76c_51a3,
    0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070,
    0x19a4_c116, 0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5,
    0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208,
    0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7, 0xc671_78f2,
];

/// SHA-256 digest of `data`, as 32 bytes.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = H0;

    // Padding, §5.1.1: a 0x80 byte, then zeros, then the original length in
    // bits as a big-endian u64, so the whole message is a multiple of 64 bytes.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut tail = Vec::with_capacity(128);
    tail.push(0x80u8);
    let rem = (data.len() + 1) % 64;
    let zeros = if rem <= 56 { 56 - rem } else { 120 - rem };
    tail.extend(std::iter::repeat_n(0u8, zeros));
    tail.extend_from_slice(&bit_len.to_be_bytes());

    let mut block = [0u8; 64];
    let mut filled = 0usize;
    let feed = |byte: u8, h: &mut [u32; 8], block: &mut [u8; 64], filled: &mut usize| {
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

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Lowercase hex, which is the form the digest is written and compared in.
///
/// Provided here rather than left to each caller because a sign-off is a string
/// comparison against a committed file: two callers formatting the same digest
/// two ways would disagree about whether an approval still holds.
pub fn hex(digest: &[u8]) -> String {
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// SHA-256 of `data`, as 64 lowercase hex characters.
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&sha256(data))
}

fn compress(h: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (i, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
        w[i] = u32::from_be_bytes(*chunk);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
    let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);

    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);

        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
    h[5] = h[5].wrapping_add(f);
    h[6] = h[6].wrapping_add(g);
    h[7] = h[7].wrapping_add(hh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fips_180_4_vectors() {
        // The three byte-oriented examples the standard publishes, plus the
        // one-million-'a' case. Taken from a run of Python's hashlib rather
        // than written from memory: a hash test with invented expectations
        // agrees with a wrong implementation forever, which is the one failure
        // mode this whole module exists to avoid.
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            sha256_hex(
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn\
                  hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
                    .iter()
                    .copied()
                    .filter(|b| !b.is_ascii_whitespace())
                    .collect::<Vec<u8>>()
                    .as_slice()
            ),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
        );
        assert_eq!(
            sha256_hex(&b"a".repeat(1_000_000)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn the_empty_string_has_the_published_digest() {
        // Its own test because the sign-off digest hashes empty cells — an
        // absent `reference_aa` contributes a zero-length value — so the empty
        // input is a case the caller really reaches, not a curiosity.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn the_padding_boundaries_are_handled() {
        // 55, 56 and 64 bytes exercise the three padding branches: room for the
        // length field, no room so an extra block is needed, and exactly full.
        // Expected values from hashlib, for the reason given above.
        for (n, want) in [
            (
                54usize,
                "a3f01b6939256127582ac8ae9fb47a382a244680806a3f613a118851c1ca1d47",
            ),
            (
                55,
                "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
            ),
            (
                56,
                "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
            ),
            (
                57,
                "f13b2d724659eb3bf47f2dd6af1accc87b81f09f59f2b75e5c0bed6589dfe8c6",
            ),
            (
                63,
                "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
            ),
            (
                64,
                "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
            ),
            (
                65,
                "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0",
            ),
            (
                119,
                "31eba51c313a5c08226adf18d4a359cfdfd8d2e816b13f4af952f7ea6584dcfb",
            ),
            (
                120,
                "2f3d335432c70b580af0e8e1b3674a7c020d683aa5f73aaaedfdc55af904c21c",
            ),
            (
                128,
                "6836cf13bac400e9105071cd6af47084dfacad4e5e302c94bfed24e013afb73e",
            ),
        ] {
            assert_eq!(sha256_hex(&b"a".repeat(n)), want, "n={n}");
        }
    }

    #[test]
    fn hex_is_lowercase_and_zero_padded() {
        // A digest byte below 0x10 must render as two characters. `{:x}` alone
        // renders 0x0a as "a", which silently shortens the string and makes two
        // different digests compare equal at a different length.
        let d = sha256(b"");
        assert_eq!(hex(&d).len(), 64);
        assert!(hex(&d)
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(hex(&[0x00, 0x0a, 0xff]), "000aff");
    }
}
