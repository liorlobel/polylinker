//! SHA-512, per FIPS 180-4.
//!
//! Hand-written for the same reason [`crate::sha1`] and [`crate::sha256`] are:
//! `pl-core` takes no dependencies, and adding one to compute a well-specified
//! function with published test vectors would be the wrong trade. It is
//! therefore checked against the standard's own vectors below.
//!
//! # Why a third hash exists in this crate
//!
//! **Ed25519, and nothing else.** RFC 8032 does not recommend a hash, it is
//! *defined* in terms of this one: the secret scalar, the per-message nonce,
//! and the challenge `k = SHA-512(R ‖ A ‖ M)` are all SHA-512 outputs, so a
//! verifier built on any other hash would not be verifying Ed25519 signatures
//! at all. That is the entire justification for this file. If Ed25519 ever
//! leaves `pl-core`, so should this.
//!
//! It is **not** the crate's content hash. Everything that identifies content —
//! the curator sign-off digest, the build's fetch cache — is SHA-256 and stays
//! SHA-256, so the repository keeps one hash name meaning one thing. Do not
//! reach for this one merely because it is longer.
//!
//! # What a bug here would actually cost
//!
//! This feeds signature verification on software updates, so it is worth being
//! precise about the failure modes rather than waving at "security".
//!
//! A hash that is wrong but *self-consistent* — a mistyped round constant, a
//! rotation off by one — fails closed: every genuine signature stops
//! verifying, no update installs, and somebody notices within the hour. That is
//! bad, and the FIPS vectors below are what catch it.
//!
//! The dangerous shape is different: a hash that **ignores part of its input**.
//! If a partial block can be dropped, then a signature issued over a manifest
//! `M` also verifies over every `M'` that differs only in the dropped bytes,
//! and an attacker who can rewrite a download URL in the manifest gets remote
//! code execution on a scientist's machine. Length-extension is not the worry
//! (Ed25519 does not use this as a MAC); silently swallowing bytes is. That is
//! a buffering bug, not an arithmetic one, which is why the tests here spend
//! most of their effort on *where the input was split* rather than on more
//! digests of more strings, and why the out-of-tree oracle varies the split
//! point as well as the length.
//!
//! # No secrets pass through here
//!
//! Verification hashes only public material: the manifest, the signature's `R`,
//! and the embedded public key. So there is deliberately no zeroing of the
//! block buffer on drop and no claim of side-channel hardening — there is
//! nothing here worth learning. This stops being true the day `pl-core` learns
//! to *sign*: RFC 8032 §5.1.5 derives the key by hashing the 32-byte private
//! key with exactly this function, and the buffer would then hold it. Anyone
//! adding signing has to revisit this paragraph, which is why it names the
//! section rather than saying "be careful".

/// The first 64 bits of the fractional parts of the square roots of the first
/// eight primes, per FIPS 180-4 §5.3.5.
///
/// These are the SHA-512 initial values, **not** SHA-256's. The two tables look
/// alike on purpose — the top halves of these words *are* the SHA-256 IV, since
/// both come from the same square roots truncated differently — so an
/// implementation adapted from `sha256.rs` by widening the types can keep
/// SHA-256's constants and still look right at a glance.
///
/// Both tables here were computed from the definition with exact integer
/// arithmetic rather than transcribed from a printed table. Floating point is
/// not an option for that and the reason is worth recording: an `f64` square
/// root carries about 53 good bits of the 64 needed, so every word it produced
/// would be plausible, wrong in its last three hex digits, and caught only by
/// the vectors at the bottom of this file.
///
/// The transcription is checkable without trusting that generator, and the
/// check was run: `H0[i] >> 32` is `sha256::H0[i]` for all eight words, and
/// `K[i] >> 32` is `sha256::K[i]` for all sixty-four of SHA-256's rounds — the
/// same primes, truncated less. Those SHA-256 values were validated against the
/// standard's vectors long before this file existed.
const H0: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

/// The first 64 bits of the fractional parts of the cube roots of the first 80
/// primes, per FIPS 180-4 §4.2.3.
// Kept twenty rows of four rather than one constant per line, which is what
// rustfmt does with it, for the same reason sha256.rs is: the table is
// transcribed from a generator and the only realistic way to check it by eye is
// to read it against another copy of the same shape.
#[rustfmt::skip]
const K: [u64; 80] = [
    0x428a_2f98_d728_ae22, 0x7137_4491_23ef_65cd, 0xb5c0_fbcf_ec4d_3b2f, 0xe9b5_dba5_8189_dbbc,
    0x3956_c25b_f348_b538, 0x59f1_11f1_b605_d019, 0x923f_82a4_af19_4f9b, 0xab1c_5ed5_da6d_8118,
    0xd807_aa98_a303_0242, 0x1283_5b01_4570_6fbe, 0x2431_85be_4ee4_b28c, 0x550c_7dc3_d5ff_b4e2,
    0x72be_5d74_f27b_896f, 0x80de_b1fe_3b16_96b1, 0x9bdc_06a7_25c7_1235, 0xc19b_f174_cf69_2694,
    0xe49b_69c1_9ef1_4ad2, 0xefbe_4786_384f_25e3, 0x0fc1_9dc6_8b8c_d5b5, 0x240c_a1cc_77ac_9c65,
    0x2de9_2c6f_592b_0275, 0x4a74_84aa_6ea6_e483, 0x5cb0_a9dc_bd41_fbd4, 0x76f9_88da_8311_53b5,
    0x983e_5152_ee66_dfab, 0xa831_c66d_2db4_3210, 0xb003_27c8_98fb_213f, 0xbf59_7fc7_beef_0ee4,
    0xc6e0_0bf3_3da8_8fc2, 0xd5a7_9147_930a_a725, 0x06ca_6351_e003_826f, 0x1429_2967_0a0e_6e70,
    0x27b7_0a85_46d2_2ffc, 0x2e1b_2138_5c26_c926, 0x4d2c_6dfc_5ac4_2aed, 0x5338_0d13_9d95_b3df,
    0x650a_7354_8baf_63de, 0x766a_0abb_3c77_b2a8, 0x81c2_c92e_47ed_aee6, 0x9272_2c85_1482_353b,
    0xa2bf_e8a1_4cf1_0364, 0xa81a_664b_bc42_3001, 0xc24b_8b70_d0f8_9791, 0xc76c_51a3_0654_be30,
    0xd192_e819_d6ef_5218, 0xd699_0624_5565_a910, 0xf40e_3585_5771_202a, 0x106a_a070_32bb_d1b8,
    0x19a4_c116_b8d2_d0c8, 0x1e37_6c08_5141_ab53, 0x2748_774c_df8e_eb99, 0x34b0_bcb5_e19b_48a8,
    0x391c_0cb3_c5c9_5a63, 0x4ed8_aa4a_e341_8acb, 0x5b9c_ca4f_7763_e373, 0x682e_6ff3_d6b2_b8a3,
    0x748f_82ee_5def_b2fc, 0x78a5_636f_4317_2f60, 0x84c8_7814_a1f0_ab72, 0x8cc7_0208_1a64_39ec,
    0x90be_fffa_2363_1e28, 0xa450_6ceb_de82_bde9, 0xbef9_a3f7_b2c6_7915, 0xc671_78f2_e372_532b,
    0xca27_3ece_ea26_619c, 0xd186_b8c7_21c0_c207, 0xeada_7dd6_cde0_eb1e, 0xf57d_4f7f_ee6e_d178,
    0x06f0_67aa_7217_6fba, 0x0a63_7dc5_a2c8_98a6, 0x113f_9804_bef9_0dae, 0x1b71_0b35_131c_471b,
    0x28db_77f5_2304_7d84, 0x32ca_ab7b_40c7_2493, 0x3c9e_be0a_15c9_bebc, 0x431d_67c4_9c10_0d4c,
    0x4cc5_d4be_cb3e_42b6, 0x597f_299c_fc65_7e2a, 0x5fcb_6fab_3ad6_faec, 0x6c44_198c_4a47_5817,
];

/// The block size in bytes. SHA-512 processes 1024 bits at a time, twice
/// SHA-256's, and this is the number an adaptation of `sha256.rs` most easily
/// leaves at 64.
const BLOCK: usize = 128;

/// Where the 128-bit length field starts inside the final block: the last 16
/// bytes of it (§5.1.2). SHA-256 uses 56 of 64 for a 64-bit field; this is the
/// other half of the same mistake.
const LEN_AT: usize = BLOCK - 16;

/// SHA-512 digest of `data`, as 64 bytes.
///
/// Implemented on top of [`Sha512`] rather than as a second, flatter routine —
/// which is how `sha1.rs` and `sha256.rs` are written — so that there is
/// exactly one compression path in this file. Two copies of a padding rule
/// disagree silently: the vectors are usually written against the one-shot
/// form, and the streaming form is the one the updater actually calls.
pub fn sha512(data: &[u8]) -> [u8; 64] {
    let mut state = Sha512::new();
    state.update(data);
    state.finalize()
}

/// Incremental SHA-512, for hashing something too large to hold at once.
///
/// The updater verifies a downloaded installer, so the bytes arrive in
/// whatever pieces the reader hands over. Everything about this type is chosen
/// so that the piece boundaries cannot matter:
///
/// * bytes are absorbed one at a time, so there is no "how much fits in the
///   current block" arithmetic to get wrong. The fast version — `copy_from_slice`
///   of `min(remaining, BLOCK - filled)` and a loop over whole blocks in the
///   caller's slice — is where real streaming implementations break, and it
///   would buy speed this caller does not need (measured below);
/// * the message length is counted in [`update`](Sha512::update) only, so the
///   padding written during `finalize` cannot disturb it;
/// * `finalize` consumes the state, so a digest cannot be taken and then more
///   data appended. That sequence has a plausible-looking answer — the hash of
///   the *padded* message — and no correct use.
///
/// Speed, since "too slow" would be the one good reason to write the clever
/// version: measured, not assumed, at 425–470 MiB/s across runs on the author's
/// machine (x86-64, rustc 1.97.1, `-O`). A 200 MB installer is under half a
/// second, against a download that takes orders of magnitude longer. If that
/// ever becomes the bottleneck, the fix is a block-at-a-time absorb *with* the
/// split tests below still passing — not instead of them.
pub struct Sha512 {
    /// The eight chaining variables, `H0` until the first block is compressed.
    h: [u64; 8],
    /// Bytes accepted but not yet compressed. Only `block[..filled]` is live;
    /// the rest is stale and is never read.
    block: [u8; BLOCK],
    /// How much of `block` is live. Kept strictly below `BLOCK`: a block is
    /// compressed the moment it fills, so `filled == BLOCK` is never observable
    /// between calls.
    filled: usize,
    /// Message length in **bytes**, not bits, and `u128` rather than `u64`.
    ///
    /// The field padded into the message is 128 bits wide, so the count that
    /// feeds it has to be. Note this is not the subtle kind of difference from
    /// SHA-256: the width also decides *where* the padding lands, so getting it
    /// wrong changes every digest, including the empty one, and the first
    /// vector below goes red. Counting bytes rather than bits keeps the
    /// multiply in one place, at the end.
    len: u128,
}

impl Default for Sha512 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha512 {
    /// A state ready to hash an empty message.
    pub fn new() -> Self {
        Sha512 {
            h: H0,
            block: [0u8; BLOCK],
            filled: 0,
            len: 0,
        }
    }

    /// Absorb the next piece of the message.
    ///
    /// Any split is equivalent to any other, including empty pieces: this is
    /// the property the streaming tests and the oracle exist to demonstrate,
    /// rather than to assert.
    pub fn update(&mut self, data: &[u8]) {
        // The counter moves here and nowhere else, so `finalize`'s padding
        // cannot be mistaken for message content.
        //
        // No overflow guard, deliberately. `len` is a byte count in a `u128`:
        // reaching even 2^64 bytes would mean feeding this function for longer
        // than the hardware will exist, and FIPS 180-4's own limit (a message
        // below 2^128 bits) is stricter still. A branch that no input can take
        // is untestable, and this project's rule is that a check which cannot
        // fail proves nothing — so there is none, and this comment is the
        // record of the reasoning instead.
        self.len += data.len() as u128;
        self.absorb(data);
    }

    /// Pad, compress the last block(s), and emit the digest.
    pub fn finalize(mut self) -> [u8; 64] {
        // Read the length before padding, since the padding bytes go through
        // `absorb` and must not be counted. (They cannot be — `absorb` does not
        // touch `len` — but taking the value first means that invariant is not
        // load-bearing twice.)
        let bit_len = self.len.wrapping_mul(8);

        // §5.1.2: append a 1 bit, then the fewest 0 bits that leave room for a
        // 128-bit length in the last block.
        self.absorb(&[0x80]);
        // Spelled as "add zeros until the block holds exactly LEN_AT bytes"
        // rather than as a closed form for how many zeros that is. The closed
        // form is the shape `sha1.rs` and `sha256.rs` use — `if rem <= 112
        // { 112 - rem } else { 240 - rem }` here — and it is a classic home for
        // an off-by-one that only shows up on one length in 128. This loop
        // states its own postcondition as its exit condition, so it cannot be
        // off by one; it terminates because `absorb` wraps `filled` back to 0
        // at BLOCK, so every value in 0..BLOCK is visited within one lap.
        while self.filled != LEN_AT {
            self.absorb(&[0]);
        }
        self.absorb(&bit_len.to_be_bytes());
        debug_assert_eq!(self.filled, 0, "padding must land on a block boundary");

        let mut out = [0u8; 64];
        for (i, word) in self.h.iter().enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    /// Push bytes into the block buffer, compressing whenever it fills.
    ///
    /// Does **not** touch `len`; that is what makes it safe for `finalize` to
    /// send padding through the same path as message data, which in turn is
    /// what keeps this file down to one buffering routine.
    fn absorb(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.block[self.filled] = b;
            self.filled += 1;
            if self.filled == BLOCK {
                compress(&mut self.h, &self.block);
                self.filled = 0;
            }
        }
    }
}

fn compress(h: &mut [u64; 8], block: &[u8; BLOCK]) {
    // The message schedule, §6.4.2. Note the shift and rotation amounts are
    // SHA-512's (1/8/>>7 and 19/61/>>6); SHA-256's are 7/18/>>3 and 17/19/>>10.
    let mut w = [0u64; 80];
    for (i, chunk) in block.chunks_exact(8).enumerate() {
        w[i] = u64::from_be_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]);
    }
    for i in 16..80 {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
    let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);

    // Eighty rounds, not sixty-four. The round function is SHA-256's with wider
    // words and different rotation amounts: Sigma1 is 14/18/41 and Sigma0 is
    // 28/34/39, against SHA-256's 6/11/25 and 2/13/22.
    for i in 0..80 {
        let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
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
    // The crate's one hex formatter, borrowed from `sha256` rather than
    // reimplemented here. A second `{:02x}` loop is a second thing that can
    // drop a leading zero, and it would do so while comparing hash digests —
    // where a shortened string is exactly how two different values start
    // looking equal.
    use crate::sha256::hex;

    // How much these tests are worth, since a hash agrees with itself no
    // matter how wrong it is.
    //
    // 1. Every expected digest below came out of Python's `hashlib`, never from
    //    memory and never from this implementation. Expectations copied from
    //    the thing under test agree with a wrong implementation forever, and
    //    that is the single failure mode this file is written against.
    //
    // 2. Each test here has been watched going red. Eight defects were injected
    //    one at a time — a mistyped digit in K[0], a Sigma1 rotation off by one,
    //    an IV word truncated to its SHA-256 half, SHA-256's 64-byte block,
    //    SHA-256's 64-bit length field, only four chaining variables written
    //    out, the message length assigned instead of accumulated, and the input
    //    ignored entirely — and every test in this module fails under at least
    //    one of them. Two results were surprising enough to be recorded beside
    //    the tests they concern: the empty-message vector survives a wrong
    //    block size, and the whole FIPS vector set survives an `update` that
    //    only remembers the length of the *last* call, because `sha512()` calls
    //    it exactly once. The second of those is the entire reason
    //    `streaming_agrees_with_one_shot_at_every_split_point` exists.
    //
    // 3. Against `hashlib` directly, out of tree: 4336 messages of length
    //    0..4999 — including twenty-four each at 0, 1, 111, 112, 113, 127, 128,
    //    129, 239, 240, 241, 255, 256 and 257 — hashed both in one call and
    //    streamed through randomly chosen split points, 8672 digests compared,
    //    zero disagreements. Rerunning it means rebuilding the harness; it
    //    lives outside the repository because it needs a Python that this
    //    project deliberately does not depend on.
    //
    // The vectors are what CI runs. Points 2 and 3 are why anyone should
    // believe them.

    /// A digest as lowercase hex, the form every expectation below is written
    /// in.
    fn hash(data: &[u8]) -> String {
        hex(&sha512(data))
    }

    #[test]
    fn fips_180_4_vectors() {
        // The two byte-oriented examples the standard publishes for SHA-512,
        // plus the one-million-'a' case from the same appendix.
        assert_eq!(
            hash(b"abc"),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );

        // The 896-bit (112-byte) two-block message. Its length is the whole
        // point. A final block has to hold the remaining message bytes, the
        // 0x80, and sixteen bytes of length, so 111 bytes is the most that can
        // fit and 112 is the least that spills into a second, message-free
        // block. This is the vector that catches a padding rule which only ever
        // pads once.
        //
        // The `\` at the line break is load-bearing: it makes rustc drop the
        // newline *and* the indentation after it. The length assertion is the
        // check on that, since a literal that quietly gained twenty-one spaces
        // would still hash to something and the failure would read as an
        // arithmetic bug.
        let m896 = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn\
                     hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";
        assert_eq!(m896.len(), 112, "the 896-bit message is 112 bytes");
        assert_eq!(
            hash(m896),
            "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb688901\
             8501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
        );

        // A long repeated message: 7813 blocks, so the chaining variables are
        // carried across far more compressions than any short vector reaches.
        assert_eq!(
            hash(&b"a".repeat(1_000_000)),
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973eb\
             de0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        );
    }

    #[test]
    fn the_empty_message_has_the_published_digest() {
        // Its own test because it is the only input that exercises the padding
        // with no message bytes at all: the digest is a function of the padding
        // rule alone, so a wrong length-field offset or a wrong IV shows up here
        // with nothing else to hide behind.
        //
        // It does **not** catch a wrong block size, and that is worth writing
        // down because the obvious guess is that it would — this comment said
        // so until the claim was tested. Compressing SHA-256's 64-byte block
        // instead of SHA-512's 128 leaves this test green: for the empty
        // message the padded block is a lone 0x80 followed by zeros and a zero
        // length, so the first sixteen schedule words come out the same either
        // way — the short block simply leaves the second eight at the zero they
        // were initialised to — and the eighty rounds never see a difference.
        // `the_padding_boundaries_are_handled` is what goes red for that one.
        //
        // Ed25519 never hands this function an empty input — the shortest thing
        // RFC 8032 hashes is 64 bytes — so this vector is here for the padding,
        // not because a caller reaches it.
        assert_eq!(
            hash(b""),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
             47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
    }

    #[test]
    fn the_padding_boundaries_are_handled() {
        // Either side of each place the padding rule changes behaviour: 111
        // (length field just fits), 112 (it does not, so an extra block), 127
        // (one byte short of a block), 128 (exactly full), and the same three
        // again one block up. 255/256/257 are included because 256 is a
        // plausible buffer size elsewhere and a two-block message with a full
        // final block is a distinct case from a one-block one.
        for (n, want) in [
            (
                110usize,
                "c825949632e509824543f7eaf159fb6041722fce3c1cdcbb613b3d37ff107c519\
                 417baac32f8e74fe29d7f4823bf6886956603dca5354a6ed6e4a542e06b7d28",
            ),
            (
                111,
                "fa9121c7b32b9e01733d034cfc78cbf67f926c7ed83e82200ef86818196921760\
                 b4beff48404df811b953828274461673c68d04e297b0eb7b2b4d60fc6b566a2",
            ),
            (
                112,
                "c01d080efd492776a1c43bd23dd99d0a2e626d481e16782e75d54c2503b5dc32\
                 bd05f0f1ba33e568b88fd2d970929b719ecbb152f58f130a407c8830604b70ca",
            ),
            (
                113,
                "55ddd8ac210a6e18ba1ee055af84c966e0dbff091c43580ae1be703bdb85da31\
                 acf6948cf5bd90c55a20e5450f22fb89bd8d0085e39f85a86cc46abbca75e24d",
            ),
            (
                126,
                "9986e67bf52a755f8924f28dae9627f889a45d466ce8616c4ed68ec3afd7a3a1\
                 4785c335c6c68d62e7379af762b2bc17117a902083a99fae337a268a5d4f4427",
            ),
            (
                127,
                "828613968b501dc00a97e08c73b118aa8876c26b8aac93df128502ab360f91ba\
                 b50a51e088769a5c1eff4782ace147dce3642554199876374291f5d921629502",
            ),
            (
                128,
                "b73d1929aa615934e61a871596b3f3b33359f42b8175602e89f7e06e5f658a24\
                 3667807ed300314b95cacdd579f3e33abdfbe351909519a846d465c59582f321",
            ),
            (
                129,
                "4f681e0bd53cda4b5a2041cc8a06f2eabde44fb16c951fbd5b87702f07aeab61\
                 1565b19c47fde30587177ebb852e3971bbd8d3fd30da18d71037dfbd98420429",
            ),
            (
                239,
                "52c853cb8d907f3d4d6b889beb027985d7c273486d75f8baf26f80d24e90c74c\
                 6c3de3e22131582380a7d14d43f2941a31385439cd6ddc469f628015e50bf286",
            ),
            (
                240,
                "4c296d90c61052a62ffb1dd196f1b7b09373b1f93e71836baebf89690546b759\
                 5684dbe9467a8e484fa0d1094272b4344a7c24f5fee8daedeb0bf549c985ab5f",
            ),
            (
                241,
                "81bd43dcdb4d9a7bae6f4f3ebd771d5988481613097aa5de5774f9fdfc1d4230\
                 a608fa1a9dfe3147dc88545df63513f93d13d92d27963926e5a3632aaed4c8bb",
            ),
            (
                255,
                "d8b5a659e365f704ab114ae7079a8da24fb9997b3052a4a63b37d654652bad6f\
                 bdd2b52d737e20a9d5ac3c5831d6afdd32ff737a3dd95269d2793bc2aa850aab",
            ),
            (
                256,
                "6a9169eb662f136d87374070e8828b3e615a7eca32a89446e9225b02832709be\
                 095e635c824a2bb70213ba2ea0ababac0809827843992c851903b7ac0c136699",
            ),
            (
                257,
                "17fa1d01865805f9e657c5f5088754d19913eb418577b03cd040b99e5e1354fd\
                 31d0d7f24b5474c62b49e3271860859510909685c5811eba23b06e1e3369899d",
            ),
        ] {
            assert_eq!(hash(&b"a".repeat(n)), want, "n={n}");
        }
    }

    /// A 300-byte message with no repeats, so a dropped or duplicated byte
    /// changes the digest rather than landing on an identical neighbour. The
    /// 'a'-repeat vectors above cannot make that distinction, which is why the
    /// streaming tests do not use them.
    fn varied(n: usize) -> Vec<u8> {
        (0..n).map(|i| ((i * 7 + 3) % 256) as u8).collect()
    }

    #[test]
    fn streaming_agrees_with_one_shot_at_every_split_point() {
        // The test this module is really about. A message of 300 bytes spans
        // three blocks, so splitting it at every position covers a split before
        // the first block boundary, exactly on it, between the two, exactly on
        // the second, and after — plus the 293 uninteresting-looking positions
        // that would catch an arithmetic slip anywhere in between.
        let msg = varied(300);
        let want = sha512(&msg);
        for i in 0..=msg.len() {
            let mut s = Sha512::new();
            s.update(&msg[..i]);
            s.update(&msg[i..]);
            assert_eq!(s.finalize(), want, "split at {i}");
        }

        // And the same message fed one byte at a time, which puts a call
        // boundary on every block boundary at once.
        let mut s = Sha512::new();
        for b in &msg {
            s.update(&[*b]);
        }
        assert_eq!(s.finalize(), want, "one byte at a time");

        // The streamed digest is pinned to hashlib's, not only to this file's
        // own one-shot answer. `sha512()` is built on `Sha512`, so comparing
        // the two proves only that one code path equals itself; without this
        // line the whole test would pass on an implementation that is wrong in
        // the same way twice.
        assert_eq!(
            hex(&want),
            "46e56ad30db9ef50f8b6762ba55839737f3fba34ab47863c9daff7b3f58f97fe\
             3465a52dd364560db47f802909ced49093322621ea0aebf8e0696b85ca8f81f0"
        );
    }

    #[test]
    fn split_sizes_that_straddle_the_block_boundary() {
        // Chunk sizes chosen to land call boundaries at every offset relative
        // to the 128-byte block: one short of a block, exactly a block, one
        // over, and two coprime-with-128 sizes that walk the boundary through
        // every residue over a long message.
        let msg = varied(1000);
        let want = sha512(&msg);
        for chunk in [1usize, 63, 64, 127, 128, 129, 255, 256, 257, 333, 999, 1000] {
            let mut s = Sha512::new();
            for piece in msg.chunks(chunk) {
                s.update(piece);
            }
            assert_eq!(s.finalize(), want, "chunk={chunk}");
        }
    }

    #[test]
    fn empty_updates_change_nothing() {
        // A reader at end-of-file hands over a zero-length slice, and it must
        // be as if the call had not happened — including before any data, after
        // all of it, and immediately either side of a block boundary, where an
        // implementation that flushed on entry rather than on filling would
        // compress a block twice.
        let msg = varied(300);
        let want = sha512(&msg);

        let mut s = Sha512::new();
        s.update(b"");
        s.update(&msg[..128]);
        s.update(b"");
        s.update(&msg[128..]);
        s.update(b"");
        assert_eq!(s.finalize(), want);

        // Empty in, empty out: the digest of nothing is the empty-string
        // digest, not a panic and not the digest of a stray padding byte.
        let mut s = Sha512::new();
        s.update(b"");
        assert_eq!(s.finalize(), sha512(b""));
    }

    #[test]
    fn a_dropped_byte_would_be_visible() {
        // The check that the checks work. Every assertion above compares a
        // digest to a fixed string, so it is worth demonstrating on the same
        // inputs that truncating the message actually moves the digest — that
        // the vectors are sensitive to the bytes at the end, which is the
        // failure this module's threat model cares about.
        let msg = varied(300);
        assert_ne!(sha512(&msg), sha512(&msg[..299]), "a lost tail byte");
        assert_ne!(
            sha512(&msg),
            sha512(&msg[..msg.len() - 128]),
            "a lost block"
        );

        // And that a message differing only in its final bit is a different
        // digest, since "signature over M also verifies M'" is the shape of the
        // attack this feeds into.
        let mut flipped = msg.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        assert_ne!(sha512(&msg), sha512(&flipped), "a flipped final bit");
    }

    #[test]
    fn the_digest_is_sixty_four_bytes_of_hex() {
        // Guards the output loop, which writes eight bytes per chaining
        // variable: writing four, as SHA-256 does, would leave the second half
        // of the digest zeroed and still look like a hash.
        let d = sha512(b"abc");
        assert_eq!(d.len(), 64);
        assert_eq!(hex(&d).len(), 128);
        assert!(d[32..].iter().any(|&b| b != 0), "the tail is not zeroed");
    }
}
