//! Ed25519 signature **verification**, per RFC 8032 §5.1.7. No context, no
//! prehash — the plain `Ed25519` instance.
//!
//! Hand-written for the same reason [`crate::sha1`], [`crate::sha256`] and
//! [`crate::sha512`] are: `pl-core` takes no dependencies. That trade is
//! different here and worth stating plainly, because it is the least obviously
//! correct one in this crate. A hash has one right answer per input and the
//! standard prints the answers; a signature verifier has to *refuse* things,
//! and no vector file can enumerate everything it must refuse. What makes it
//! defensible is that the surface is tiny and fixed — one curve, one hash, one
//! equation, 96 bytes of input — and that the refusals it has to get right are
//! a short, published list, reproduced under "What is refused" below.
//!
//! # Verification only. This file must never learn to sign
//!
//! Everything here operates on public data: an embedded public key, a
//! downloaded manifest, and a signature that arrived with it. There is no
//! secret, so there is nothing an attacker could learn by watching how long a
//! branch took, and **none of this code is constant-time**. It branches on
//! secret-free values everywhere and deliberately so: `while v >= p` reduces by
//! repeated subtraction, `pow` is square-and-multiply with a data-dependent
//! multiply, `verify` returns the moment an input is malformed. Each of those
//! is a smaller, more readable, more auditable thing than its constant-time
//! equivalent, and the property that equivalent would buy is one nothing here
//! needs.
//!
//! That is exactly why signing must never be added to this file. RFC 8032
//! §5.1.6 derives the nonce and the scalar from the private key, and every one
//! of the shortcuts above becomes a leak the moment a secret scalar flows
//! through it — `pow`'s multiply pattern is its exponent, and the reduction
//! loop's trip count is its input. A signing routine needs the opposite of
//! every decision made here, so it needs a different file, a different review,
//! and probably a different crate. Adding `sign()` next to `verify()` would
//! silently repurpose code written under the assumption that nothing it touches
//! is worth stealing. The same paragraph is in `sha512.rs` for the same reason.
//!
//! # Which verification equation
//!
//! RFC 8032 §5.1.7 offers two and calls either one conformant: the **cofactored**
//! `[8][S]B = [8]R + [8][k]A` and the **cofactorless** `[S]B = R + [k]A`. This
//! implements the cofactorless one.
//!
//! They are not the same predicate. Multiplying through by 8 annihilates any
//! component of order 1, 2, 4 or 8, so the cofactored equation holds whenever
//! the cofactorless one does and also whenever `[S]B - R - [k]A` is a non-zero
//! point of order dividing 8. Cofactorless is therefore strictly the more
//! refusing of the two, which is the direction to err in for code whose only
//! job is to decide whether to execute a downloaded installer. It is also what
//! the reference implementation (ref10) and libsodium do — both recompute `R`
//! and compare it against the signature's, with no cofactor anywhere — so
//! anything this refuses is refused by the tools a maintainer would reach for
//! to cross-check. A cofactored verifier would instead accept updates that
//! those tools call unsigned, and the disagreement would surface as an
//! unreproducible "but it installed on my machine".
//!
//! The choice is only ever visible for a public key or an `R` carrying a
//! torsion component, and this file refuses small-order keys outright (below),
//! so the remaining window is public keys of mixed order. Those cannot be
//! produced by RFC 8032 key generation — `A = [s]B` with `s` clamped is always
//! in the prime-order subgroup — so no honest signer emits one.
//!
//! # What is refused
//!
//! Each of these is an accepted-forgery bug if it is missing, not a
//! nice-to-have. They are tested individually, and each test also demonstrates
//! that the group equation *would* have held without the rule.
//!
//! * **`S >= L`.** The scalar half of the signature must be the canonical
//!   representative mod `L`. `[S + L]B = [S]B`, so `S + L` satisfies the
//!   equation exactly as well as `S` does: without this check every signature
//!   has a second, different 64-byte spelling. That is signature malleability,
//!   and it is fatal to anything that identifies a release by the hash or the
//!   bytes of its signature.
//! * **Non-canonical point encodings.** A compressed point is a little-endian
//!   `y` in the low 255 bits. `y` must be less than `p`; the 19 values from `p`
//!   to `2^255 - 1` re-encode `0..=18` and would give some points two spellings.
//! * **Points not on the curve.** `x` is recovered from `y`, and when the
//!   required square root does not exist the encoding names no point at all.
//!   Returning some fallback `x` instead would put the rest of the arithmetic
//!   on a curve nobody analysed.
//! * **`x = 0` with the sign bit set.** RFC 8032 §5.1.3 makes this a decoding
//!   failure. `0` and `-0` are the same field element, so accepting it would be
//!   a second spelling of two specific points.
//! * **Small-order public keys**, meaning `[8]A` is the identity. Under such a
//!   key `[k]A` takes at most eight values regardless of the message, and one
//!   signature verifies every message — the test constructs exactly that
//!   forgery. Refusing them costs three point additions and cannot reject a
//!   real Polylinker release, whose key is generated once and shipped inside
//!   the binary. `R` is deliberately *not* subjected to the same rule: a
//!   conforming signer computes `R = [r]B`, which is small-order only if
//!   `r ≡ 0 (mod L)`, and inventing a refusal that a correct signer could trip
//!   would trade a forgery risk for an update-that-will-not-install risk.
//! * **Wrong lengths.** [`verify`] takes arrays, so the compiler enforces it;
//!   [`verify_slices`] is for the caller reading a file, and returns `false`
//!   rather than panicking. Attacker-controlled bytes reach both.
//!
//! Nothing here panics on any input. No array is indexed by a value read from
//! the message, no slice is taken at a length read from it, and the only
//! division anywhere is by the literals 8 and 64 while walking bit positions.
//! Every arithmetic bound is argued at the site that relies on it, and the two
//! that are tight — the `u128` accumulator in `Fe::mul`, and `2r + 1` in
//! `scalar_reduce_512` — say so in as many words.

// ---------------------------------------------------------------------------
// The field, GF(2^255 - 19)
// ---------------------------------------------------------------------------

/// `p = 2^255 - 19`, as four little-endian 64-bit limbs.
///
/// Written out rather than computed because a `const fn` that subtracted 19
/// from `1 << 255` would be a second place to be wrong. It is checked in
/// `the_field_constants_are_what_they_claim_to_be`, which reconstructs it from
/// `2^255 - 19` by ordinary arithmetic on the same limb helpers the field uses.
const P: [u64; 4] = [
    0xffff_ffff_ffff_ffed,
    0xffff_ffff_ffff_ffff,
    0xffff_ffff_ffff_ffff,
    0x7fff_ffff_ffff_ffff,
];

/// `(p - 5) / 8`, the exponent RFC 8032 §5.1.1 uses to take a square root.
///
/// `p ≡ 5 (mod 8)`, so `z^((p-5)/8)` is a fourth root of unity times the square
/// root of `z` when one exists — which is why the decoder has to test the
/// result and fall back on multiplying by `sqrt(-1)`.
const P_MINUS_5_OVER_8: [u64; 4] = [
    0xffff_ffff_ffff_fffd,
    0xffff_ffff_ffff_ffff,
    0xffff_ffff_ffff_ffff,
    0x0fff_ffff_ffff_ffff,
];

/// `L = 2^252 + 27742317777372353535851937790883648493`, the order of the
/// prime-order subgroup, and the modulus every scalar lives under.
///
/// Not `p`. The two are close in size and both appear in this file, and mixing
/// them up is the kind of mistake that leaves most signatures verifying — the
/// scalars in a real signature are far from both bounds — so this is worth a
/// sentence: `p` bounds a *coordinate*, `L` bounds a *scalar*.
const L: [u64; 4] = [
    0x5812_631a_5cf5_d3ed,
    0x14de_f9de_a2f7_9cd6,
    0x0000_0000_0000_0000,
    0x1000_0000_0000_0000,
];

/// An element of GF(2^255 - 19), as four little-endian 64-bit limbs, **always
/// fully reduced into `[0, p)`**.
///
/// That invariant is the central design decision in this file and it was chosen
/// against the usual alternative. Fast Curve25519 implementations carry
/// unreduced values in ten 25.5-bit or five 51-bit limbs and reduce lazily,
/// which turns every operation into a claim about how much slack its inputs
/// still have; get one of those claims wrong and the arithmetic stays correct
/// for almost every input and silently overflows for a rare one. Here a value
/// is a plain 256-bit integer below `p` at every instant, so:
///
/// * equality is `==` on the limbs, with no "are these the same element in two
///   representations" question to answer;
/// * the sign bit of an encoding is `limbs[0] & 1`, exactly;
/// * every bound in this file is a bound on a 256-bit number, and the two that
///   are tight are argued where they occur.
///
/// The cost is a comparison and up to two subtractions per multiply, and it was
/// measured rather than waved at, because "too slow" would be the one good
/// reason to write the clever version. One verification is roughly eight
/// thousand field multiplications — two 256-bit ladders at nine each per bit,
/// plus two decompressions — and runs in **0.13 ms** release, 1.9 ms debug
/// (x86-64, rustc 1.97.1). The thing it gates is a download measured in
/// seconds, so the lazy representation would buy nothing this caller can
/// perceive at a cost this file is written to avoid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Fe([u64; 4]);

/// `a + b + carry`, as `(sum, carry_out)`.
///
/// `u128` rather than `u64::carrying_add`, which *is* stable on this project's
/// compiler (1.97.1) — this comment claimed otherwise until it was tried. The
/// real reason is uniformity: `carrying_add` and `borrowing_sub` hand back a
/// `bool`, while `Fe::mul` has to accumulate a genuine 128-bit product and
/// `widening_mul` is still unstable, so adopting the intrinsics would leave
/// this file with two different ways of thinking about a carry for the sake of
/// two of its three carry chains. One idiom, used everywhere, is worth more
/// than an idiom that fits two thirds of the file.
///
/// The sum of three `u64`s cannot overflow a `u128`, so the `+`s here are
/// ordinary checked arithmetic that never trips.
fn adc(a: u64, b: u64, carry: u64) -> (u64, u64) {
    let t = a as u128 + b as u128 + carry as u128;
    (t as u64, (t >> 64) as u64)
}

/// `a - b - borrow`, as `(difference, borrow_out)` where the borrow is 0 or 1.
///
/// The subtraction is deliberately wrapping: on underflow the `u128` result's
/// bit 64 is set, which is the borrow, and the low 64 bits are the two's
/// complement difference — the answer wanted in both cases.
fn sbb(a: u64, b: u64, borrow: u64) -> (u64, u64) {
    let t = (a as u128)
        .wrapping_sub(b as u128)
        .wrapping_sub(borrow as u128);
    (t as u64, ((t >> 64) as u64) & 1)
}

/// `a >= b`, reading both as 256-bit little-endian integers.
fn ge(a: &[u64; 4], b: &[u64; 4]) -> bool {
    for i in (0..4).rev() {
        if a[i] != b[i] {
            return a[i] > b[i];
        }
    }
    true
}

/// `a + b (mod 2^256)`, with the carry out.
fn add256(a: &[u64; 4], b: &[u64; 4]) -> ([u64; 4], u64) {
    let mut out = [0u64; 4];
    let mut carry = 0u64;
    for i in 0..4 {
        let (v, c) = adc(a[i], b[i], carry);
        out[i] = v;
        carry = c;
    }
    (out, carry)
}

/// `a - b (mod 2^256)`, with the borrow out.
fn sub256(a: &[u64; 4], b: &[u64; 4]) -> ([u64; 4], u64) {
    let mut out = [0u64; 4];
    let mut borrow = 0u64;
    for i in 0..4 {
        let (v, bo) = sbb(a[i], b[i], borrow);
        out[i] = v;
        borrow = bo;
    }
    (out, borrow)
}

/// Bring any 256-bit value into `[0, p)`.
///
/// A loop rather than a fixed pair of conditional subtractions, because the
/// loop's exit condition *is* the postcondition — an unreduced value cannot
/// escape it, whatever the caller's bound turns out to be. It terminates after
/// at most two passes for any input: `2p = 2^256 - 38`, so the largest possible
/// argument, `2^256 - 1`, is less than `3p`.
fn reduce(mut v: [u64; 4]) -> Fe {
    while ge(&v, &P) {
        v = sub256(&v, &P).0;
    }
    Fe(v)
}

impl Fe {
    const ZERO: Fe = Fe([0, 0, 0, 0]);
    const ONE: Fe = Fe([1, 0, 0, 0]);

    fn is_zero(&self) -> bool {
        self.0 == [0, 0, 0, 0]
    }

    /// The sign bit RFC 8032 §5.1.2 writes into bit 255 of an encoding: the
    /// least significant bit of the element's canonical representative.
    ///
    /// Exact only because `Fe` is always canonical. On a lazily-reduced
    /// representation this would have to reduce first, and forgetting to is a
    /// classic way to decode half the points with the wrong sign.
    fn is_odd(&self) -> bool {
        self.0[0] & 1 == 1
    }

    fn add(&self, other: &Fe) -> Fe {
        let (sum, carry) = add256(&self.0, &other.0);
        // Both operands are below `p < 2^255`, so the sum is below `2^256` and
        // no carry can be lost here. Asserted rather than handled: if it ever
        // fires, the invariant this whole type rests on is already broken and
        // quietly folding the carry back in would hide that.
        debug_assert_eq!(carry, 0, "Fe operands must be reduced");
        reduce(sum)
    }

    fn sub(&self, other: &Fe) -> Fe {
        let (diff, borrow) = sub256(&self.0, &other.0);
        if borrow == 0 {
            // `self >= other`, so the difference is already in `[0, p)`.
            Fe(diff)
        } else {
            // `self < other`. `diff` is `self - other + 2^256`; adding `p` and
            // discarding the carry out lands on `self - other + p`, which is in
            // `(0, p)` because `other < p`. The discarded carry is exactly the
            // `2^256` being dropped, so this is not a lost bit.
            Fe(add256(&diff, &P).0)
        }
    }

    fn neg(&self) -> Fe {
        Fe::ZERO.sub(self)
    }

    /// Schoolbook 4x4 -> 8 limb multiply, then fold the top half back in.
    ///
    /// The fold uses `2^256 ≡ 38 (mod p)`, which follows from `2^255 ≡ 19`.
    /// Every step's bound is stated because the whole reason to write this out
    /// rather than reach for a crate is that the bounds are checkable by
    /// reading.
    fn mul(&self, other: &Fe) -> Fe {
        // Step 1: the 512-bit product.
        //
        // The accumulator expression is the tightest arithmetic in this file:
        // `(2^64-1)^2 + (2^64-1) + (2^64-1)` is exactly `2^128 - 1`, so it fits
        // a `u128` with nothing to spare and cannot overflow for any inputs.
        // `t[i + 4]` is zero when row `i` starts, because rows `0..i` only ever
        // wrote up to index `i + 3`.
        let mut t = [0u64; 8];
        for i in 0..4 {
            let mut carry = 0u64;
            for j in 0..4 {
                let prod =
                    self.0[i] as u128 * other.0[j] as u128 + t[i + j] as u128 + carry as u128;
                t[i + j] = prod as u64;
                carry = (prod >> 64) as u64;
            }
            t[i + 4] = carry;
        }

        // Step 2: `low + 38 * high`, which is five limbs. `38 * (2^256 - 1)`
        // is below `2^262`, so the fifth limb holds at most 38 and `acc4`
        // below cannot be large.
        let mut acc = [0u64; 5];
        let mut carry = 0u128;
        for i in 0..4 {
            let v = t[4 + i] as u128 * 38 + t[i] as u128 + carry;
            acc[i] = v as u64;
            carry = v >> 64;
        }
        acc[4] = carry as u64;

        // Step 3: fold that fifth limb in the same way. `38 * 38 = 1444`, so
        // this addend is tiny.
        let mut r = [0u64; 4];
        let mut carry = acc[4] as u128 * 38;
        for i in 0..4 {
            let v = acc[i] as u128 + carry;
            r[i] = v as u64;
            carry = v >> 64;
        }
        // A carry out here means the sum passed `2^256`, which by the same
        // congruence is worth another 38. It cannot happen twice: an overflow
        // implies `acc[0..4] >= 2^256 - 1444`, so `r` is now below 1444 and
        // adding 38 to it cannot reach `2^256`.
        if carry != 0 {
            debug_assert_eq!(carry, 1);
            r = add256(&r, &[38, 0, 0, 0]).0;
        }
        reduce(r)
    }

    fn sq(&self) -> Fe {
        // Deliberately not a specialised squaring routine. The classic one
        // halves the multiplies by exploiting `a_i * a_j == a_j * a_i`, and it
        // is a second place for a carry to go wrong for a saving this caller
        // cannot measure.
        self.mul(self)
    }

    /// `self^exp`, square-and-multiply, most significant bit first.
    ///
    /// The exponent is always one of this file's two public constants, never a
    /// secret, so the data-dependent multiply is not a leak. See the module
    /// doc: this is one of the shortcuts that would have to go if this file
    /// ever signed anything.
    fn pow(&self, exp: &[u64; 4]) -> Fe {
        let mut acc = Fe::ONE;
        for i in (0..256).rev() {
            acc = acc.sq();
            if (exp[i / 64] >> (i % 64)) & 1 == 1 {
                acc = acc.mul(self);
            }
        }
        acc
    }

    /// Read 32 little-endian bytes, refusing any value that is not the
    /// canonical representative.
    ///
    /// `None` for `y >= p`. See "What is refused" in the module doc: those 19
    /// values are second spellings of `0..=18`, and accepting them would mean
    /// one point had two encodings.
    fn from_bytes_canonical(b: &[u8; 32]) -> Option<Fe> {
        let mut limbs = [0u64; 4];
        for (i, limb) in limbs.iter_mut().enumerate() {
            let mut le = [0u8; 8];
            le.copy_from_slice(&b[i * 8..i * 8 + 8]);
            *limb = u64::from_le_bytes(le);
        }
        if ge(&limbs, &P) {
            return None;
        }
        Some(Fe(limbs))
    }
}

/// `d = -121665 / 121666`, the curve parameter of edwards25519.
///
/// Derived, not transcribed: computed as `-121665 * 121666^(p-2) mod p` with
/// exact integer arithmetic. `the_curve_parameter_is_the_right_ratio` checks it
/// the way the definition reads — `d * 121666 + 121665 == 0` — which needs no
/// reference table and no trust in whatever produced these limbs.
const D: Fe = Fe([
    0x75eb_4dca_1359_78a3,
    0x0070_0a4d_4141_d8ab,
    0x8cc7_4079_7779_e898,
    0x5203_6cee_2b6f_fe73,
]);

/// `sqrt(-1) = 2^((p-1)/4)`, the fallback RFC 8032 §5.1.3 applies when the
/// first square-root candidate is off by a factor of `-1`.
///
/// `2` is a quadratic non-residue mod `p` because `p ≡ 5 (mod 8)`, so
/// `2^((p-1)/2) = -1` and this is a square root of `-1`. Derived from that
/// definition; `the_square_root_of_minus_one_squares_to_minus_one` checks both
/// halves — that it squares to `p - 1`, and that it really is `2^((p-1)/4)`
/// computed by this file's own `pow`.
const SQRT_M1: Fe = Fe([
    0xc4ee_1b27_4a0e_a0b0,
    0x2f43_1806_ad2f_e478,
    0x2b4d_0099_3dfb_d7a7,
    0x2b83_2480_4fc1_df0b,
]);

// ---------------------------------------------------------------------------
// The group, edwards25519: -x^2 + y^2 = 1 + d x^2 y^2
// ---------------------------------------------------------------------------

/// A curve point in extended coordinates: `x = X/Z`, `y = Y/Z`, `T = XY/Z`.
///
/// Extended rather than affine because affine addition needs a field inversion
/// per addition, and an inversion here is a 255-step exponentiation — five
/// hundred multiplies to save nine. `Z` is never zero for any point this file
/// can construct; see [`Point::add`].
#[derive(Clone, Copy, Debug)]
struct Point {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

/// The base point `B`, `y = 4/5` with the even `x`.
///
/// Stored in extended coordinates rather than decompressed from its published
/// encoding at run time, so that no code path in [`verify`] has to consider
/// what to do if the base point failed to decode. The two are cross-checked:
/// `the_base_point_is_the_published_one` decompresses
/// `5866666666666666666666666666666666666666666666666666666666666666` — the
/// canonical encoding, the one every specification prints — and asserts it is
/// this point, so the constant is derived from something short enough to check
/// by eye rather than transcribed from sixteen limbs of somebody's table.
///
/// `t` is `x * y`, checked by the same test. Getting it wrong would be a point
/// off the extended-coordinate variety, on which the addition formula below is
/// simply not a group law.
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

    /// The twisted-Edwards addition law in extended coordinates
    /// (Hisil–Wong–Carter–Dawson 2008, the `a = -1` case).
    ///
    /// **There is deliberately no separate doubling routine.** For `a = -1`
    /// with `d` a non-residue — which is edwards25519 — this law is *complete*:
    /// it is correct for every pair of points on the curve, including `P + P`,
    /// including the identity, including points of small order, with no
    /// exceptional case to detect. A dedicated doubling formula would be four
    /// multiplies cheaper per call and would be a second implementation of the
    /// group law whose only untested-by-accident input is `P + P` — which is
    /// precisely what a double-and-add ladder hits 255 times per scalar
    /// multiplication. One law that is always exercised beats two of which one
    /// is exercised by half the vectors.
    ///
    /// `Z3 = F * G = D^2 - C^2 = 4(Z1 Z2)^2 - 4(d T1 T2)^2`, which in affine
    /// terms is `4(1 - d x1 x2 y1 y2)(1 + d x1 x2 y1 y2)`. Completeness is the
    /// statement that neither factor vanishes for points on this curve
    /// (Bernstein–Birkner–Joye–Lange–Peters, "Twisted Edwards Curves",
    /// AFRICACRYPT 2008, which proves it for `a` a square and `d` a
    /// non-residue — both checked for these constants in
    /// `the_curve_parameter_is_the_right_ratio`), so `Z3 != 0`. The
    /// projective comparison in [`Point::same_point`] depends on that, so it is
    /// asserted here — at the one place that could break it — rather than
    /// merely written down. It runs on every point operation of every test
    /// below and has never fired.
    fn add(&self, other: &Point) -> Point {
        let a = self.y.sub(&self.x).mul(&other.y.sub(&other.x));
        let b = self.y.add(&self.x).mul(&other.y.add(&other.x));
        // `C = T1 * (2d) * T2`, spelled as `(T1 * d * T2) + itself` so that
        // `2d` does not have to exist as a fourth curve constant. One constant
        // that a test can tie to the definition of `d` is worth more than one
        // saved addition.
        let c = self.t.mul(&D).mul(&other.t);
        let c = c.add(&c);
        // `D = Z1 * 2 * Z2`, same reasoning.
        let dd = self.z.mul(&other.z);
        let dd = dd.add(&dd);

        let e = b.sub(&a);
        let f = dd.sub(&c);
        let g = dd.add(&c);
        let h = b.add(&a);

        let out = Point {
            x: e.mul(&f),
            y: g.mul(&h),
            z: f.mul(&g),
            t: e.mul(&h),
        };
        debug_assert!(
            !out.z.is_zero(),
            "the addition law is complete on this curve"
        );
        out
    }

    /// `[k]self` by plain double-and-add, most significant bit first.
    ///
    /// No windowing, no signed digits, no precomputed table, and no attempt to
    /// hide `k`: `k` is derived from the signature and the message, both of
    /// which the attacker already has. The complete addition law is what makes
    /// this safe to write so plainly — starting from the identity and doubling
    /// through 253 leading zero bits is an ordinary case here, where on an
    /// incomplete formula it would be the first thing to break.
    fn mul(&self, k: &[u64; 4]) -> Point {
        let mut acc = Point::IDENTITY;
        for i in (0..256).rev() {
            acc = acc.add(&acc);
            if (k[i / 64] >> (i % 64)) & 1 == 1 {
                acc = acc.add(self);
            }
        }
        acc
    }

    /// `[8]self`, by three doublings. Used only to test for small order.
    fn mul_by_cofactor(&self) -> Point {
        let p2 = self.add(self);
        let p4 = p2.add(&p2);
        p4.add(&p4)
    }

    /// Is this the neutral element `(0, 1)`?
    ///
    /// Projectively that is `X = 0` and `Y = Z`. Both halves are needed: `X = 0`
    /// alone also admits `(0, -1)`, the point of order two, and calling that the
    /// identity would let a key of order two pass the small-order check that
    /// exists to catch it.
    fn is_identity(&self) -> bool {
        self.x.is_zero() && self.y == self.z
    }

    /// Do these two projective triples name the same affine point?
    ///
    /// Cross-multiplication, `X1 Z2 == X2 Z1 && Y1 Z2 == Y2 Z1`, rather than
    /// normalising both sides and comparing. Normalising costs two inversions —
    /// about a thousand field multiplies — where this costs four, and more to
    /// the point it has no fallible step to get wrong. It is valid because `Z`
    /// is never zero here; see [`Point::add`].
    fn same_point(&self, other: &Point) -> bool {
        self.x.mul(&other.z) == other.x.mul(&self.z) && self.y.mul(&other.z) == other.y.mul(&self.z)
    }

    /// Decode a compressed point, per RFC 8032 §5.1.3.
    ///
    /// `None` — never a panic, never a fallback point — for every encoding that
    /// does not name exactly one point: `y >= p`, a `y` for which no `x`
    /// exists, and `x = 0` with the sign bit set.
    fn decompress(b: &[u8; 32]) -> Option<Point> {
        let sign = b[31] >> 7;
        // The sign lives in bit 255, so it has to come out before the value is
        // read as a number. Copying first keeps the caller's bytes untouched:
        // `verify` hashes the very same bytes afterwards, and mutating them
        // here would change the challenge.
        let mut raw = *b;
        raw[31] &= 0x7f;
        let y = Fe::from_bytes_canonical(&raw)?;

        // The curve equation solved for x^2:  x^2 = (y^2 - 1) / (d y^2 + 1).
        let yy = y.sq();
        let u = yy.sub(&Fe::ONE);
        let v = D.mul(&yy).add(&Fe::ONE);

        // The candidate root, RFC 8032's `x = u v^3 (u v^7)^((p-5)/8)`. Writing
        // it this way rather than as `(u/v)^((p+3)/8)` avoids an inversion, and
        // it is the form the RFC itself gives, so it can be read against the
        // document.
        let v3 = v.sq().mul(&v);
        let v7 = v3.sq().mul(&v);
        let mut x = u.mul(&v3).mul(&u.mul(&v7).pow(&P_MINUS_5_OVER_8));

        // Now decide which of the three cases holds. `v x^2` is `u` times a
        // fourth root of unity: `+1` if the candidate is right, `-1` if it is
        // off by `sqrt(-1)`, and `+-i` if `u/v` is a non-residue, in which case
        // no square root exists and the encoding names no point.
        //
        // This test is also the on-curve check, and the only one needed:
        // `v x^2 == u` is `(d y^2 + 1) x^2 == y^2 - 1`, which rearranges to
        // `-x^2 + y^2 == 1 + d x^2 y^2`. A point that passes it is on the
        // curve by definition rather than by a separate assertion.
        //
        // There is deliberately no `v == 0` guard. `v = 0` would need
        // `y^2 = -1/d`, and `-1/d` is a non-residue — `-1` is a square and `d`
        // is not, both checked in `the_curve_parameter_is_the_right_ratio` —
        // so no `y` reaches it. Even if one did the test below is still total:
        // `x` would come out `0`, `v x^2` would be `0`, and `u` would not be,
        // so the encoding would be refused rather than mishandled. A guard for
        // a case no input can produce would be a branch no test could cover.
        let vxx = v.mul(&x.sq());
        if vxx != u {
            if vxx == u.neg() {
                x = x.mul(&SQRT_M1);
            } else {
                return None;
            }
        }

        // RFC 8032 §5.1.3 step 4: `x = 0` has no sign, so an encoding that
        // claims one is invalid rather than a request for `-0`.
        if x.is_zero() && sign == 1 {
            return None;
        }
        if x.is_odd() != (sign == 1) {
            x = x.neg();
        }

        Some(Point {
            x,
            y,
            z: Fe::ONE,
            t: x.mul(&y),
        })
    }
}

// ---------------------------------------------------------------------------
// Scalars, mod L
// ---------------------------------------------------------------------------

/// Read the `S` half of a signature, refusing anything that is not the
/// canonical representative mod `L`.
///
/// `None` for `S >= L`. Without this, `S + L` is a second valid signature over
/// the same message under the same key, because `[L]B` is the identity — see
/// "What is refused" in the module doc, and
/// `a_signature_with_a_non_canonical_scalar_is_refused`, which shows the group
/// equation holding for exactly such a value.
fn scalar_canonical(b: &[u8; 32]) -> Option<[u64; 4]> {
    let mut limbs = [0u64; 4];
    for (i, limb) in limbs.iter_mut().enumerate() {
        let mut le = [0u8; 8];
        le.copy_from_slice(&b[i * 8..i * 8 + 8]);
        *limb = u64::from_le_bytes(le);
    }
    if ge(&limbs, &L) {
        return None;
    }
    Some(limbs)
}

/// Reduce a 512-bit little-endian value mod `L`.
///
/// Shift-and-subtract, one bit at a time, which is the schoolbook long
/// division everybody learns and nothing else. The alternatives — Barrett
/// reduction, or ref10's radix-2^21 `sc_reduce` — are much faster and are,
/// respectively, a precomputed reciprocal that has to be right and a long
/// stretch of hand-scheduled limb arithmetic with no intermediate whose value
/// can be stated in a comment.
///
/// The speed does not matter here, and that is a measurement rather than a
/// hope: this loop runs 512 cheap iterations **once** per verification,
/// alongside the roughly eight thousand field multiplications the two scalar
/// ladders cost, so it is a low single-digit percentage of a 0.13 ms
/// operation. It would be worth optimising in a signer, which reduces a scalar
/// on a hot path; there is no signer here and there must not be one.
///
/// The invariant is on every line of it: `r < L` on entry to each iteration, so
/// `2r + bit <= 2L - 1 < 2^254` never leaves the four limbs, and one
/// conditional subtraction is always enough to restore `r < L`.
fn scalar_reduce_512(h: &[u8; 64]) -> [u64; 4] {
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

/// `k = SHA-512(R ‖ A ‖ M) mod L`, the challenge scalar of RFC 8032 §5.1.7.
///
/// A named function rather than four lines inside [`verify`] so that the tests
/// which demonstrate a forgery can compute the very same challenge the verifier
/// does. A test that recomputed it independently would be testing its own copy.
///
/// There is no domain-separation prefix because this is Ed25519, not
/// Ed25519ctx or Ed25519ph: RFC 8032 §5.1 defines `dom2` as the empty string
/// for this instance. Prepending one would make every signature in the world
/// fail to verify, which is at least a loud failure.
fn challenge(r_bytes: &[u8; 32], public_key: &[u8; 32], message: &[u8]) -> [u64; 4] {
    let mut hash = crate::sha512::Sha512::new();
    hash.update(r_bytes);
    hash.update(public_key);
    hash.update(message);
    scalar_reduce_512(&hash.finalize())
}

// ---------------------------------------------------------------------------
// The public entry points
// ---------------------------------------------------------------------------

/// Does `signature` verify `message` under `public_key`?
///
/// `true` only if every one of the checks listed under "What is refused" in the
/// module doc passes and the group equation `[S]B = R + [k]A` holds. `false`
/// for everything else, including malformed input; there is no panic and no
/// error type, because there is nothing a caller could usefully do differently
/// between "this signature is wrong" and "these bytes are not a signature".
///
/// Verification only. Do not add a signing counterpart to this module; the
/// module doc says why at length.
pub fn verify(public_key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let Some(a) = Point::decompress(public_key) else {
        return false;
    };
    // Small-order keys, refused before anything else is computed. Under such a
    // key one signature verifies every message; see the module doc and
    // `a_small_order_public_key_would_verify_anything_so_it_is_refused`.
    if a.mul_by_cofactor().is_identity() {
        return false;
    }

    let mut r_bytes = [0u8; 32];
    r_bytes.copy_from_slice(&signature[..32]);
    let Some(r) = Point::decompress(&r_bytes) else {
        return false;
    };

    let mut s_bytes = [0u8; 32];
    s_bytes.copy_from_slice(&signature[32..]);
    let Some(s) = scalar_canonical(&s_bytes) else {
        return false;
    };

    let k = challenge(&r_bytes, public_key, message);

    // Cofactorless, per the module doc: `[S]B = R + [k]A`.
    B.mul(&s).same_point(&r.add(&a.mul(&k)))
}

/// [`verify`], for a caller holding slices of unknown length.
///
/// This exists because the alternative at every call site is
/// `signature[..64].try_into().unwrap()`, and a manifest file whose signature
/// field is 63 bytes long is exactly the input an attacker supplies. A wrong
/// length is a refusal, not a panic and not a truncation.
pub fn verify_slices(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    if public_key.len() != 32 || signature.len() != 64 {
        return false;
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(public_key);
    let mut sig = [0u8; 64];
    sig.copy_from_slice(signature);
    verify(&key, message, &sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    // How much these tests are worth.
    //
    // 1. **A mistyped vector fails; it cannot pass.** Every positive case below
    //    asserts that a specific 64-byte signature verifies under a specific
    //    key. Get one byte of any of them wrong and verification fails — the
    //    test goes red, and it is red for a reason that reads like an
    //    arithmetic bug. So the vectors were checked, out of tree, against
    //    OpenSSL through Python's `cryptography` package: for each one, that the
    //    public key really is the one the RFC's secret key derives, and that
    //    signing the message with it reproduces the signature byte for byte.
    //    That check found a real error — the second half of the SHA(abc)
    //    signature had been written from memory and was wrong — which is the
    //    reason this note exists rather than an assurance that it was careful.
    //
    // 2. **A verifier that returns `true` for everything passes any suite of
    //    valid signatures.** Every positive case therefore has negative twins:
    //    a bit flipped in `R`, a bit flipped in `S`, a bit flipped in the
    //    message, and the signature offered under a different key.
    //
    // 3. **Each refusal test shows the group equation holding without it.** The
    //    malleable-`S` test and the small-order-key test do not merely assert
    //    `!verify(..)`; they compute `[S]B` and `R + [k]A` with this file's own
    //    primitives and assert they are equal. If either rule were deleted,
    //    those signatures would verify. That is what makes them tests of a rule
    //    rather than tests of a typo.
    //
    // 4. **Watched going red.** Twenty-three defects were injected one at a
    //    time and the module rebuilt against each: a flipped limb in `d`,
    //    `SQRT_M1`, `P`, `L`, `(p-5)/8` and `B.t`; the `y >= p`, `S >= L`,
    //    small-order, `x = 0` sign and on-curve refusals each deleted in turn;
    //    the sign bit ignored, and read from bit 254 instead of 255; `2d`
    //    written as `d` in the addition law; the second conditional subtraction
    //    in `reduce` removed; the 38-fold of the top limb dropped;
    //    `scalar_reduce_512` walking the bits least-significant-first;
    //    `is_identity` reduced to `X == 0`; `same_point` reduced to each of its
    //    two halves; the challenge dropping `A`, and dropping `R`; and `verify`
    //    returning `true` unconditionally. **Every one turned at least one test
    //    below red, and every test below was turned red by at least one of
    //    them** — no test in this module is decorative and no injected defect
    //    survives.
    //
    //    Two of those results changed the tests rather than confirming them,
    //    and are recorded beside the assertions they produced:
    //    `same_point` reduced to its `x` half survived the whole module on the
    //    first run, and `is_identity` reduced to `X == 0` was caught by
    //    nothing until the order-2 point was named explicitly. A third is worth
    //    knowing: removing the *second* pass of `reduce`'s subtraction loop is
    //    caught by exactly one test, so that path is real but rare — do not
    //    "simplify" the loop into a single conditional subtraction.

    fn nibble(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("not a hex digit: {:?}", c as char),
        }
    }

    /// Hex to bytes, ignoring whitespace so the RFC's own line breaks survive.
    fn unhex(s: &str) -> Vec<u8> {
        let digits: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        assert_eq!(digits.len() % 2, 0, "an odd number of hex digits");
        digits
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| nibble(p[0]) * 16 + nibble(p[1]))
            .collect()
    }

    /// A scalar back to the 32 little-endian bytes a signature carries.
    fn scalar_bytes(s: &[u64; 4]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, limb) in s.iter().enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&limb.to_le_bytes());
        }
        out
    }

    fn a32(s: &str) -> [u8; 32] {
        let v = unhex(s);
        assert_eq!(v.len(), 32, "not a 32-byte value");
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        out
    }

    fn a64(s: &str) -> [u8; 64] {
        let v = unhex(s);
        assert_eq!(v.len(), 64, "not a 64-byte value");
        let mut out = [0u8; 64];
        out.copy_from_slice(&v);
        out
    }

    /// A field element from a hex string, for stating expectations in the same
    /// little-endian form the encodings use.
    fn fe(s: &str) -> Fe {
        Fe::from_bytes_canonical(&a32(s)).expect("the test's own constant is canonical")
    }

    // -----------------------------------------------------------------------
    // The field
    // -----------------------------------------------------------------------

    #[test]
    fn the_field_constants_are_what_they_claim_to_be() {
        // `P` is `2^255 - 19`, rebuilt here from that expression rather than
        // compared against a second copy of the same sixteen digits.
        let two_255 = [0u64, 0, 0, 0x8000_0000_0000_0000];
        assert_eq!(sub256(&two_255, &[19, 0, 0, 0]).0, P);

        // `(p - 5) / 8`, likewise: `p - 5` shifted right three places.
        let p_minus_5 = sub256(&P, &[5, 0, 0, 0]).0;
        let mut shifted = [0u64; 4];
        for i in 0..4 {
            shifted[i] = (p_minus_5[i] >> 3) | p_minus_5.get(i + 1).map_or(0, |h| h << 61);
        }
        assert_eq!(shifted, P_MINUS_5_OVER_8);

        // `L = 2^252 + 27742317777372353535851937790883648493`. The addend is
        // below 2^125, so it occupies the low two limbs and nothing else.
        let addend = 27_742_317_777_372_353_535_851_937_790_883_648_493u128;
        let l = add256(
            &[0, 0, 0, 0x1000_0000_0000_0000],
            &[addend as u64, (addend >> 64) as u64, 0, 0],
        )
        .0;
        assert_eq!(l, L);
    }

    #[test]
    fn field_arithmetic_agrees_with_exact_integer_arithmetic() {
        // Expectations computed out of tree with Python's arbitrary-precision
        // integers, never with this file. `a` is a value with no structure that
        // could hide a limb mix-up; `b` is `(p-1)/3`, which is dense in every
        // limb.
        let a = fe("78695a4b3c2d1e0f1032547698badcfeefcdab8967452301efbeaddedec0ad1b");
        let b = fe("a4aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2a");

        assert_eq!(
            a.mul(&b),
            fe("2732373c41464b50a544398322176100b0101cd2323ef454b015c6b5b5bf7021")
        );
        assert_eq!(
            a.add(&b),
            fe("1c1405f6e6d7c8b9badcfe20436587a99a78563412f0cdab99695889896b5846")
        );
        assert_eq!(
            a.sub(&b),
            fe("c1beafa0918273646587a9cbed0f3254452301dfbc9a78564414033434160371")
        );
        // The other direction, which is the branch that has to add `p` back.
        assert_eq!(
            b.sub(&a),
            fe("2c41505f6e7d8c9b9a78563412f0cdabbadcfe20436587a9bbebfccbcbe9fc0e")
        );
        assert_eq!(
            a.sq(),
            fe("212c4c32d94f9024f2a76119deda3895de8a52ea42f738d81f0449368aa1e21a")
        );
        // `a^(p-2)` is `a^-1`, which pins `pow` against a value this file has
        // no other way to produce.
        let p_minus_2 = sub256(&P, &[2, 0, 0, 0]).0;
        assert_eq!(
            a.pow(&p_minus_2),
            fe("d26b8c3561ce8a9f3770594af954a0c5eebc8c246547f7ede8c01a1e4079c26c")
        );
        assert_eq!(a.mul(&a.pow(&p_minus_2)), Fe::ONE);
    }

    #[test]
    fn the_reduction_boundary_is_handled() {
        // Everything that lands at or beside `p`, where the fold and the final
        // subtraction meet. `(p-1)^2 = 1 (mod p)` and `(p-1) + (p-1) = p - 2`
        // are the two that a missing second conditional subtraction gets wrong.
        let minus_one = Fe::ZERO.sub(&Fe::ONE);
        assert_eq!(minus_one, Fe(P).sub(&Fe::ONE));
        assert_eq!(minus_one.add(&Fe::ONE), Fe::ZERO);
        assert_eq!(minus_one.mul(&minus_one), Fe::ONE);
        assert_eq!(
            minus_one.add(&minus_one),
            fe("ebffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f")
        );
        // The largest product two reduced elements can make.
        assert_eq!(minus_one.sq(), Fe::ONE);

        // Parity, which is what the encoder's sign bit is. `p - 1 = 2^255 - 20`
        // is **even**, and negation therefore does not simply flip parity —
        // this assertion originally claimed it did, in a comment, and the test
        // was the thing that said otherwise. `-x` and `x` have the same parity
        // exactly when `p - x` and `x` do, which for odd `p` means never for
        // non-zero `x`... except that `p` is odd here, so `p - x` flips parity
        // for every non-zero `x` and `0` is its own negation. Both are checked.
        assert!(!minus_one.is_odd(), "p - 1 = 2^255 - 20 is even");
        assert!(Fe::ONE.is_odd());
        assert!(!Fe::ZERO.is_odd());
        assert!(!Fe::ZERO.neg().is_odd(), "-0 is 0, and 0 is even");
        assert_ne!(Fe::ONE.is_odd(), Fe::ONE.neg().is_odd());
        assert_ne!(Fe([2, 0, 0, 0]).is_odd(), Fe([2, 0, 0, 0]).neg().is_odd());
    }

    #[test]
    fn a_non_canonical_field_encoding_is_refused() {
        // The nineteen values from `p` to `2^255 - 1` re-spell `0..=18`. All of
        // them must be refused, or points get two encodings.
        for s in [
            "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f", // p
            "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f", // p + 1
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f", // p + 18
        ] {
            assert!(
                Fe::from_bytes_canonical(&a32(s)).is_none(),
                "y >= p must be refused: {s}"
            );
        }
        // One below `p` is fine, and is a different element from `p - 1 - p`.
        assert!(Fe::from_bytes_canonical(&a32(
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f"
        ))
        .is_some());
    }

    #[test]
    fn the_curve_parameter_is_the_right_ratio() {
        // `d = -121665 / 121666`, checked the way the definition reads. No
        // reference table is consulted: if `D`'s limbs were mistyped this fails,
        // and it fails without needing to know what the correct limbs are.
        let n121665 = Fe([121_665, 0, 0, 0]);
        let n121666 = Fe([121_666, 0, 0, 0]);
        assert_eq!(D.mul(&n121666).add(&n121665), Fe::ZERO);

        // The two hypotheses the completeness of the addition law rests on, and
        // therefore the two on which `Point::add` needing no special cases
        // rests: `a` is a square and `d` is not. Euler's criterion, `z^((p-1)/2)`,
        // is `1` for a residue and `-1` for a non-residue.
        let mut e = sub256(&P, &[1, 0, 0, 0]).0;
        for i in 0..4 {
            e[i] = (e[i] >> 1) | e.get(i + 1).map_or(0, |h| h << 63);
        }
        let minus_one = Fe::ZERO.sub(&Fe::ONE);
        assert_eq!(D.pow(&e), minus_one, "d must be a non-residue");
        // `a` is `-1` for edwards25519, and it is a square because `p = 1 (mod 4)`.
        assert_eq!(minus_one.pow(&e), Fe::ONE, "a = -1 must be a residue");
        assert_eq!(P[0] & 3, 1, "p = 1 (mod 4), which is why -1 is a square");
    }

    #[test]
    fn the_square_root_of_minus_one_squares_to_minus_one() {
        assert_eq!(SQRT_M1.sq(), Fe::ZERO.sub(&Fe::ONE));
        // ...and it is the specific root RFC 8032 names, `2^((p-1)/4)`, not the
        // other one. Either root works for the decoder, but the constant should
        // be the one its own doc comment claims it is.
        let mut e = sub256(&P, &[1, 0, 0, 0]).0;
        for _ in 0..2 {
            for i in 0..4 {
                e[i] = (e[i] >> 1) | e.get(i + 1).map_or(0, |h| h << 63);
            }
        }
        assert_eq!(Fe([2, 0, 0, 0]).pow(&e), SQRT_M1);
    }

    // -----------------------------------------------------------------------
    // The group
    // -----------------------------------------------------------------------

    /// The canonical encoding of the base point, as every specification prints
    /// it. Short enough to check against the document by eye, which is the
    /// point of deriving `B` from it rather than the other way round.
    const B_ENCODED: &str = "5866666666666666666666666666666666666666666666666666666666666666";

    #[test]
    fn the_base_point_is_the_published_one() {
        let decoded = Point::decompress(&a32(B_ENCODED)).expect("B decodes");
        assert!(decoded.same_point(&B), "the B constant is not B");
        // `decompress` leaves `Z = 1`, so these are affine coordinates and the
        // extended coordinate can be compared directly rather than up to scale.
        assert_eq!(decoded.z, Fe::ONE);
        assert_eq!(decoded.x, B.x);
        assert_eq!(decoded.y, B.y);
        assert_eq!(
            decoded.t, B.t,
            "T must be X*Y or the addition law is not one"
        );
        assert_eq!(B.t, B.x.mul(&B.y));

        // `y = 4/5`, which is where the encoding's endless 0x66 comes from.
        assert_eq!(B.y.mul(&Fe([5, 0, 0, 0])), Fe([4, 0, 0, 0]));
        // ...and B is on the curve: -x^2 + y^2 = 1 + d x^2 y^2.
        let (xx, yy) = (B.x.sq(), B.y.sq());
        assert_eq!(yy.sub(&xx), Fe::ONE.add(&D.mul(&xx).mul(&yy)));
    }

    #[test]
    fn the_base_point_has_order_l() {
        // `[L]B` is the identity and `[L+1]B` is `B` again. This is the single
        // strongest check on the group law, the scalar ladder and `L` all at
        // once: 253 doublings and roughly 130 additions have to be individually
        // correct for the result to land exactly on the neutral element.
        assert!(B.mul(&L).is_identity(), "[L]B must be the identity");
        let l_plus_1 = add256(&L, &[1, 0, 0, 0]).0;
        assert!(B.mul(&l_plus_1).same_point(&B));

        // And B is not itself of small order, or the line above would be
        // trivially true. (Injecting `mul` returning the identity unconditionally
        // passes the first assertion and fails this one.)
        assert!(!B.mul_by_cofactor().is_identity());
    }

    #[test]
    fn the_group_law_behaves_like_one() {
        let two = [2u64, 0, 0, 0];
        let three = [3u64, 0, 0, 0];
        let five = [5u64, 0, 0, 0];

        // The identity is neutral on both sides, which an incomplete addition
        // formula gets wrong.
        assert!(B.add(&Point::IDENTITY).same_point(&B));
        assert!(Point::IDENTITY.add(&B).same_point(&B));
        assert!(Point::IDENTITY.add(&Point::IDENTITY).is_identity());

        // Doubling through the same code path as addition, which is the whole
        // argument for having one formula.
        assert!(B.add(&B).same_point(&B.mul(&two)));
        assert!(B.mul(&two).add(&B).same_point(&B.mul(&three)));
        assert!(B.mul(&two).add(&B.mul(&three)).same_point(&B.mul(&five)));

        // `P + (-P)` is the identity. Negation on a twisted Edwards curve is
        // `(x, y) -> (-x, y)`, so this also checks that `T` negates with `X`.
        let neg_b = Point {
            x: B.x.neg(),
            y: B.y,
            z: B.z,
            t: B.t.neg(),
        };
        assert!(B.add(&neg_b).is_identity());

        // `[0]P` is the identity and `[1]P` is `P`, the two ends of the ladder.
        assert!(B.mul(&[0, 0, 0, 0]).is_identity());
        assert!(B.mul(&[1, 0, 0, 0]).same_point(&B));

        // Associativity on a case where all three operands differ.
        let p2 = B.mul(&two);
        let p3 = B.mul(&three);
        assert!(B.add(&p2).add(&p3).same_point(&B.add(&p2.add(&p3))));

        // `same_point` compares both coordinates, and both halves are needed.
        // `-P` shares `P`'s `y` and differs only in `x`, so a comparison that
        // dropped the `x` half would call them equal — and `-P` is exactly what
        // an attacker gets by flipping bit 255 of a compressed point. `[2]P`
        // shares neither, and is here so that the test does not pass merely
        // because the two sides differ somewhere.
        assert!(!B.same_point(&neg_b), "P and -P are different points");
        assert_eq!(B.y, neg_b.y, "...and they agree in y, which is the trap");
        assert!(!B.same_point(&p2));
        assert!(!B.same_point(&Point::IDENTITY));

        // The same trap in the other direction, and it is the one that survived
        // the first round of injected defects: dropping the `y` half left every
        // other assertion in this module green. The curve equation is even in
        // `y`, so `(x, -y)` is on the curve whenever `(x, y)` is — and it is a
        // *different point*, not `-P`, which is `(-x, y)`. It is built and then
        // checked to be on the curve, so that the assertion below is about
        // `same_point` rather than about a fabricated triple.
        let flip_y = Point {
            x: B.x,
            y: B.y.neg(),
            z: Fe::ONE,
            t: B.x.mul(&B.y.neg()),
        };
        let (fx, fy) = (flip_y.x.sq(), flip_y.y.sq());
        assert_eq!(
            fy.sub(&fx),
            Fe::ONE.add(&D.mul(&fx).mul(&fy)),
            "(x, -y) must be on the curve for this to prove anything"
        );
        assert_eq!(
            B.x, flip_y.x,
            "...and it agrees with B in x, which is the trap"
        );
        assert!(
            !B.same_point(&flip_y),
            "(x, y) and (x, -y) are different points"
        );

        // The scale invariance the projective comparison exists for: the same
        // point with every coordinate multiplied through by 7.
        let scaled = Point {
            x: B.x.mul(&Fe([7, 0, 0, 0])),
            y: B.y.mul(&Fe([7, 0, 0, 0])),
            z: B.z.mul(&Fe([7, 0, 0, 0])),
            t: B.t.mul(&Fe([7, 0, 0, 0])),
        };
        assert!(B.same_point(&scaled), "(X:Y:Z) is a projective triple");
    }

    #[test]
    fn decompression_refuses_what_names_no_point() {
        // `y = 2`: the smallest `y` for which `(y^2-1)/(dy^2+1)` is not a
        // square, so no `x` exists and the encoding is not a point at all.
        assert!(Point::decompress(&a32(
            "0200000000000000000000000000000000000000000000000000000000000000"
        ))
        .is_none());

        // Non-canonical `y`, which `Fe::from_bytes_canonical` refuses. Worth
        // repeating at this level because the sign bit is masked off first and
        // a decoder that masked the wrong bit would let `p + 1` through as `1`.
        for s in [
            "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            // The same three with the sign bit set, so the mask is exercised.
            "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ] {
            assert!(Point::decompress(&a32(s)).is_none(), "non-canonical y: {s}");
        }

        // `x = 0` with the sign bit set: RFC 8032 §5.1.3 step 4. The only two
        // points with `x = 0` are `y = 1` (the identity) and `y = p - 1`.
        assert!(Point::decompress(&a32(
            "0100000000000000000000000000000000000000000000000000000000000080"
        ))
        .is_none());
        assert!(Point::decompress(&a32(
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        ))
        .is_none());
        // The same two with the sign bit clear are perfectly good points.
        assert!(Point::decompress(&a32(
            "0100000000000000000000000000000000000000000000000000000000000000"
        ))
        .expect("the identity encodes")
        .is_identity());
        assert!(Point::decompress(&a32(
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f"
        ))
        .is_some());
    }

    #[test]
    fn the_sign_bit_selects_between_the_two_roots() {
        // Setting bit 255 must give the other root and nothing else: the two
        // decodings of one `y` differ in `x` by negation, and their `x`
        // parities are the two the caller asked for. A decoder that ignored the
        // bit passes neither half.
        let plain = a32(B_ENCODED);
        let mut flipped = plain;
        flipped[31] ^= 0x80;

        let a = Point::decompress(&plain).expect("decodes");
        let b = Point::decompress(&flipped).expect("decodes");
        assert_eq!(a.y, b.y, "the same y");
        assert_eq!(a.x, b.x.neg(), "opposite x");
        assert!(
            !a.x.is_odd(),
            "B's x is the even root: its encoding's bit 255 is 0"
        );
        assert!(b.x.is_odd());
        assert!(a.add(&b).is_identity(), "they are each other's negation");
    }

    #[test]
    fn small_order_points_are_recognised() {
        // The eight points of order dividing 8, derived out of tree by taking
        // `[L]P` for a curve point `P` outside the prime-order subgroup and
        // enumerating its multiples. They match the published table in
        // "Taming the many EdDSAs" (Chalkias, Garillot, Nikolaenko 2020).
        for s in [
            "0100000000000000000000000000000000000000000000000000000000000000", // order 1
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f", // order 2
            "0000000000000000000000000000000000000000000000000000000000000000", // order 4
            "0000000000000000000000000000000000000000000000000000000000000080", // order 4
            "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05", // order 8
            "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85", // order 8
            "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a", // order 8
            "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa", // order 8
        ] {
            let p = Point::decompress(&a32(s)).expect("a real point");
            assert!(p.mul_by_cofactor().is_identity(), "[8]P must vanish: {s}");
        }
        // And the guard against a cofactor test that says yes to everything.
        assert!(!B.mul_by_cofactor().is_identity());

        // `is_identity` is `X == 0 && Y == Z`, and the second half is the only
        // thing separating the identity from `(0, -1)`, the point of order two.
        // The order-2 point is the sole input that can tell the two halves
        // apart — every order-4 and order-8 point has `X != 0` — so it is named
        // explicitly. Dropping `Y == Z` leaves every other assertion in this
        // module green, which is exactly why this one is written down.
        let order2 = Point::decompress(&a32(
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        ))
        .expect("the order-2 point decodes");
        assert!(order2.x.is_zero(), "it is the other point with x = 0");
        assert!(!order2.is_identity(), "(0, -1) is not the identity");
        assert!(order2.add(&order2).is_identity(), "...but twice over it is");
    }

    // -----------------------------------------------------------------------
    // Scalars
    // -----------------------------------------------------------------------

    #[test]
    fn scalars_reduce_mod_l() {
        let mut zero = [0u8; 64];
        assert_eq!(scalar_reduce_512(&zero), [0, 0, 0, 0]);

        // `L` itself reduces to zero, and `L - 1` is left alone. These are the
        // two the conditional subtraction is about.
        let l_bytes = unhex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
        zero[..32].copy_from_slice(&l_bytes);
        assert_eq!(scalar_reduce_512(&zero), [0, 0, 0, 0]);
        let mut minus1 = [0u8; 64];
        minus1[..32].copy_from_slice(&l_bytes);
        minus1[0] -= 1;
        assert_eq!(
            scalar_reduce_512(&minus1),
            scalar_canonical(&a32(
                "ecd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010"
            ))
            .unwrap()
        );

        // The largest possible input, and a real SHA-512 digest. Expectations
        // from exact integer arithmetic out of tree.
        let all_ones = [0xffu8; 64];
        assert_eq!(
            scalar_reduce_512(&all_ones),
            scalar_canonical(&a32(
                "000f9c44e31106a447938568a71b0ed065bef517d273ecce3d9a307c1b419903"
            ))
            .unwrap()
        );
        let digest = crate::sha512::sha512(b"polylinker");
        assert_eq!(
            scalar_reduce_512(&digest),
            scalar_canonical(&a32(
                "b9877876ad1ce9af81f6ec5ae5b241359f2966a32919276a09694703e45efd08"
            ))
            .unwrap()
        );
        // The digest really is the one that expectation was computed from.
        assert_eq!(
            crate::sha256::hex(&digest),
            "83aca52f8f3f9473ddd31bbac18b881e5e9c1abdccd66484fea0e215efe6db8e\
             cbeeece1d3c8131c2b968e32376a56fbd49cc435475c6c109f9419eaead40b95"
        );
    }

    #[test]
    fn a_scalar_at_or_above_l_is_refused() {
        assert!(
            scalar_canonical(&a32(
                "ecd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010"
            ))
            .is_some(),
            "L - 1 is canonical"
        );
        assert!(
            scalar_canonical(&a32(
                "edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010"
            ))
            .is_none(),
            "L itself is not"
        );
        assert!(
            scalar_canonical(&[0xff; 32]).is_none(),
            "and nor is 2^256 - 1"
        );
    }

    // -----------------------------------------------------------------------
    // RFC 8032 §7.1
    // -----------------------------------------------------------------------

    /// The 1023-byte message of TEST 1024, exactly as RFC 8032 §7.1 prints it.
    const MSG_1024: &str = "
        08b8b2b733424243760fe426a4b54908632110a66c2f6591eabd3345e3e4eb98
        fa6e264bf09efe12ee50f8f54e9f77b1e355f6c50544e23fb1433ddf73be84d8
        79de7c0046dc4996d9e773f4bc9efe5738829adb26c81b37c93a1b270b20329d
        658675fc6ea534e0810a4432826bf58c941efb65d57a338bbd2e26640f89ffbc
        1a858efcb8550ee3a5e1998bd177e93a7363c344fe6b199ee5d02e82d522c4fe
        ba15452f80288a821a579116ec6dad2b3b310da903401aa62100ab5d1a36553e
        06203b33890cc9b832f79ef80560ccb9a39ce767967ed628c6ad573cb116dbef
        efd75499da96bd68a8a97b928a8bbc103b6621fcde2beca1231d206be6cd9ec7
        aff6f6c94fcd7204ed3455c68c83f4a41da4af2b74ef5c53f1d8ac70bdcb7ed1
        85ce81bd84359d44254d95629e9855a94a7c1958d1f8ada5d0532ed8a5aa3fb2
        d17ba70eb6248e594e1a2297acbbb39d502f1a8c6eb6f1ce22b3de1a1f40cc24
        554119a831a9aad6079cad88425de6bde1a9187ebb6092cf67bf2b13fd65f270
        88d78b7e883c8759d2c4f5c65adb7553878ad575f9fad878e80a0c9ba63bcbcc
        2732e69485bbc9c90bfbd62481d9089beccf80cfe2df16a2cf65bd92dd597b07
        07e0917af48bbb75fed413d238f5555a7a569d80c3414a8d0859dc65a46128ba
        b27af87a71314f318c782b23ebfe808b82b0ce26401d2e22f04d83d1255dc51a
        ddd3b75a2b1ae0784504df543af8969be3ea7082ff7fc9888c144da2af58429e
        c96031dbcad3dad9af0dcbaaaf268cb8fcffead94f3c7ca495e056a9b47acdb7
        51fb73e666c6c655ade8297297d07ad1ba5e43f1bca32301651339e22904cc8c
        42f58c30c04aafdb038dda0847dd988dcda6f3bfd15c4b4c4525004aa06eeff8
        ca61783aacec57fb3d1f92b0fe2fd1a85f6724517b65e614ad6808d6f6ee34df
        f7310fdc82aebfd904b01e1dc54b2927094b2db68d6f903b68401adebf5a7e08
        d78ff4ef5d63653a65040cf9bfd4aca7984a74d37145986780fc0b16ac451649
        de6188a7dbdf191f64b5fc5e2ab47b57f7f7276cd419c17a3ca8e1b939ae49e4
        88acba6b965610b5480109c8b17b80e1b7b750dfc7598d5d5011fd2dcc5600a3
        2ef5b52a1ecc820e308aa342721aac0943bf6686b64b2579376504ccc493d97e
        6aed3fb0f9cd71a43dd497f01f17c0e2cb3797aa2a2f256656168e6c496afc5f
        b93246f6b1116398a346f1a641f3b041e989f7914f90cc2c7fff357876e506b5
        0d334ba77c225bc307ba537152f3f1610e4eafe595f6d9d90d11faa933a15ef1
        369546868a7f3a45a96768d40fd9d03412c091c6315cf4fde7cb68606937380d
        b2eaaa707b4c4185c32eddcdd306705e4dc1ffc872eeee475a64dfac86aba41c
        0618983f8741c5ef68d3a101e8a3b8cac60c905c15fc910840b94c00a0b9d0";

    /// One test vector: a name, a public key, a message, and a signature.
    ///
    /// A named type because `clippy::type_complexity` objects to the tuple
    /// written out, and because naming it is where the field order can be
    /// stated once instead of being re-derived at each destructuring.
    type Vector = (&'static str, [u8; 32], Vec<u8>, [u8; 64]);

    /// The five vectors of RFC 8032 §7.1.
    ///
    /// The secret keys the RFC prints alongside these are deliberately absent:
    /// this module cannot sign and must never be able to, so a private key has
    /// no use here — not even as a test fixture. They were used once, out of
    /// tree, to confirm that each public key really is the one its secret key
    /// derives and that each signature is reproducible; see the note at the top
    /// of this module.
    fn rfc_vectors() -> Vec<Vector> {
        vec![
            (
                "TEST 1",
                a32("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"),
                unhex(""),
                a64(
                    "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155
                     5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
                ),
            ),
            (
                "TEST 2",
                a32("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c"),
                unhex("72"),
                a64(
                    "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da
                     085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
                ),
            ),
            (
                "TEST 3",
                a32("fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025"),
                unhex("af82"),
                a64(
                    "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac
                     18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
                ),
            ),
            (
                "TEST 1024",
                a32("278117fc144c72340f67d0f2316e8386ceffbf2b2428c9c51fef7c597f1d426e"),
                unhex(MSG_1024),
                a64(
                    "0aab4c900501b3e24d7cdf4663326a3a87df5e4843b2cbdb67cbf6e460fec350
                     aa5371b1508f9f4528ecea23c436d94b5e8fcd4f681e30a6ac00a9704a188a03",
                ),
            ),
            (
                "TEST SHA(abc)",
                a32("ec172b93ad5e563bf4932c70e1245034c35467ef2efd4d64ebf819683467e2bf"),
                unhex(
                    "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a
                     2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
                ),
                a64(
                    "dc2a4459e7369633a52b1bf277839a00201009a3efbf3ecb69bea2186c26b589
                     09351fc9ac90b3ecfdfbc7c66431e0303dca179c138ac17ad9bef1177331a704",
                ),
            ),
        ]
    }

    #[test]
    fn rfc_8032_vectors_verify() {
        for (name, key, msg, sig) in rfc_vectors() {
            assert!(verify(&key, &msg, &sig), "{name} must verify");
        }
        // The message lengths the five vectors cover, stated so that a
        // truncated fixture is visible rather than merely slower.
        let lengths: Vec<usize> = rfc_vectors().iter().map(|v| v.2.len()).collect();
        assert_eq!(lengths, vec![0, 1, 2, 1023, 64]);
        // TEST SHA(abc)'s message is SHA-512("abc"), which this crate can
        // check for itself — the one part of the fixture that does not have to
        // be taken on trust.
        assert_eq!(rfc_vectors()[4].2, crate::sha512::sha512(b"abc").to_vec());
    }

    #[test]
    fn every_rfc_vector_has_a_negative_twin() {
        // A verifier that returns `true` unconditionally passes the test above.
        // These are what it fails. Each valid signature is broken in four
        // independent ways, and none of them may still verify.
        for (name, key, msg, sig) in rfc_vectors() {
            // A bit flipped in R (bytes 0..32).
            for pos in [0usize, 15, 31] {
                let mut bad = sig;
                bad[pos] ^= 1;
                assert!(!verify(&key, &msg, &bad), "{name}: flipped R bit {pos}");
            }
            // A bit flipped in S (bytes 32..64). Bit 0 of byte 32 is the
            // lowest bit of the scalar, so this stays canonical and has to be
            // caught by the equation rather than by the range check.
            for pos in [32usize, 47, 62] {
                let mut bad = sig;
                bad[pos] ^= 1;
                assert!(!verify(&key, &msg, &bad), "{name}: flipped S bit {pos}");
            }
            // A bit flipped in the message, including the empty one, which
            // becomes a one-byte message.
            let mut bad_msg = msg.clone();
            if bad_msg.is_empty() {
                bad_msg.push(0);
            } else {
                bad_msg[0] ^= 1;
            }
            assert!(!verify(&key, &bad_msg, &sig), "{name}: flipped message bit");
            // ...and the last byte too, since a hash that dropped its final
            // partial block would still pass the first-byte test.
            if !msg.is_empty() {
                let mut bad_msg = msg.clone();
                let last = bad_msg.len() - 1;
                bad_msg[last] ^= 1;
                assert!(!verify(&key, &bad_msg, &sig), "{name}: flipped last byte");
            }
            // The signature under somebody else's key.
            for (other, key2, _, _) in rfc_vectors() {
                if other != name {
                    assert!(!verify(&key2, &msg, &sig), "{name}: verified under {other}");
                }
            }
        }
    }

    #[test]
    fn negating_the_scalar_does_not_produce_a_second_valid_signature() {
        // `(R, L - S)` is the obvious "just negate it" attempt, and unlike
        // `S + L` it is perfectly canonical, so nothing turns it away before
        // the group equation is reached. `[L - S]B = -[S]B = -(R + [k]A)`, and
        // on a twisted Edwards curve `-P` is `(-x, y)` — the two sides of the
        // equation therefore agree in `y` and differ only in the sign of `x`.
        // A point comparison that looked at `y` alone would accept all five of
        // these, which is the concrete cost of the `x` half of `same_point`.
        for (name, key, msg, sig) in rfc_vectors() {
            let mut s_bytes = [0u8; 32];
            s_bytes.copy_from_slice(&sig[32..]);
            let s = scalar_canonical(&s_bytes).expect("the vector's S is canonical");
            let negated = scalar_bytes(&sub256(&L, &s).0);
            assert!(
                scalar_canonical(&negated).is_some(),
                "{name}: L - S is in range, so the range check cannot be what refuses it"
            );
            let mut bad = sig;
            bad[32..].copy_from_slice(&negated);
            assert!(
                !verify(&key, &msg, &bad),
                "{name}: (R, L - S) must not verify"
            );
        }
    }

    #[test]
    fn signatures_whose_hash_input_straddles_a_block_boundary_verify() {
        // `R ‖ A ‖ M` is 64 bytes longer than the message, so the RFC's own
        // vectors leave the SHA-512 padding boundaries at 111/112/113 and
        // 239/240 bytes untested — the message lengths that reach them are 47,
        // 48, 49, 175 and 176, and none of the five is any of those. These
        // signatures were produced out of tree by OpenSSL from the seed
        // 0001020304050607080910111213141516171819202122232425262728293031;
        // the messages are `(7i + 3) mod 256`, chosen with no repeats so that a
        // dropped byte cannot land on an identical neighbour.
        let key = a32("4b5f52db17ebdeb555101922e89beac9b43e864086b02e4529951d7f491f0cfa");
        for (n, sig) in [
            (
                47usize,
                "a61cc76ba758591ca16a4b784e4739b1b026f67dba49482526fb60185328bb7f\
                 872f8602b7f9dc9d4dd0eb968bc75df4d6f1b6e7752163f2e41c3304cf6f4604",
            ),
            (
                48,
                "4fc02066ed55a262128d9f58af145581547c85604b368d5915afe973eafdc60e\
                 d535d9b319a4842f7320051bbd6b3562b025e201dc9a804192058b7bcf9a7202",
            ),
            (
                49,
                "6a3adb1247181bddaab19e3ac5068eb76332eef9309df43650b049c79721de24\
                 70cf852ca22560a7ff591687dab7ff8b84a44f2ac1307f4db9b0c119fe595305",
            ),
            (
                63,
                "5c98b99eaf743a0af18eb69d2817efe50412fd2a120284ac6e707bcfc4d24e38\
                 cf9356e352d0820f7cdb128d14099b7759ef7a5eb2c82fe0a06ab5f059fe870a",
            ),
            (
                64,
                "5d011ecd200d5378f82fc5fad3656410fada72c12b26c7886a468f1abf6e377a\
                 b8b5ca252a2c9c80a94c60f025b18faa72c2af7fe5b13acb4f7ee1af6d36b602",
            ),
            (
                175,
                "4ca6fc8a099b13ba199ee13124ae9cdffcc3575429f6e4477efbcf5c9149a91c\
                 416901656cdc4a7fc064448301464378b0553d6e23169caef44f827f0decbc04",
            ),
            (
                176,
                "208f029622440069a9b7119cf2b6137cb40b52ecd54a5272ac9b6a56e943da9e\
                 3446bfac9cbe4fc029a81784648006c04d5c897ca90d1d4662e4246b41e47904",
            ),
        ] {
            let msg: Vec<u8> = (0..n).map(|i| ((i * 7 + 3) % 256) as u8).collect();
            let sig = a64(sig);
            assert!(verify(&key, &msg, &sig), "n={n}");
            // The negative twin, for the same reason as everywhere else.
            let mut bad = sig;
            bad[63] ^= 0x01;
            assert!(
                !verify(&key, &msg, &bad),
                "n={n}: flipped bit still verified"
            );
        }
    }

    // -----------------------------------------------------------------------
    // The refusals, each shown to be load-bearing
    // -----------------------------------------------------------------------

    #[test]
    fn a_signature_with_a_non_canonical_scalar_is_refused() {
        // TEST 2's signature with `S` replaced by `S + L`. `[L]B` is the
        // identity, so `[S + L]B = [S]B` and the group equation holds exactly
        // as well as it does for the original: without the `S < L` check every
        // signature would have a second, different spelling.
        let (_, key, msg, sig) = rfc_vectors().into_iter().nth(1).unwrap();
        let malleable = a64(
            "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da
             f52db7415978abc61b2c2eb6aeebfca0387b2eaeb4302aeeb00d291612bb0c10",
        );
        assert_eq!(&malleable[..32], &sig[..32], "same R, only S changed");
        assert!(verify(&key, &msg, &sig), "the original still verifies");
        assert!(!verify(&key, &msg, &malleable), "S + L must be refused");

        // ...and here is the equation holding for it, computed with this
        // module's own primitives. If `scalar_canonical`'s range test were
        // deleted, the assertion above would fail and this one would not
        // change: that is the difference between a rule and a coincidence.
        let mut s_plus_l = [0u64; 4];
        for (i, limb) in s_plus_l.iter_mut().enumerate() {
            let mut le = [0u8; 8];
            le.copy_from_slice(&malleable[32 + i * 8..32 + i * 8 + 8]);
            *limb = u64::from_le_bytes(le);
        }
        assert!(ge(&s_plus_l, &L), "the point of the test is that S >= L");
        let mut r_bytes = [0u8; 32];
        r_bytes.copy_from_slice(&malleable[..32]);
        let a = Point::decompress(&key).unwrap();
        let r = Point::decompress(&r_bytes).unwrap();
        let k = challenge(&r_bytes, &key, &msg);
        assert!(
            B.mul(&s_plus_l).same_point(&r.add(&a.mul(&k))),
            "the group equation holds for S + L; only the range check refuses it"
        );
    }

    #[test]
    fn a_small_order_public_key_would_verify_anything_so_it_is_refused() {
        // Under the identity as a public key, `[k]A` is the identity for every
        // `k`, so the equation collapses to `[S]B = R`. Pick any `S`, publish
        // `R = [S]B`, and that one 64-byte string is a valid signature on every
        // message ever written. Here it is: `S = 1`, `R = B`.
        let key = a32("0100000000000000000000000000000000000000000000000000000000000000");
        let sig = a64(
            "5866666666666666666666666666666666666666666666666666666666666666
             0100000000000000000000000000000000000000000000000000000000000000",
        );

        for msg in [
            &b""[..],
            b"polylinker 1.0.0",
            b"install this instead",
            &[0u8; 100][..],
        ] {
            // The equation really does hold, for every one of them.
            let mut r_bytes = [0u8; 32];
            r_bytes.copy_from_slice(&sig[..32]);
            let a = Point::decompress(&key).unwrap();
            let r = Point::decompress(&r_bytes).unwrap();
            let s = scalar_canonical(&a32(
                "0100000000000000000000000000000000000000000000000000000000000000",
            ))
            .unwrap();
            let k = challenge(&r_bytes, &key, msg);
            assert!(
                B.mul(&s).same_point(&r.add(&a.mul(&k))),
                "the forgery's group equation must hold, or this test proves nothing"
            );
            // And `verify` refuses it anyway.
            assert!(
                !verify(&key, msg, &sig),
                "a small-order key must be refused"
            );
        }

        // The same for a key of order 8, where the collapse is subtler: `[k]A`
        // depends only on `k mod 8`, so a forger searches a few hundred
        // candidate `R = [n]B` until the challenge happens to be `0 mod 8`.
        // This one was found at `n = 6`.
        let key8 = a32("26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05");
        let msg8 = b"polylinker 1.0.0 update manifest";
        let sig8 = a64(
            "f47e49f9d07ad2c1606b4d94067c41f9777d4ffda709b71da1d88628fce34d85
             0600000000000000000000000000000000000000000000000000000000000000",
        );
        let mut r_bytes = [0u8; 32];
        r_bytes.copy_from_slice(&sig8[..32]);
        let a = Point::decompress(&key8).unwrap();
        let r = Point::decompress(&r_bytes).unwrap();
        let mut s_bytes = [0u8; 32];
        s_bytes.copy_from_slice(&sig8[32..]);
        let s = scalar_canonical(&s_bytes).unwrap();
        let k = challenge(&r_bytes, &key8, msg8);
        assert!(
            B.mul(&s).same_point(&r.add(&a.mul(&k))),
            "the order-8 forgery's group equation must hold"
        );
        assert!(
            !verify(&key8, msg8, &sig8),
            "an order-8 key must be refused"
        );
    }

    #[test]
    fn a_non_canonical_or_off_curve_encoding_is_refused_wherever_it_appears() {
        let (_, key, msg, sig) = rfc_vectors().into_iter().nth(1).unwrap();
        assert!(verify(&key, &msg, &sig), "the control must verify");

        for bad in [
            // y >= p
            "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            // on no curve at all
            "0200000000000000000000000000000000000000000000000000000000000000",
            // x = 0 with the sign bit set
            "0100000000000000000000000000000000000000000000000000000000000080",
        ] {
            // As the public key...
            assert!(!verify(&a32(bad), &msg, &sig), "as A: {bad}");
            // ...and as R, where it also changes the challenge, so this is a
            // weaker claim on its own — which is why the decoder is tested
            // directly in `decompression_refuses_what_names_no_point` too.
            let mut spoiled = sig;
            spoiled[..32].copy_from_slice(&a32(bad));
            assert!(!verify(&key, &msg, &spoiled), "as R: {bad}");
        }
    }

    #[test]
    fn wrong_lengths_are_refused_rather_than_panicking() {
        let (_, key, msg, sig) = rfc_vectors().into_iter().nth(1).unwrap();
        assert!(verify_slices(&key, &msg, &sig), "the control must verify");

        // Every truncation and extension of both inputs. The point is not that
        // any of these could verify — it is that none of them panics, because
        // the caller's alternative, `sig[..64].try_into().unwrap()`, does.
        for n in 0..64 {
            assert!(
                !verify_slices(&key, &msg, &sig[..n]),
                "signature of {n} bytes"
            );
        }
        for n in 0..32 {
            assert!(!verify_slices(&key[..n], &msg, &sig), "key of {n} bytes");
        }
        let mut long_sig = sig.to_vec();
        long_sig.push(0);
        assert!(!verify_slices(&key, &msg, &long_sig));
        let mut long_key = key.to_vec();
        long_key.push(0);
        assert!(!verify_slices(&long_key, &msg, &sig));
        assert!(!verify_slices(&[], &msg, &[]));
    }

    #[test]
    fn the_all_zero_signature_is_refused() {
        // `R = 0x00..00` is the order-4 point, not the identity, and `S = 0` is
        // canonical — so this input reaches the group equation rather than
        // being turned away by a range check. It is the shape a caller gets
        // from a zeroed buffer or a truncated download, and it must not verify
        // under any key.
        let (_, key, msg, _) = rfc_vectors().into_iter().nth(1).unwrap();
        assert!(!verify(&key, &msg, &[0u8; 64]));
        assert!(!verify(&key, b"", &[0u8; 64]));
        // A zeroed key with a real signature, likewise. (The zero key is the
        // order-4 point, which the small-order rule catches first.)
        let (_, _, msg2, sig2) = rfc_vectors().into_iter().nth(1).unwrap();
        assert!(!verify(&[0u8; 32], &msg2, &sig2));
    }

    // -----------------------------------------------------------------------
    // Project Wycheproof
    // -----------------------------------------------------------------------

    /// The Wycheproof Ed25519 suite, flattened to a TSV. See the header of the
    /// file itself for its provenance, its licence, and why it is not the
    /// upstream JSON.
    const WYCHEPROOF: &str = include_str!("../vectors/ed25519_wycheproof.tsv");

    /// One parsed row: tcId, expected result, flags, comment, key, message,
    /// signature. The three byte fields stay `Vec<u8>` rather than fixed-size
    /// arrays because several cases are deliberately the wrong length.
    struct WpCase {
        tc_id: u32,
        valid: bool,
        flags: String,
        comment: String,
        key: Vec<u8>,
        msg: Vec<u8>,
        sig: Vec<u8>,
    }

    fn wycheproof_cases() -> Vec<WpCase> {
        WYCHEPROOF
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .map(|line| {
                let f: Vec<&str> = line.split('\t').collect();
                assert_eq!(f.len(), 7, "malformed fixture row: {line:?}");
                let valid = match f[1] {
                    "valid" => true,
                    "invalid" => false,
                    other => panic!("unknown expected result {other:?}"),
                };
                WpCase {
                    tc_id: f[0].parse().expect("tcId is a number"),
                    valid,
                    flags: f[2].to_string(),
                    comment: f[3].to_string(),
                    key: unhex(f[4]),
                    msg: unhex(f[5]),
                    sig: unhex(f[6]),
                }
            })
            .collect()
    }

    #[test]
    fn the_wycheproof_fixture_is_all_there() {
        // The counts, asserted before anything is run. Without this, a fixture
        // that failed to parse — an editor that ate the tabs, a truncated
        // checkout, a filter that quietly matched nothing — would leave
        // `wycheproof_verification_agrees_with_upstream` iterating over an
        // empty list and passing, which is the exact shape of a check that
        // cannot fail. The numbers are upstream's and are also stated in the
        // fixture's own header, so the two have to be edited together.
        let cases = wycheproof_cases();
        assert_eq!(cases.len(), 150, "case count");
        assert_eq!(cases.iter().filter(|c| c.valid).count(), 88, "valid cases");
        assert_eq!(
            cases.iter().filter(|c| !c.valid).count(),
            62,
            "invalid cases"
        );

        // tcIds are upstream's and are what makes a failure lookup-able, so
        // they must be the unbroken run 1..=150 rather than whatever survived
        // a bad edit.
        let ids: Vec<u32> = cases.iter().map(|c| c.tc_id).collect();
        assert_eq!(ids, (1..=150).collect::<Vec<u32>>(), "tcIds");

        // The suite is only worth committing if it still contains the classes
        // of forgery this module's rules exist to refuse. Each of these flags
        // names one; if upstream ever drops a class, this fails rather than
        // silently testing less than the module doc claims.
        for flag in [
            "SignatureMalleability",
            "InvalidEncoding",
            "InvalidSignature",
            "TruncatedSignature",
            "SignatureWithGarbage",
            "CompressedSignature",
        ] {
            assert!(
                cases.iter().any(|c| c.flags.contains(flag) && !c.valid),
                "no invalid case flagged {flag}"
            );
        }

        // And it must contain wrong-length inputs, since those are what
        // `verify_slices` is for.
        assert!(
            cases.iter().any(|c| c.sig.len() != 64),
            "no wrong-length signature in the fixture"
        );
    }

    #[test]
    fn wycheproof_verification_agrees_with_upstream() {
        // Every case, with no exceptions list. An exceptions list is how a
        // verifier ends up documenting its own bug as expected behaviour, and
        // if one is ever genuinely needed the entry has to be argued here
        // rather than added quietly.
        //
        // This ran against OpenSSL 3.x through the `cryptography` package as
        // well, out of tree, on 2026-08-05: all 150 agreed three ways —
        // upstream's expectation, OpenSSL's answer, and this module's.
        let mut accepted_forgeries = Vec::new();
        let mut rejected_valid = Vec::new();
        for c in wycheproof_cases() {
            let got = verify_slices(&c.key, &c.msg, &c.sig);
            if got != c.valid {
                let line = format!(
                    "tcId {} [{}] {}: expected {}, got {}",
                    c.tc_id,
                    c.flags,
                    c.comment,
                    if c.valid { "valid" } else { "invalid" },
                    got
                );
                if got {
                    accepted_forgeries.push(line);
                } else {
                    rejected_valid.push(line);
                }
            }
        }
        // Accepting something upstream calls invalid is reported first and
        // separately: it is the failure that means a forged update installs,
        // where the other means a real one does not.
        assert!(
            accepted_forgeries.is_empty(),
            "FORGERIES ACCEPTED ({}):\n{}",
            accepted_forgeries.len(),
            accepted_forgeries.join("\n")
        );
        assert!(
            rejected_valid.is_empty(),
            "valid signatures rejected ({}):\n{}",
            rejected_valid.len(),
            rejected_valid.join("\n")
        );
    }
}
