//! Ed25519 **signing**, compiled only under `cfg(test)`, for throwaway keys.
//!
//! # Why this exists at all, when `pl-core` refuses to have it
//!
//! `pl_core::ed25519`'s module doc says a signer must never be added to it, and
//! that is right: none of its arithmetic is constant-time, so a secret scalar
//! flowing through it would leak through timing. Nothing here changes that
//! judgement. This is a different crate, it is behind `#[cfg(test)]` so it is
//! not compiled into any binary this project ships, and the only keys it ever
//! touches are generated three lines earlier in a test.
//!
//! The reason a signer is needed is the house rule about checks that cannot
//! fail. Every requirement this crate has to meet is a *refusal* — a bad
//! signature is refused, a flipped bit is refused, a wrong digest is refused —
//! and a verifier that returned `false` for absolutely everything would satisfy
//! all of them. The only thing that distinguishes "fails closed" from "does not
//! work" is a case where a **good** signature is **accepted**, and there is no
//! way to write one without signing something: the release key's private half
//! is deliberately not on this machine and never will be
//! (`crates/pl-update/src/lib.rs`), so it cannot supply the positive case.
//!
//! # Why it is trustworthy enough to test against
//!
//! Two independent oracles, because either alone has a hole.
//!
//! 1. **RFC 8032 §7.1, bit for bit.** [`the_signer_reproduces_rfc_8032`] feeds
//!    the RFC's published secret keys in and requires both the public key and
//!    the signature to come out exactly as the RFC prints them. That pins every
//!    constant in this file, the clamping, the prefix-derived nonce, and the
//!    byte order, against a document nobody here wrote.
//!
//! 2. **`pl_core::ed25519::verify` accepts what this produces.**
//!    [`what_this_signs_verifies_under_pl_core`] closes the loop with the
//!    verifier that was checked against 150 Wycheproof cases and 35,000 OpenSSL
//!    ones. This is not circular: a self-consistent error here — a wrong base
//!    point, say, where `A = [a]B'` and `R = [r]B'` agree with each other —
//!    would still fail against a verifier computing `[S]B` with the real `B`.
//!
//! The secret keys below are RFC 8032's own published test vectors. They
//! protect nothing, they are printed in an IETF document, and they exist here
//! for the one purpose of proving this file computes what the standard says.
//!
//! `tools/ci.ps1`'s step "no private key material is anywhere in the tree"
//! reads every tracked file and is not fooled by names, so it is worth saying
//! why these do not fall under it rather than leaving the next reader to
//! wonder. That step looks for PEM, OpenSSH and PGP private-key blocks —
//! material that unlocks something. Thirty-two bytes published in RFC 8032 §7.1
//! unlock nothing; the release key's private half is a GitHub Actions secret
//! and has never been in this repository, which is the property that step
//! exists to keep.

use pl_core::sha512::sha512;

// ---------------------------------------------------------------------------
// 256-bit helpers, four little-endian 64-bit limbs
// ---------------------------------------------------------------------------

type Limbs = [u64; 4];

/// `p = 2^255 - 19`.
const P: Limbs = [
    0xffff_ffff_ffff_ffed,
    0xffff_ffff_ffff_ffff,
    0xffff_ffff_ffff_ffff,
    0x7fff_ffff_ffff_ffff,
];

/// `p - 2`, the exponent that inverts a field element by Fermat.
const P_MINUS_2: Limbs = [
    0xffff_ffff_ffff_ffeb,
    0xffff_ffff_ffff_ffff,
    0xffff_ffff_ffff_ffff,
    0x7fff_ffff_ffff_ffff,
];

/// `L = 2^252 + 27742317777372353535851937790883648493`, the group order every
/// scalar is reduced under. Not `p`; see `pl_core::ed25519`, which makes the
/// same distinction at more length.
const L: Limbs = [
    0x5812_631a_5cf5_d3ed,
    0x14de_f9de_a2f7_9cd6,
    0x0000_0000_0000_0000,
    0x1000_0000_0000_0000,
];

fn ge(a: &Limbs, b: &Limbs) -> bool {
    for i in (0..4).rev() {
        if a[i] != b[i] {
            return a[i] > b[i];
        }
    }
    true
}

fn add256(a: &Limbs, b: &Limbs) -> (Limbs, u64) {
    let mut out = [0u64; 4];
    let mut carry = 0u64;
    for i in 0..4 {
        let sum = a[i] as u128 + b[i] as u128 + carry as u128;
        out[i] = sum as u64;
        carry = (sum >> 64) as u64;
    }
    (out, carry)
}

fn sub256(a: &Limbs, b: &Limbs) -> (Limbs, u64) {
    let mut out = [0u64; 4];
    let mut borrow = 0u64;
    for i in 0..4 {
        let diff = (a[i] as i128) - (b[i] as i128) - (borrow as i128);
        out[i] = diff as u64;
        borrow = u64::from(diff < 0);
    }
    (out, borrow)
}

/// Schoolbook 4x4 -> 8 limbs. Used for scalars, where the product is reduced
/// mod `L` afterwards rather than mod `p`.
///
/// `t[i + 4] = carry` assigns rather than accumulates, and that is correct
/// rather than a lost addend: rows `0..i` only ever wrote up to index `i + 3`,
/// so `t[i + 4]` is still zero when row `i` reaches it.
fn mul512(a: &Limbs, b: &Limbs) -> [u64; 8] {
    let mut t = [0u64; 8];
    for i in 0..4 {
        let mut carry = 0u64;
        for j in 0..4 {
            let prod = a[i] as u128 * b[j] as u128 + t[i + j] as u128 + carry as u128;
            t[i + j] = prod as u64;
            carry = (prod >> 64) as u64;
        }
        t[i + 4] = carry;
    }
    t
}

// ---------------------------------------------------------------------------
// The field, GF(2^255 - 19)
// ---------------------------------------------------------------------------

/// A field element, always fully reduced into `[0, p)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Fe(Limbs);

/// Conditional subtraction until the value is below `p`.
///
/// A loop rather than a fixed count for the reason `pl_core::ed25519` gives:
/// the exit condition is the postcondition. It runs at most twice, because the
/// largest possible input `2^256 - 1` is below `3p`.
fn reduce_p(mut v: Limbs) -> Fe {
    while ge(&v, &P) {
        v = sub256(&v, &P).0;
    }
    Fe(v)
}

impl Fe {
    const ZERO: Fe = Fe([0, 0, 0, 0]);
    const ONE: Fe = Fe([1, 0, 0, 0]);

    fn add(&self, other: &Fe) -> Fe {
        let (sum, carry) = add256(&self.0, &other.0);
        debug_assert_eq!(carry, 0, "both operands are below p < 2^255");
        reduce_p(sum)
    }

    fn sub(&self, other: &Fe) -> Fe {
        let (diff, borrow) = sub256(&self.0, &other.0);
        if borrow == 0 {
            Fe(diff)
        } else {
            // `diff` is `self - other + 2^256`; adding `p` and discarding the
            // carry out lands on `self - other + p`, in `(0, p)`.
            Fe(add256(&diff, &P).0)
        }
    }

    /// Schoolbook multiply, then fold the top half back with `2^256 ≡ 38`.
    ///
    /// The same routine as `pl_core::ed25519::Fe::mul`, which argues every
    /// bound in it. The `u128` accumulator is the tight one:
    /// `(2^64-1)^2 + 2(2^64-1)` is exactly `2^128 - 1`.
    fn mul(&self, other: &Fe) -> Fe {
        let t = mul512(&self.0, &other.0);

        // low + 38 * high, five limbs: 38 * (2^256 - 1) < 2^262.
        let mut acc = [0u64; 5];
        let mut carry = 0u128;
        for i in 0..4 {
            let v = t[4 + i] as u128 * 38 + t[i] as u128 + carry;
            acc[i] = v as u64;
            carry = v >> 64;
        }
        acc[4] = carry as u64;

        // Fold the fifth limb the same way; 38 * 38 = 1444, so it is tiny.
        let mut r = [0u64; 4];
        let mut carry = acc[4] as u128 * 38;
        for i in 0..4 {
            let v = acc[i] as u128 + carry;
            r[i] = v as u64;
            carry = v >> 64;
        }
        if carry != 0 {
            debug_assert_eq!(carry, 1);
            r = add256(&r, &[38, 0, 0, 0]).0;
        }
        reduce_p(r)
    }

    fn sq(&self) -> Fe {
        self.mul(self)
    }

    fn pow(&self, exp: &Limbs) -> Fe {
        let mut acc = Fe::ONE;
        for i in (0..256).rev() {
            acc = acc.sq();
            if (exp[i / 64] >> (i % 64)) & 1 == 1 {
                acc = acc.mul(self);
            }
        }
        acc
    }

    /// `self^(p-2)`, which is `1/self` for every non-zero element.
    fn invert(&self) -> Fe {
        self.pow(&P_MINUS_2)
    }

    /// The canonical little-endian encoding. Bit 255 is always clear, because
    /// the value is below `p < 2^255`; that is the bit the sign goes in.
    fn to_bytes(self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, limb) in self.0.iter().enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&limb.to_le_bytes());
        }
        out
    }

    fn is_odd(&self) -> bool {
        self.0[0] & 1 == 1
    }
}

// ---------------------------------------------------------------------------
// The group, edwards25519
// ---------------------------------------------------------------------------

/// `d = -121665 / 121666`. Transcribed from `pl_core::ed25519`, which derives
/// it and checks `d * 121666 + 121665 == 0`; a wrong value here would put every
/// point off the curve and RFC 8032's signatures would not reproduce.
const D: Fe = Fe([
    0x75eb_4dca_1359_78a3,
    0x0070_0a4d_4141_d8ab,
    0x8cc7_4079_7779_e898,
    0x5203_6cee_2b6f_fe73,
]);

/// A point in extended coordinates: `x = X/Z`, `y = Y/Z`, `T = XY/Z`.
#[derive(Clone, Copy)]
struct Point {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

/// The base point `B`, `y = 4/5` with the even `x`, in extended coordinates.
/// Same source and same caveat as [`D`].
const B: Point = Point {
    x: Fe([
        0xc956_2d60_8f25_d51a,
        0x692c_c760_9525_a7b2,
        0xc0a4_e231_fdd6_dc5c,
        0x2169_36d3_cd6e_53fe,
    ]),
    y: Fe([
        0x6666_6666_6666_6658,
        0x6666_6666_6666_6666,
        0x6666_6666_6666_6666,
        0x6666_6666_6666_6666,
    ]),
    z: Fe::ONE,
    t: Fe([
        0x6dde_8ab3_a5b7_dda3,
        0x20f0_9f80_7751_52f5,
        0x66ea_4e8e_64ab_e37d,
        0x6787_5f0f_d78b_7665,
    ]),
};

impl Point {
    const IDENTITY: Point = Point {
        x: Fe::ZERO,
        y: Fe::ONE,
        z: Fe::ONE,
        t: Fe::ZERO,
    };

    /// The Hisil–Wong–Carter–Dawson `a = -1` addition law, which is complete on
    /// this curve — correct for `P + P` and for the identity, with no
    /// exceptional case. One law, exercised by every doubling of every ladder,
    /// for the reason `pl_core::ed25519::Point::add` sets out.
    fn add(&self, other: &Point) -> Point {
        let a = self.y.sub(&self.x).mul(&other.y.sub(&other.x));
        let b = self.y.add(&self.x).mul(&other.y.add(&other.x));
        let c = {
            let dt = self.t.mul(&D).mul(&other.t);
            dt.add(&dt)
        };
        let d = {
            let zz = self.z.mul(&other.z);
            zz.add(&zz)
        };
        let e = b.sub(&a);
        let f = d.sub(&c);
        let g = d.add(&c);
        let h = b.add(&a);
        Point {
            x: e.mul(&f),
            y: g.mul(&h),
            z: f.mul(&g),
            t: e.mul(&h),
        }
    }

    /// `[k]self`, double-and-add from the top bit.
    ///
    /// Not constant-time, and it does not need to be: nothing in this file runs
    /// anywhere an attacker can time it. That is exactly the shortcut
    /// `pl_core::ed25519` refuses to allow near a real secret, and the reason
    /// this signer lives here behind `cfg(test)` rather than there.
    fn mul(&self, k: &Limbs) -> Point {
        let mut acc = Point::IDENTITY;
        for i in (0..256).rev() {
            acc = acc.add(&acc);
            if (k[i / 64] >> (i % 64)) & 1 == 1 {
                acc = acc.add(self);
            }
        }
        acc
    }

    /// RFC 8032 §5.1.2: `y` little-endian, with `x`'s low bit in bit 255.
    fn compress(&self) -> [u8; 32] {
        let zinv = self.z.invert();
        let x = self.x.mul(&zinv);
        let y = self.y.mul(&zinv);
        let mut out = y.to_bytes();
        out[31] |= u8::from(x.is_odd()) << 7;
        out
    }
}

// ---------------------------------------------------------------------------
// Scalars, mod L
// ---------------------------------------------------------------------------

/// A 512-bit little-endian value reduced mod `L`, one bit at a time.
///
/// Lifted from `pl_core::ed25519::scalar_reduce_512`, including its invariant:
/// `r < L` on entry to each iteration, so `2r + bit <= 2L - 1 < 2^253` never
/// leaves the four limbs and one conditional subtraction always restores it.
fn scalar_reduce_512(h: &[u8; 64]) -> Limbs {
    let mut r = [0u64; 4];
    for i in (0..512).rev() {
        let bit = (h[i / 8] >> (i % 8)) & 1;
        let mut carry = bit as u64;
        for limb in r.iter_mut() {
            let next = *limb >> 63;
            *limb = (*limb << 1) | carry;
            carry = next;
        }
        debug_assert_eq!(carry, 0, "2r + 1 must fit in 256 bits");
        if ge(&r, &L) {
            r = sub256(&r, &L).0;
        }
    }
    r
}

/// `(k * a + r) mod L`, the `S` half of a signature.
fn scalar_mul_add(k: &Limbs, a: &Limbs, r: &Limbs) -> Limbs {
    let product = mul512(k, a);
    let mut bytes = [0u8; 64];
    for (i, limb) in product.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&limb.to_le_bytes());
    }
    let mut s = scalar_reduce_512(&bytes);
    // Both addends are below `L < 2^253`, so the sum is below `2^254` and the
    // carry out of `add256` is always zero; one conditional subtraction is
    // enough to land back in `[0, L)`.
    let (sum, carry) = add256(&s, r);
    debug_assert_eq!(carry, 0);
    s = sum;
    if ge(&s, &L) {
        s = sub256(&s, &L).0;
    }
    s
}

fn limbs_from_le(b: &[u8; 32]) -> Limbs {
    let mut out = [0u64; 4];
    for (i, limb) in out.iter_mut().enumerate() {
        let mut le = [0u8; 8];
        le.copy_from_slice(&b[i * 8..i * 8 + 8]);
        *limb = u64::from_le_bytes(le);
    }
    out
}

fn limbs_to_le(s: &Limbs) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, limb) in s.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&limb.to_le_bytes());
    }
    out
}

/// RFC 8032 §5.1.5: SHA-512 the seed, clamp the low half into a scalar, keep
/// the high half as the nonce prefix.
fn expand(seed: &[u8; 32]) -> (Limbs, [u8; 32]) {
    let h = sha512(seed);
    let mut a = [0u8; 32];
    a.copy_from_slice(&h[..32]);
    a[0] &= 248;
    a[31] &= 127;
    a[31] |= 64;
    let mut prefix = [0u8; 32];
    prefix.copy_from_slice(&h[32..]);
    (limbs_from_le(&a), prefix)
}

/// The public key for `seed`, as the 32 bytes `pl_core::ed25519::verify` takes.
pub fn public_key(seed: &[u8; 32]) -> [u8; 32] {
    let (a, _) = expand(seed);
    B.mul(&a).compress()
}

/// An Ed25519 signature over `message` by `seed`, per RFC 8032 §5.1.6.
///
/// Deterministic: the nonce comes from the prefix and the message, so the same
/// inputs always give the same 64 bytes. That is what lets the RFC vectors be
/// compared byte for byte instead of merely verified.
pub fn sign(seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    let (a, prefix) = expand(seed);
    let public = B.mul(&a).compress();

    let mut hash = pl_core::sha512::Sha512::new();
    hash.update(&prefix);
    hash.update(message);
    let r = scalar_reduce_512(&hash.finalize());
    let r_point = B.mul(&r).compress();

    let mut hash = pl_core::sha512::Sha512::new();
    hash.update(&r_point);
    hash.update(&public);
    hash.update(message);
    let k = scalar_reduce_512(&hash.finalize());

    // `a` is the clamped scalar, which is below `2^255` and so may exceed `L`.
    // `scalar_mul_add` reduces the product, which is the same answer as
    // reducing `a` first, and one fewer place to get a reduction wrong.
    let s = scalar_mul_add(&k, &a, &r);

    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&r_point);
    sig[32..].copy_from_slice(&limbs_to_le(&s));
    sig
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut hi = None;
        for c in s.chars().filter(|c| !c.is_whitespace()) {
            let v = c.to_digit(16).expect("hex digit") as u8;
            match hi.take() {
                None => hi = Some(v),
                Some(h) => out.push((h << 4) | v),
            }
        }
        assert!(hi.is_none(), "odd number of hex digits");
        out
    }

    fn a32(s: &str) -> [u8; 32] {
        unhex(s).try_into().expect("32 bytes")
    }

    fn a64(s: &str) -> [u8; 64] {
        unhex(s).try_into().expect("64 bytes")
    }

    /// RFC 8032 §7.1, reproduced exactly.
    ///
    /// The public keys and signatures here are the same strings
    /// `pl_core::ed25519`'s `rfc_vectors` carries — that module deliberately
    /// keeps no secret keys, since it cannot sign — and the secret keys are the
    /// RFC's own, printed beside them in §7.1.
    ///
    /// This is the test that makes every constant above load-bearing. A wrong
    /// limb in `D`, in `B`, in `L`, a clamp applied to the wrong byte, a
    /// little-endian encoding written big-endian: each of them changes these
    /// 64 bytes, and none of them could be caught by a signer checked only
    /// against itself.
    #[test]
    fn the_signer_reproduces_rfc_8032() {
        for (name, seed, public, message, signature) in [
            (
                "TEST 1",
                "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
                "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
                "",
                "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155
                 5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
            ),
            (
                "TEST 2",
                "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
                "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
                "72",
                "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da
                 085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
            ),
            (
                "TEST 3",
                "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
                "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
                "af82",
                "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac
                 18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
            ),
        ] {
            let seed = a32(seed);
            assert_eq!(
                public_key(&seed),
                a32(public),
                "{name}: the derived public key is not the one RFC 8032 prints"
            );
            assert_eq!(
                sign(&seed, &unhex(message)),
                a64(signature),
                "{name}: the signature is not the one RFC 8032 prints"
            );
        }
    }

    /// What this signs, `pl_core::ed25519` accepts — and what it did not sign,
    /// `pl_core::ed25519` refuses.
    ///
    /// The second half is the part that matters. Without it this test would
    /// pass against a verifier that returns `true` unconditionally, which is
    /// the same shape of hole the rest of this crate's tests are written
    /// against.
    #[test]
    fn what_this_signs_verifies_under_pl_core() {
        for i in 0..8u8 {
            let seed = [i.wrapping_mul(37).wrapping_add(11); 32];
            let public = public_key(&seed);
            let message: Vec<u8> = (0..i as usize * 29).map(|n| (n as u8) ^ i).collect();
            let sig = sign(&seed, &message);
            assert!(
                pl_core::ed25519::verify(&public, &message, &sig),
                "seed {i}: a signature this file made must verify"
            );

            // A different message under the same signature.
            let mut other = message.clone();
            other.push(0);
            assert!(!pl_core::ed25519::verify(&public, &other, &sig));

            // Every byte of the signature, flipped in its low bit.
            for b in 0..64 {
                let mut bad = sig;
                bad[b] ^= 1;
                assert!(
                    !pl_core::ed25519::verify(&public, &message, &bad),
                    "seed {i}: flipping signature byte {b} must not still verify"
                );
            }

            // A different key.
            let stranger = public_key(&[i.wrapping_add(1); 32]);
            assert!(!pl_core::ed25519::verify(&stranger, &message, &sig));
        }
    }

    /// Two seeds give two keys, and one seed always gives the same one.
    ///
    /// Cheap, and it catches the failure that would make every other test in
    /// this crate vacuous in the friendliest possible way: a `sign` that
    /// ignored its arguments and returned a fixed 64 bytes would still satisfy
    /// "signed by A verifies under A" for a single A.
    #[test]
    fn distinct_seeds_give_distinct_keys_and_signing_is_deterministic() {
        let a = public_key(&[1; 32]);
        let b = public_key(&[2; 32]);
        assert_ne!(a, b);
        assert_eq!(sign(&[1; 32], b"x"), sign(&[1; 32], b"x"));
        assert_ne!(sign(&[1; 32], b"x"), sign(&[2; 32], b"x"));
        assert_ne!(sign(&[1; 32], b"x"), sign(&[1; 32], b"y"));
    }
}
