//! Double-stranded DNA with ends that remember their shape.
//!
//! Everything up to here has treated a molecule as a string plus a topology
//! flag. That is enough to draw a map and enough to find restriction sites, and
//! not enough to do cloning: the whole question of whether two fragments will
//! join is a question about their *ends*, which a string does not have.
//!
//! # The model
//!
//! [`Dseq`] follows pydna, deliberately, because pydna is the oracle this is
//! tested against and a model that differs subtly is worse than no model.
//! A duplex is two strands and an offset:
//!
//! ```text
//!   ovhg = 0            ovhg = -4              ovhg = +4
//!   AAAAG               GATCCTTTT              AAAAG
//!   TTTTCCTAG               GAAAA          TTTTCCTAG
//! ```
//!
//! `watson` is the top strand 5'->3'. `crick` is the bottom strand, also
//! 5'->3', so it reads right-to-left against watson. `ovhg` is where crick's
//! 3' end sits relative to watson's 5' start: negative means watson protrudes
//! on the left, positive means crick does.
//!
//! Sticky ends are not decoration. A BamHI fragment and a BglII fragment have
//! different recognition sites and the same `GATC` overhang, which is why they
//! ligate — and a tool that models ends as "blunt or not" cannot tell you that.

pub mod assembly;
pub mod goldengate;
pub mod ligate;

use pl_core::{reverse_complement, Topology};
use pl_enzymes::Enzyme;

/// A double-stranded DNA molecule, with ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dseq {
    /// Top strand, 5'->3'.
    pub watson: String,
    /// Bottom strand, 5'->3' — so it reads antiparallel to `watson`.
    pub crick: String,
    /// Offset of crick's 3' end from watson's 5' start. Negative: watson
    /// protrudes on the left. Positive: crick does. Zero: flush.
    pub ovhg: i64,
    pub circular: bool,
}

/// Reverse-complement a strand, decoding the result lossily.
///
/// Lossily rather than checked because `pl_core::reverse_complement` passes a
/// byte it does not recognise through *reversed* (`iupac.rs`'s `complement`
/// ends `other => other`), and the two bytes of a non-ASCII character reversed
/// are not valid UTF-8. Making this fallible for every caller would be the
/// wrong shape: the crate's answer to a strand that is not DNA is to refuse the
/// operation at the door — [`try_cut`] and [`pcr`] both check `is_ascii()`
/// before they read a strand — not to thread a `Result` through the
/// arithmetic.
pub(crate) fn rc(s: &str) -> String {
    rc_bytes(s.as_bytes())
}

/// As [`rc`], but over a byte range that is not necessarily a whole `str`.
///
/// Every index into a strand in this crate is a byte count that came out of
/// the duplex arithmetic — `ovhg`, `watson.len()`, `crick.len()` — and not one
/// of them knows anything about character boundaries. `&self.crick[..tail]` is
/// a `str` slice, so it panics the moment `tail` lands inside a multi-byte
/// character, and once `rc()` has widened one stray two-byte character into
/// two three-byte replacement characters that is not a corner: `crick` runs
/// exactly four bytes longer than `watson`, the tail term is 4, and byte 4 of
/// `crick` is in the middle of a `U+FFFD` for a stray character at byte `w-2`,
/// `w-4` or `w-5` of the sequence.
///
/// So the bytes are handed to `reverse_complement` and decoded *afterwards*. A
/// split character comes back as a replacement character instead of as a
/// panic. That is garbage in, garbage out — but a `Dseq` built from a file
/// with one stray byte in it must not be able to abort the process, and it
/// could: `bins/pl-gui/src/clone.rs` calls `plan` synchronously in the panel
/// body on the UI thread, there is no worker and no `catch_unwind` anywhere in
/// the GUI, so `pcr` on such a template took the whole editor down with
/// "byte index 4 is not a char boundary" instead of showing the refusal that
/// was written for exactly this case.
pub(crate) fn rc_bytes(b: &[u8]) -> String {
    String::from_utf8_lossy(&reverse_complement(b)).into_owned()
}

/// The bases of `s` between two byte offsets, clamped to the strand and
/// decoded lossily.
///
/// The same argument as [`rc_bytes`], for the end-shape methods that report
/// protruding bases as they are rather than complemented. The clamping is not
/// new — every call site below already carried its own `.min(len)` or
/// `saturating_sub`, because a hand-built `Dseq` whose `|ovhg|` exceeds its
/// strand length must return the bases that exist rather than underflow a
/// `usize` — it is gathered here so the bound and the lossy decode are stated
/// once instead of four times.
fn take_bytes(s: &str, from: usize, to: usize) -> String {
    let b = s.as_bytes();
    let to = to.min(b.len());
    let from = from.min(to);
    String::from_utf8_lossy(&b[from..to]).into_owned()
}

impl Dseq {
    /// A blunt-ended, fully double-stranded molecule.
    pub fn new(seq: &str, circular: bool) -> Self {
        let watson = seq.to_ascii_uppercase();
        let crick = rc(&watson);
        Dseq {
            watson,
            crick,
            ovhg: 0,
            circular,
        }
    }

    pub fn from_parts(watson: &str, crick: &str, ovhg: i64, circular: bool) -> Self {
        Dseq {
            watson: watson.to_ascii_uppercase(),
            crick: crick.to_ascii_uppercase(),
            ovhg,
            circular,
        }
    }

    /// Total length spanned by the duplex, counting single-stranded ends.
    pub fn len(&self) -> usize {
        if self.circular {
            return self.watson.len();
        }
        let w = self.watson.len() as i64;
        let c = self.crick.len() as i64;
        // watson occupies [0, w); crick occupies **[-ovhg, -ovhg + c)**.
        //
        // The sign matters and this line had it backwards. `fragment()`,
        // `left_end()` and `to_string_full()` all place crick at `-ovhg`, and
        // that is the convention pydna uses — `xcheck_clone.py` asserts field
        // equality against pydna and passes. Reading `+ovhg` here over-reported
        // the length of every sticky-ended fragment by `|ovhg|`.
        let left = 0.min(-self.ovhg);
        let right = w.max(c - self.ovhg);
        (right - left) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.watson.is_empty() && self.crick.is_empty()
    }

    /// The single-stranded overhang at the left end.
    ///
    /// Positive length means a 5' overhang, negative a 3' overhang, and the
    /// string is the protruding bases read 5'->3'.
    pub fn left_end(&self) -> End {
        match self.ovhg.cmp(&0) {
            std::cmp::Ordering::Equal => End::Blunt,
            // watson protrudes: a 5' overhang on the top strand. The index is
            // clamped so a malformed hand-built `Dseq` whose `|ovhg|` exceeds the
            // strand length returns the bases that exist rather than panicking on
            // a `usize` underflow — `left_overhang` already bounds the same way,
            // and every internal producer keeps `|ovhg| <= strand`. Being in
            // range is not the same as being on a character boundary, which is
            // the second half of the bound and why this goes through
            // [`take_bytes`] rather than slicing the strand as a `str`.
            std::cmp::Ordering::Less => End::Overhang {
                five_prime: true,
                bases: take_bytes(&self.watson, 0, (-self.ovhg) as usize),
            },
            // crick protrudes on the left, which is crick's 3' side
            std::cmp::Ordering::Greater => End::Overhang {
                five_prime: false,
                bases: take_bytes(
                    &self.crick,
                    self.crick.len().saturating_sub(self.ovhg as usize),
                    self.crick.len(),
                ),
            },
        }
    }

    /// The single-stranded overhang at the right end.
    pub fn right_end(&self) -> End {
        let w = self.watson.len() as i64;
        let c = self.crick.len() as i64;
        // crick ends at `-ovhg + c`, so watson's protrusion is `w - (c - ovhg)`.
        // With the sign wrong this called a blunt fragment an 8-base 3'
        // overhang, and re-closing a molecule you had just cut reported
        // `ligates_with == false` — "complete digest then religation
        // reconstructs the original" is a stated validation criterion
        // (`docs/PLAN.md` §6) and it was failing.
        let d = w + self.ovhg - c;
        match d.cmp(&0) {
            std::cmp::Ordering::Equal => End::Blunt,
            // watson runs past crick on the right: a 3' overhang on top. Indices
            // clamped like `left_end` and taken as bytes for the same two
            // reasons, so a malformed `Dseq` returns the bases that exist rather
            // than panicking; a no-op for any well-formed one.
            std::cmp::Ordering::Greater => End::Overhang {
                five_prime: false,
                bases: take_bytes(&self.watson, (w - d).max(0) as usize, self.watson.len()),
            },
            // crick runs past: a 5' overhang on the bottom strand
            std::cmp::Ordering::Less => End::Overhang {
                five_prime: true,
                bases: take_bytes(&self.crick, 0, (-d) as usize),
            },
        }
    }

    /// The molecule as a single string, taking watson where it exists and
    /// filling from crick where it does not. Loses the end shapes, so it is for
    /// display and checksums rather than for cloning decisions.
    ///
    /// # Why the crick terms are taken as bytes
    ///
    /// Because this method must not panic, and as a `str` slice it did. Both
    /// crick terms are indexed by a byte count that came out of the duplex
    /// arithmetic (`ovhg` for the head, `c - ovhg - w` for the tail), and a
    /// byte count is not a character boundary. For any `Dseq::new(seq, _)`
    /// whose `seq` holds one non-ASCII character — a non-breaking space or a
    /// micro sign pasted from a supplier's PDF, which `pl-fileio` preserves,
    /// since `genbank.rs` filters ORIGIN for whitespace and digits only and
    /// `Document::from_bytes` sanitises nothing — `crick` comes back four
    /// bytes longer than `watson` (see [`CutError::NotDna`] for the mechanism)
    /// and the tail term is 4. Whether byte 4 of `crick` is then a character
    /// boundary depends on where the stray character sits: for byte `w-2`,
    /// `w-4` or `w-5` of the sequence it is not, and `crick[..4]` split a
    /// `U+FFFD`.
    ///
    /// `Dseq::new("ACGTACGTACGTACGT\u{a0}", false)` is the minimal case: watson
    /// 18 bytes, crick 22, tail 4, and the panic reads "end byte index 4 is not
    /// a char boundary; it is inside '\u{FFFD}' (bytes 3..6 of string)". Put
    /// the same character in the middle of the sequence instead and nothing
    /// panics — the tail is then plain ASCII, and four phantom bases are
    /// appended silently — which is exactly why this went unnoticed, and why
    /// the regression test pins the offsets that trip it.
    ///
    /// The guards in [`try_cut`] and [`pcr`] stay, and are still the right
    /// place to *answer* the user: they refuse with a named strand and the
    /// offending character, where this method can only produce replacement
    /// characters. What changes here is that no public method of [`Dseq`] can
    /// bring the process down on a value that came out of a file, whatever the
    /// caller forgot to check first — including [`assembly::assemble`], which
    /// flattens every fragment through this method and has no such guard.
    pub fn to_string_full(&self) -> String {
        let w = self.watson.len() as i64;
        let c = self.crick.len() as i64;
        let mut out = String::with_capacity(self.len());

        // crick protruding past watson on the left is crick's 3' end.
        if self.ovhg > 0 {
            // Clamped like `left_end`: a malformed `Dseq` with `ovhg` past the
            // crick length must not underflow this index.
            let keep = self.crick.len().saturating_sub(self.ovhg as usize);
            out.push_str(&rc_bytes(&self.crick.as_bytes()[keep..]));
        }
        out.push_str(&self.watson);

        // ...and on the right it is crick's 5' end, which had no term at all.
        // Without it a sequential double digest *deleted bases*: cutting a
        // 15 nt molecule with BamHI then EcoRI summed to 11 nt, and the missing
        // GATC was not recoverable from any strand.
        let tail = (c - self.ovhg - w).min(c);
        if tail > 0 {
            out.push_str(&rc_bytes(&self.crick.as_bytes()[..tail as usize]));
        }
        out
    }
}

/// The shape of one end of a duplex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    Blunt,
    Overhang {
        /// True for a 5' overhang, false for a 3' overhang.
        five_prime: bool,
        /// The protruding bases, 5'->3'.
        bases: String,
    },
}

impl End {
    /// Whether two ends can be ligated: same kind, and complementary bases.
    ///
    /// Note what this does *not* require — that the two fragments came from the
    /// same enzyme. BamHI and BglII recognise different sites and leave the same
    /// `GATC`, which is the basis of a great deal of cloning.
    pub fn ligates_with(&self, other: &End) -> bool {
        match (self, other) {
            (End::Blunt, End::Blunt) => true,
            (
                End::Overhang {
                    five_prime: a,
                    bases: x,
                },
                End::Overhang {
                    five_prime: b,
                    bases: y,
                },
            ) => a == b && *x == rc(y),
            _ => false,
        }
    }
}

/// The overhang an enzyme leaves: positive for 5', negative for 3', 0 blunt.
///
/// The opposite sign to [`Enzyme::ovhg`], which follows Biopython's convention;
/// this one is `bottom - top`, which is what the fragment arithmetic below
/// wants. Both are kept rather than one silently reused, because a sign error
/// here turns a 5' overhang into a 3' one and every ligation downstream is
/// wrong in a way that still looks like DNA.
pub fn overhang_of(e: &Enzyme) -> i64 {
    -(e.ovhg as i64)
}

/// Why a molecule could not be cut at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CutError {
    /// A strand holds a byte that is not DNA.
    ///
    /// [`Dseq`] keeps its strands as `String` and `rc()` round-trips them
    /// through `from_utf8_lossy`, so a single non-ASCII byte — a micro sign or
    /// a non-breaking space pasted from a vendor's order sheet, or a
    /// Windows-1252 file lossily decoded upstream — does not survive
    /// reverse-complementation as itself. `pl_core::reverse_complement`
    /// passes an unknown byte through *reversed*, which splits a two-byte
    /// character into an invalid pair, which decodes to two three-byte
    /// replacement characters. `crick` then measures four bytes longer than
    /// `watson`, `to_string_full`'s tail term goes positive, and the molecule
    /// grows four bases that were never in the file.
    NotDna {
        /// Which strand: "watson strand" or "crick strand".
        what: &'static str,
        found: char,
    },
}

impl std::fmt::Display for CutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CutError::NotDna { what, found } => {
                write!(f, "the {what} contains {found:?}, which is not a DNA base")
            }
        }
    }
}

impl std::error::Error for CutError {}

/// Cut a molecule with one enzyme, returning the fragments with their ends.
///
/// A linear molecule with *k* cuts gives *k + 1* fragments; a circular one
/// gives *k*, because the first and last are the same piece — and a single cut
/// therefore linearises rather than fragmenting.
///
/// An empty result means the molecule could not be digested at all — it was
/// empty, or it is not DNA. That is *not* the same as "the enzyme does not cut
/// here", which returns the molecule whole. Use [`try_cut`] to be told which,
/// and why: this signature cannot say, and a caller that reports "0 fragments"
/// without the reason is passing the problem on to the user unlabelled.
pub fn cut(seq: &Dseq, enzyme: &Enzyme) -> Vec<Dseq> {
    try_cut(seq, enzyme).unwrap_or_default()
}

/// Cut with every enzyme in turn: a double digest.
///
/// **The caller's order is discarded.** The enzymes are sorted before anything
/// is cut, so `digest(m, [SmaI, XmaI])` and `digest(m, [XmaI, SmaI])` are one
/// question with one answer. That has to be arranged, and this doc used to
/// assume it instead: it said the tube does not care which enzyme reaches a
/// site first, so neither does this. The tube does not care *when the sites are
/// separate*. When they overlap it cares very much, and so did this function.
///
/// `Dseq::new("AAAAAAAAAACCCGGGAAAAAAAAAA", true)` is a 26 bp circle holding
/// one CCCGGG. SmaI reads it CCC^GGG and XmaI reads it C^CCGGG, so whichever
/// arrives first destroys the other's site — and cutting in the caller's order,
/// `[SmaI, XmaI]` returned a blunt 26-mer while `[XmaI, SmaI]` returned a
/// 26-mer carrying a CCGG overhang at each end. [`ligate::ligate`] then
/// answered "no product" for the first and one circle for the second, because
/// it refuses blunt joins by default. Same molecule, same two enzymes, opposite
/// biology, decided by an argument order no caller thinks is significant.
///
/// Both of those answers are real molecules. The tube holds a mixture of them,
/// and any function returning one `Vec<Dseq>` returns one member of that
/// mixture; the defect was only in *which* member came back. So the order is
/// fixed here, by name — arbitrary, but total, stable across runs and
/// platforms, and out of the caller's hands. It is also the order the one
/// production caller already used (`bins/pl-gui/src/clone.rs` carries the
/// ticked enzymes in a `BTreeSet<String>`), so no answer anyone is being shown
/// today changes.
///
/// # Why not gather every enzyme's cuts against the original duplex instead
///
/// Because that fabricates molecules, and an arbitrary choice between two real
/// answers beats a manufactured third one. It was tried on the circle above,
/// where SmaI nicks the top strand at 13 and XmaI at 11. Handing both nicks to
/// one split gives `{watson: "CC", crick: "", ovhg: -4}` — a fragment with not
/// one base pair, the exact object
/// `no_double_digest_produces_a_fragment_with_no_base_pairs` exists to forbid —
/// and then, with that piece dropped as a non-duplex, a single fragment whose
/// crick strand is 28 nt long: two bases longer than the entire 26 bp circle it
/// was cut from, because its two ends come from cuts that cannot both have
/// happened. Cutting in sequence cannot produce either, for the reason the
/// enzyme itself supplies — the first cut takes the site away.
///
/// An enzyme that does not cut a piece leaves it whole, and one that cuts
/// nothing at all leaves the molecule as it was. The result is therefore never
/// empty for a non-empty input, and a single circular fragment coming back is
/// the caller's signal that none of these enzymes cut.
///
/// Lived in `bins/pl-gui/src/clone.rs` and had to move: [`ligate::subclone`]
/// needs the same thing, and a digest performed one way in the GUI and another
/// way in the engine is two answers to "what are the fragments".
pub fn digest<'a>(seq: &Dseq, enzymes: impl IntoIterator<Item = &'a Enzyme>) -> Vec<Dseq> {
    // Name first, then the rest of the definition. Name alone would leave a
    // stable sort holding the caller's order for two `Enzyme`s that share a
    // name and differ in where they cut, which is the one remaining way the
    // argument order could still pick the answer.
    let mut enzymes: Vec<&Enzyme> = enzymes.into_iter().collect();
    enzymes.sort_by_key(|e| (e.name, e.site, e.fst5, e.ovhg));

    let mut frags = vec![seq.clone()];
    for e in enzymes {
        let mut next = Vec::with_capacity(frags.len() + 1);
        for f in &frags {
            match try_cut(f, e) {
                Ok(parts) if !parts.is_empty() => next.extend(parts),
                _ => next.push(f.clone()),
            }
        }
        frags = next;
    }
    frags
}

/// As [`cut`], but says why a molecule could not be digested.
pub fn try_cut(seq: &Dseq, enzyme: &Enzyme) -> Result<Vec<Dseq>, CutError> {
    // ASCII up front, the guard `pcr` already applies to its template, and for
    // a worse reason: `pcr` only panicked, whereas `cut` also *invented bases*.
    //
    // Linear `GGTCTCA` + 27 A + '\u{B5}' + 10 C digested with BsaI came back as
    // two fragments whose second held fourteen C where the input had ten, exit
    // 0, no diagnostic -- because `Dseq::new` had already built a `crick` four
    // bytes longer than `watson` (see `CutError::NotDna`) and `to_string_full`
    // dutifully appended `rc(&crick[..4])`. Move the same character next to the
    // nick and the run died instead: `full[0..11]` is not a char boundary, so
    // `pl goldengate --enzyme BsaI` exited 101 on a file `pl digest` reads
    // without complaint. On a circular molecule there was no panic and no
    // insertion, and the fragments came back as Latin-1 mojibake, because the
    // wrap branch below re-encodes each raw byte with `as char`.
    //
    // The strands are checked rather than `to_string_full()` because that
    // method is where the phantom bases are appended: by the time it returns,
    // the damage is in the string being inspected.
    for (what, s) in [("watson strand", &seq.watson), ("crick strand", &seq.crick)] {
        if !s.is_ascii() {
            return Err(CutError::NotDna {
                what,
                found: s.chars().find(|c| !c.is_ascii()).unwrap_or('?'),
            });
        }
    }

    let full = seq.to_string_full();
    let n = full.len() as i64;
    if n == 0 {
        return Ok(Vec::new());
    }
    let topology = if seq.circular {
        Topology::Circular
    } else {
        Topology::Linear
    };
    // pl-enzymes reports the base 3' of the top-strand nick, 1-based.
    let tops: Vec<i64> = pl_enzymes::cut_positions(full.as_bytes(), topology, enzyme)
        .into_iter()
        .map(|p| p as i64 - 1)
        .collect();
    if tops.is_empty() {
        return Ok(vec![seq.clone()]);
    }
    let ovhg = overhang_of(enzyme);

    // Each cut nicks the top strand at `t` and the bottom strand `ovhg` further
    // along. A fragment is the stretch of each strand between consecutive
    // nicks — but the two strands have *different* boundary lists, which is
    // precisely what gives the fragments their sticky ends.
    let (top_b, bot_b): (Vec<i64>, Vec<i64>) = if seq.circular {
        // No molecule ends to worry about: every boundary is a nick.
        let mut t = tops.clone();
        t.push(tops[0] + n);
        let b = t.iter().map(|x| x + ovhg).collect();
        (t, b)
    } else {
        // On a linear molecule the outermost boundaries are THE STRANDS' OWN
        // ENDS, which are not the same thing as the molecule's ends and were
        // written as if they were.
        //
        // `full` is the flattened duplex — watson plus whatever of crick hangs
        // off either side — so `[0, n]` on both strands says "this molecule is
        // blunt at both ends". For an input straight off a file that is true.
        // For a fragment that came out of an earlier cut it is FALSE, and that
        // is the ordinary case the moment a second enzyme is added:
        //
        //     EcoRI + BamHI on a plasmid, in the tube together
        //
        // The EcoRI cut leaves AATT overhangs; the BamHI cut of those pieces
        // then reported their outer ends as Blunt. Every double digest in this
        // program has been describing two of its ends wrongly — the GUI's
        // fragment list printed "blunt" for a sticky end, and the religation
        // search, which is sticky-only by default, refused the join that a
        // ligase actually makes. "No sticky ends match" for an EcoRI/BamHI
        // double digest, which is the commonest cloning there is.
        //
        // Watson occupies `[0, w)` and crick `[-ovhg, -ovhg + c)` in `Dseq`
        // coordinates; `full` starts at the leftmost of the two. Placing the
        // outer boundaries there costs nothing when the input is blunt — both
        // reduce to `[0, n]` — and is the whole fix when it is not.
        let origin = std::cmp::min(0, -seq.ovhg);
        let w = seq.watson.len() as i64;
        let c = seq.crick.len() as i64;
        let (w_lo, w_hi) = (-origin, w - origin);
        let (c_lo, c_hi) = (-seq.ovhg - origin, -seq.ovhg + c - origin);
        //
        // A cut also needs *both* nicks to land on the molecule, and that is
        // now enforced upstream: `place()` in `pl-enzymes/src/lib.rs` 702-718
        // reports a linear cut only when `bond_exists(cut0) &&
        // bond_exists(cut0 - ovhg)`, i.e. `1 <= t <= n-1` and
        // `1 <= t + ovhg <= n-1`. That guarantee is what this crate relies on:
        // clamping a bottom nick back on to the molecule while `ovhg` was still
        // computed from the *unclamped* value turned linear
        // `AAAAAAAAAAAAGGTCTCAC` (n = 20, BsaI top nick 19, bottom nick 23)
        // into `{watson:"C", crick:"", ovhg:-4}` -- a one-base strand claiming a
        // four-base overhang, whose `len()` said 4 while `to_string_full()`
        // returned one character, and whose `left_end()` panicked with "end byte
        // index 4 is out of bounds for string of length 1".
        //
        // This crate used to re-filter for it here, with `(0..=n)`. That test
        // could not reject: it is strictly looser than the one upstream (0
        // rejections over every shipped enzyme at every site offset for
        // n in 1..=60), so it was a check that could not fail, and the
        // `usable.is_empty()` return under it was doubly dead because
        // `tops.is_empty()` above already handles a wholly uncut molecule.
        // A `debug_assert` over the *tight* range said the same thing honestly
        // and tripped if pl-enzymes ever loosened -- which it could, because
        // `cut_positions`' public doc does not promise this; only an internal
        // comment in `cut_sites` does.
        //
        // IT IS A FILTER NOW, and the reason is the paragraph above rather than
        // a loss of nerve. pl-enzymes guarantees both nicks land on `full`, and
        // `full` is no longer the same thing as the duplex: a fragment with a
        // four-base overhang has four bases that exist on one strand only, and a
        // recognition site is not a site there — there is nothing for the enzyme
        // to bind. `cut_positions` cannot know that, because it is handed a
        // string. So the range that has to hold is the DUPLEX, both nicks
        // strictly inside it, and pl-enzymes' guarantee is exactly this
        // condition in the blunt case where the two coincide.
        // ONE INTERVAL, TESTED TWICE — not each nick against its own strand.
        //
        // The paragraph above states the duplex rule and the first version of
        // this line did not implement it: it tested `t` against watson's extent
        // and `t + ovhg` against crick's. Those two intervals coincide only when
        // the molecule is blunt, so on a fragment left over from an earlier
        // enzyme a nick could sit inside watson and outside the duplex, and the
        // filter passed it.
        //
        // What that produced: `digest("AAAAAAAAAATCCGGATCCAAAAAAAAAA",
        // [BamHI, BspEI])` returned THREE fragments, the middle one
        // `{watson: "CCG", crick: "GAT", ovhg: -4}` — three bases of watson and
        // three of crick, overlapping in NOT ONE BASE PAIR. `len()` said 7 while
        // `to_string_full()` returned six characters, and `left_end()` reported
        // a 3-base overhang for an enzyme that always leaves 4. A sweep over the
        // shipped enzymes found 128 such fragments on ordinary MCS-shaped cores
        // (`AvrII+BamHI`, `AgeI+SalI`, `AflII+HindIII`). The clone panel listed
        // them as real and passed them to `ligate`.
        //
        // That 128 is a historical number from a sweep that no longer exists,
        // and it is kept as the record of what was seen rather than as a claim
        // anyone can re-run. The in-tree sweep,
        // `no_double_digest_produces_a_fragment_with_no_base_pairs`, reports 11
        // against the eight names it used to carry and 216 against the whole
        // table it carries now, when this filter is reverted to the per-strand
        // test; its doc gives the exact recipe. The three pairs named above are
        // all from those eight names, which is the narrowness that finding
        // fixed.
        //
        // Single digests were unaffected, which is why every test passed: the
        // two intervals are the same until something has already cut.
        let (d_lo, d_hi) = (w_lo.max(c_lo), w_hi.min(c_hi));
        let inside = |t: &i64| *t > d_lo && *t < d_hi && t + ovhg > d_lo && t + ovhg < d_hi;
        let tops: Vec<i64> = tops.iter().copied().filter(|t| inside(t)).collect();
        if tops.is_empty() {
            return Ok(vec![seq.clone()]);
        }
        let mut t = vec![w_lo];
        t.extend(tops.iter().copied());
        t.push(w_hi);
        let mut b = vec![c_lo];
        b.extend(tops.iter().map(|x| x + ovhg));
        b.push(c_hi);
        (t, b)
    };

    let mut out = Vec::with_capacity(top_b.len() - 1);
    for i in 0..top_b.len() - 1 {
        // Two nicks closer together than the overhang release no fragment.
        //
        // `fragment` pairs watson `[t_i, t_i+1)` with crick `[t_i + ovhg,
        // t_i+1 + ovhg)`, and those two intervals share a base pair only when
        // the nicks are more than `|ovhg|` apart. Closer than that and the
        // piece between them is not a duplex at all: it is one short
        // top-strand oligo and a separate, non-complementary bottom-strand
        // oligo. Emitting it as a `Dseq` stamped with the enzyme's full
        // overhang produced an object that lied about itself -- linear
        // `TTTTGGTCTCAAAAGAGACCT20` with BsaI gave `{watson:"CA", crick:"CT",
        // ovhg:-4}`, whose `len()` said 6 while `to_string_full()` returned the
        // four characters "CAAG" (itself a chimera of bases 9-10 spliced to
        // 13-14), and whose `left_end()` and `right_end()` panicked with "end
        // byte index 4 is out of bounds for string of length 2". Through the
        // CLI that surfaced as `pl goldengate --enzyme BsaI` reporting the
        // overhangs `["CA", "AAAG"]` and "the overhangs are not all the same
        // length" -- a fabricated 2-base junction and a diagnosis that is
        // impossible for BsaI, which always leaves four.
        //
        // `<=`, not `<`: at exactly `|ovhg|` the object passes its own length
        // and slicing invariants but is still two abutting oligos with no base
        // pair between them, and it was being reported as a real junction --
        // two BsaI sites facing inward across a 2 nt spacer printed
        // `{"overhangs":["CTCA","AGAG"], "usable":true}` for a cassette that
        // releases no insert.
        //
        // pydna 5.5.16 -- the oracle this module otherwise follows -- refuses
        // the whole digest rather than dropping one piece: `Dseq.cut` raises
        // "Cuts by BsaI BsaI overlap." for every inward spacer from 2 to 10,
        // linear and circular alike, which is exactly the range this guard
        // covers. Keeping the two flanks and dropping only the impossible piece
        // is a deliberate divergence, and the reason is scale: refusing the
        // digest outright would make `pl goldengate --enzyme BsaI` on a
        // chromosome lose every real junction to one adventitious crowded pair,
        // and those are not rare enough to ignore -- 2 such pieces among 2,266
        // BsaI fragments of a 4.6 Mb circle. What matters is common to both
        // tools: neither hands back the phantom duplex. The one spacer in that
        // range where pydna does answer, 6, is where both sites nick the *same*
        // bond; `cut_positions` reports that cut once, so this loop never sees a
        // piece to drop, while pydna lists the cutsite twice and returns an
        // empty `{watson:"", crick:"", ovhg:0}` fragment on a line and two
        // copies of the whole molecule on a circle. That one predates this
        // guard and is unchanged by it.
        //
        // Only nick-to-nick boundaries qualify. On a linear molecule the first
        // and last boundaries are the molecule's own ends on *both* strands, so
        // those fragments are duplexes however close the nick is -- pl-enzymes
        // guarantees `1 <= t <= n-1` and `1 <= t + ovhg <= n-1`, which is
        // exactly one base pair of overlap at the tightest.
        let nick_to_nick = seq.circular || (i > 0 && i + 2 < top_b.len());
        if nick_to_nick && top_b[i + 1] - top_b[i] <= ovhg.abs() {
            continue;
        }
        out.push(fragment(
            &full,
            (top_b[i], top_b[i + 1]),
            (bot_b[i], bot_b[i + 1]),
            n,
            seq.circular,
        ));
    }
    Ok(out)
}

/// One fragment, given the stretch of each strand it spans.
///
/// `ovhg` is where crick's 3' end sits relative to watson's 5' start. Crick is
/// the reverse complement of its region, so its 3' end is at that region's
/// *left* edge — hence `top_start - bot_start`.
fn fragment(full: &str, top: (i64, i64), bot: (i64, i64), n: i64, wrap: bool) -> Dseq {
    let take = |from: i64, to: i64| -> String {
        if !wrap {
            let a = from.clamp(0, n) as usize;
            let b = to.clamp(0, n) as usize;
            return full[a..b.max(a)].to_string();
        }
        let mut s = String::with_capacity((to - from).max(0) as usize);
        for i in from..to {
            s.push(full.as_bytes()[i.rem_euclid(n) as usize] as char);
        }
        s
    };

    Dseq {
        watson: take(top.0, top.1),
        crick: rc(&take(bot.0, bot.1)),
        ovhg: top.0 - bot.0,
        circular: false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PcrError {
    ForwardNotFound,
    ReverseNotFound,
    /// The reverse primer anneals before the forward one, so there is no product.
    Inverted,
    /// A primer or the template contains something that is not DNA.
    ///
    /// Checked before anything reads any of them, which for the template is
    /// newer than it looks: `rc()` decodes through `from_utf8_lossy`, so a
    /// non-ASCII byte -- a non-breaking space pasted from a vendor's order
    /// sheet is the realistic case -- became a multi-byte replacement
    /// character and then panicked on a char boundary, aborting a whole batch
    /// rather than rejecting one input. The primers were guarded first; the
    /// template's guard was written but placed *below* the
    /// `template.to_string_full()` call that panicked, so for the templates
    /// that actually trip it this variant could not be reached. Both strands
    /// of the template are now tested directly, above that call.
    NotDna {
        /// Which input: "forward primer", "reverse primer" or "template". The
        /// template's two strands share one label, because "the crick strand
        /// of your template" is not a thing the user typed and cannot be a
        /// thing they go and fix.
        what: &'static str,
        /// The offending character as it appears in the input -- the
        /// non-breaking space itself, not the `U+FFFD` the flattened duplex
        /// would have shown, which appears nowhere in the user's file.
        found: char,
    },
    /// A primer anneals in more than one place.
    ///
    /// This is an error, not a detail. A reaction whose primer binds three
    /// sites gives a smear or the wrong band, and a tool that answers with one
    /// confident product has told the user their experiment worked when it did
    /// not. `docs/PLAN.md` §7.12.2 puts this in hazard tier 1: silent,
    /// expensive, and hard to notice until the gel.
    ///
    /// This paragraph used to sit above `NotDna` in one unbroken `///` block,
    /// so rustdoc and every IDE hover summarised `NotDna` -- returned only for
    /// non-ASCII input -- as "A primer anneals in more than one place", and
    /// `NotSpecific`, the variant it was written for, rendered with no doc at
    /// all. A caller matching on the wrong variant to decide how to report a
    /// failure was reading a false statement about it.
    NotSpecific {
        /// Which primer: "forward" or "reverse".
        primer: &'static str,
        /// 0-based starts, on either strand, of the primer's 3'-terminal
        /// [`MIN_ANNEAL`] bases — the seed the specificity scan counts, not
        /// the longer footprint the primary site happens to match over.
        sites: Vec<usize>,
    },
}

impl std::fmt::Display for PcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PcrError::ForwardNotFound => write!(f, "the forward primer does not anneal"),
            PcrError::ReverseNotFound => write!(f, "the reverse primer does not anneal"),
            PcrError::Inverted => write!(
                f,
                "the primers face away from each other; there is no product"
            ),
            PcrError::NotDna { what, found } => {
                write!(f, "the {what} contains {found:?}, which is not a DNA base")
            }
            PcrError::NotSpecific { primer, sites } => {
                // Cap the list. A primer against a homopolymer tract can bind
                // at a hundred overlapping offsets, and an error that prints
                // them all is not an error message.
                const SHOW: usize = 6;
                let listed = sites
                    .iter()
                    .take(SHOW)
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                let more = sites.len().saturating_sub(SHOW);
                write!(
                    f,
                    "not specific: the {primer} primer anneals at {} sites ({listed}{})",
                    sites.len(),
                    if more > 0 {
                        format!(", and {more} more")
                    } else {
                        String::new()
                    }
                )
            }
        }
    }
}

impl std::error::Error for PcrError {}

/// The shortest 3' match that counts as annealing.
///
/// A primer binds by its 3' end; the 5' end may be a tail that matches nothing,
/// which is how restriction sites and homology arms get added to a product.
///
/// It is a floor in both directions, and the second one is easy to lose: a
/// site matching only this many bases is an annealing site *by this
/// definition*, so it has to be counted when [`pcr`] decides whether a primer
/// is specific — not merely tolerated when it happens to be the only site.
pub const MIN_ANNEAL: usize = 12;

/// Simulate PCR.
///
/// The product is the forward primer, the template between the two annealing
/// sites, and the reverse complement of the reverse primer — so 5' tails on
/// either primer appear in the product, which is the entire point of using
/// them.
///
/// This models specificity as exact 3' matching. It is not a thermodynamic
/// simulation and will not tell you a reaction fails for having three
/// mismatches near the 3' end; `docs/PLAN.md` §7.4 keeps that separate.
pub fn pcr(forward: &str, reverse: &str, template: &Dseq) -> Result<Dseq, PcrError> {
    // ASCII up front, ALL FOUR INPUTS, before anything reads them. `rc()` goes
    // through `from_utf8_lossy`, so a non-ASCII byte anywhere -- a non-breaking
    // space pasted from a vendor's order sheet is the realistic case -- became a
    // multi-byte replacement character and then panicked on a char boundary,
    // aborting a whole batch run rather than rejecting one input.
    //
    // THE TEMPLATE'S CHECK USED TO SIT THREE LINES DOWNSTREAM OF THE CALL IT
    // WAS WRITTEN TO PROTECT. It read `let tmpl = template.to_string_full()`
    // and then tested `tmpl.is_ascii()`, so for the templates that actually
    // trip the boundary -- a stray character at byte `w-2`, `w-4` or `w-5`,
    // where `to_string_full`'s tail term slices into a replacement character --
    // the process died on the line above the guard. `AUDIT-2026-07` raised this
    // class and the primers were hardened; the template check was added and
    // placed after the thing it was meant to guard, which is the "raised but
    // not actually fixed" category. `to_string_full` no longer panics either
    // (see its doc), but a lossy string is not an answer: this refusal is,
    // and it names the character the user has to go and find.
    //
    // The strands are checked rather than `to_string_full()`, exactly as
    // `try_cut` does and for the reason its comment gives: that method is
    // where the phantom bases are appended, so by the time it returns, the
    // damage is already in the string being inspected -- and the character it
    // would report is a `U+FFFD` that appears nowhere in the user's file,
    // rather than the non-breaking space that does. Both strands, because
    // `Dseq::from_parts` can carry a stray byte on crick alone.
    for (what, s) in [
        ("forward primer", forward),
        ("reverse primer", reverse),
        ("template", template.watson.as_str()),
        ("template", template.crick.as_str()),
    ] {
        if !s.is_ascii() {
            return Err(PcrError::NotDna {
                what,
                found: s.chars().find(|c| !c.is_ascii()).unwrap_or('?'),
            });
        }
    }

    // ASCII in, ASCII out: `complement` maps every ASCII byte to an ASCII byte
    // (`iupac.rs`, `other => other`), so with both strands checked above there
    // is nothing left here for a second `is_ascii()` test to catch. There was
    // one, and after the reordering it would have been a check that cannot
    // fail.
    let tmpl = template.to_string_full().to_ascii_uppercase();
    let n = tmpl.len();
    if n == 0 {
        return Err(PcrError::ForwardNotFound);
    }
    let fwd = forward.to_ascii_uppercase();
    let rev = reverse.to_ascii_uppercase();
    let rev_rc = rc(&rev);

    // The forward primer's 3' end anneals to the bottom strand, so its
    // sequence appears in the top strand; the reverse primer's reverse
    // complement appears there too.
    let (f_start, f_len) =
        anneal(&tmpl, &fwd, template.circular).ok_or(PcrError::ForwardNotFound)?;
    let (r_start, _) =
        anneal_last(&tmpl, &rev_rc, template.circular).ok_or(PcrError::ReverseNotFound)?;

    // Specificity is judged over **both strands**, and at the **floor**.
    //
    // Both strands: searching only the top strand called a primer specific when
    // its second site was an inverted repeat -- absent from the top strand,
    // present on the bottom, and pydna returns two products for exactly that
    // input. Positions are deduplicated, because a self-complementary seed
    // matches itself on both strands and would otherwise count one real site
    // twice.
    //
    // At the floor: how well a primer binds *here* and how many places it binds
    // are two questions, and answering both with one search got the second one
    // wrong. `anneal` returns the longest 3' suffix that matches anywhere, so
    // the scan used to run at that length -- meaning a template carrying a
    // primer's full 20-mer once and its 3'-terminal 14 nt somewhere else was
    // called specific, because the 20-mer occurs once. Feeding the same crate
    // that bare 14-mer against the same template made it name both sites
    // itself, `pl primers` printed "2 sites: this primer is not specific to one
    // place" for the identical input, and pydna -- searching at its `limit`
    // rather than at the longest match -- built two products and raised "PCR
    // not specific!". `MIN_ANNEAL` is this crate's own statement of what counts
    // as annealing, so it is what gets counted; the longest match is still what
    // sets the footprint and the geometry below.
    //
    // Two deviations from pydna, both taken deliberately and both toward
    // refusing rather than answering, because `docs/PLAN.md` §7.12.2 puts a
    // silently wrong PCR product in hazard tier 1: a false "not specific" costs
    // a re-run, a false "specific" costs a cloning experiment. First, pydna
    // accepts a primer whose reverse-complement site lies upstream, where the
    // two extensions diverge and no artifact forms; we refuse it. Second, this
    // scan runs at 12 where pydna's `limit` is 13, so a second site matching
    // exactly 12 is refused here and amplified there.
    let sites_on_both_strands = |seed: &str| -> Vec<usize> {
        let mut all = find_all(&tmpl, seed, template.circular);
        all.extend(find_all(&tmpl, &rc(seed), template.circular));
        all.sort_unstable();
        all.dedup();
        all
    };
    // `anneal` succeeded, so each primer is at least `MIN_ANNEAL` long.
    let f_sites = sites_on_both_strands(&fwd[fwd.len() - MIN_ANNEAL..]);
    let r_sites = sites_on_both_strands(&rev_rc[..MIN_ANNEAL]);

    if f_sites.len() > 1 {
        return Err(PcrError::NotSpecific {
            primer: "forward",
            sites: f_sites,
        });
    }
    if r_sites.len() > 1 {
        return Err(PcrError::NotSpecific {
            primer: "reverse",
            sites: r_sites,
        });
    }

    // Exactly one seed site each by now, and a longer match can only sit where
    // its own 3' seed does, so the site `anneal` found is that one and the
    // geometry has a single answer.
    let f_start = f_start % n;
    let r_start = r_start % n;
    let f_end = (f_start + f_len) % n;

    let travelled = if template.circular {
        // Extension from the forward primer's 3' end runs forward, wrapping,
        // until it reaches the reverse primer's 3' end. A circle always has
        // such a path, which is why an amplicon across the origin is ordinary
        // and used to be rejected as "the primers face away from each other".
        // It is also why overlapping primers on a plasmid give a
        // whole-plasmid product rather than a short one: pydna returns 430 bp
        // for a 400 bp template with footprints overlapping by 10.
        (r_start + n - f_end) % n
    } else {
        // On a line the polymerase cannot come round again, so the reverse
        // primer's 3' end must lie at or after the forward primer's. Where it
        // does not — overlapping SDM primers, say — there is no product at
        // all, which is what pydna reports. This used to return the two
        // primers concatenated, with the overlapping bases duplicated.
        if r_start < f_start + f_len {
            return Err(PcrError::Inverted);
        }
        r_start - (f_start + f_len)
    };

    let products = [(f_start, travelled)];
    let (f_start, travelled) = products[0];
    let from = (f_start + f_len) % n;
    // Read the template forward from the forward primer's 3' end, wrapping if
    // the amplicon crosses the origin. Slicing `&tmpl[f_end..]` directly
    // panicked whenever the footprint ran past the end of a circular template
    // -- one crafted line of stdin was enough to kill `pl bench-adapter`.
    let mut middle = String::with_capacity(travelled);
    for i in 0..travelled {
        middle.push(tmpl.as_bytes()[(from + i) % n] as char);
    }

    let product = format!("{fwd}{middle}{rev_rc}");
    Ok(Dseq::new(&product, false))
}

/// Where the longest annealing suffix of `primer` binds, and how long it is.
///
/// Returns `(first start, matched length)`. This answers "how well does the
/// primer bind *here*", which sets the footprint and the amplicon geometry —
/// and only that. It is deliberately **not** the specificity query: it used to
/// return every start as well and `pcr` counted those, which meant a second
/// site matching a shorter 3' suffix was never even searched for. Sites are
/// counted at [`MIN_ANNEAL`] in `pcr` instead.
fn anneal(tmpl: &str, primer: &str, circular: bool) -> Option<(usize, usize)> {
    // `MIN_ANNEAL` is a floor, not a suggestion. This loop used to start at
    // `MIN_ANNEAL.min(primer.len())`, which exempted from the floor exactly the
    // primers the floor exists for: an 8 nt primer annealed over its whole
    // length, and `pcr("ACGGTTAC", "TGACCTGA", ...)` on a 36 nt template
    // returned a confident, checksummed 36 bp product where pydna -- the oracle
    // this crate is differential-tested against -- raises "No PCR product!
    // ... limit=13". The clamp was not defensive, either: unclamped,
    // `(12..=8).rev()` is simply an empty range in Rust, so `anneal` returns
    // `None` and `pcr` answers `ForwardNotFound`, which is pydna's answer.
    for take in (MIN_ANNEAL..=primer.len()).rev() {
        let foot = &primer[primer.len() - take..];
        let sites = find_all(tmpl, foot, circular);
        if !sites.is_empty() {
            return Some((sites[0], take));
        }
    }
    None
}

/// As [`anneal`], but for a probe matched by its 5' end — the reverse
/// primer's 3' end is the *start* of its reverse complement in the top strand.
fn anneal_last(tmpl: &str, probe: &str, circular: bool) -> Option<(usize, usize)> {
    // Unclamped for the same reason as [`anneal`]: a reverse primer below
    // `MIN_ANNEAL` does not anneal, it is not exempted from the floor.
    for take in (MIN_ANNEAL..=probe.len()).rev() {
        let foot = &probe[..take];
        let sites = find_all(tmpl, foot, circular);
        if !sites.is_empty() {
            return Some((*sites.last().unwrap(), take));
        }
    }
    None
}

/// Every start position of `needle` in `tmpl`, wrapping the origin when the
/// template is circular. Positions are 0-based and within `tmpl`.
fn find_all(tmpl: &str, needle: &str, circular: bool) -> Vec<usize> {
    if needle.is_empty() || needle.len() > tmpl.len() {
        return Vec::new();
    }
    let hay = if circular {
        format!("{tmpl}{tmpl}")
    } else {
        tmpl.to_string()
    };
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = hay[from..].find(needle) {
        let at = from + rel;
        if at >= tmpl.len() {
            break;
        }
        out.push(at);
        from = at + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_enzymes::by_name;

    #[test]
    fn a_blunt_molecule_has_blunt_ends() {
        let d = Dseq::new("GGATCC", false);
        assert_eq!(d.ovhg, 0);
        assert_eq!(d.left_end(), End::Blunt);
        assert_eq!(d.right_end(), End::Blunt);
        assert_eq!(d.len(), 6);
    }

    #[test]
    fn enzyme_overhangs_match_the_chemistry() {
        // EcoRI G^AATTC leaves a 4-base 5' overhang.
        assert_eq!(overhang_of(by_name("EcoRI").unwrap()), 4);
        assert_eq!(overhang_of(by_name("BamHI").unwrap()), 4);
        // EcoRV GAT^ATC is blunt.
        assert_eq!(overhang_of(by_name("EcoRV").unwrap()), 0);
        assert_eq!(overhang_of(by_name("SmaI").unwrap()), 0);
        // PstI CTGCA^G leaves a 4-base 3' overhang.
        assert_eq!(overhang_of(by_name("PstI").unwrap()), -4);
        assert_eq!(overhang_of(by_name("KpnI").unwrap()), -4);
        // NotI GC^GGCCGC leaves 4 bases from an 8-base site.
        assert_eq!(overhang_of(by_name("NotI").unwrap()), 4);
    }

    #[test]
    fn cutting_matches_the_pydna_reference_shape() {
        // The exact fragments pydna produces for this input, read off the
        // reference during the port.
        let d = Dseq::new("AAAAGGATCCTTTT", false);
        let frags = cut(&d, by_name("BamHI").unwrap());
        assert_eq!(frags.len(), 2);
        assert_eq!(frags[0].watson, "AAAAG");
        assert_eq!(frags[0].crick, "GATCCTTTT");
        assert_eq!(frags[0].ovhg, 0);
        assert_eq!(frags[1].watson, "GATCCTTTT");
        assert_eq!(frags[1].crick, "AAAAG");
        assert_eq!(frags[1].ovhg, -4);
    }

    /// A double digest must not flatten the first enzyme's sticky ends.
    ///
    /// PROVEN TO FAIL against 8c41d59, and it is not a corner: EcoRI + BamHI is
    /// the commonest cloning there is. `try_cut` works on `to_string_full()` —
    /// the flattened duplex — and then placed the outer fragment boundaries at
    /// `[0, n]` on BOTH strands, which says "this molecule is blunt at both
    /// ends". True for a molecule off a file, false for a piece that came out of
    /// an earlier cut.
    ///
    /// So `left=Blunt right=GATC` for a fragment whose left end is a live AATT
    /// overhang. What that cost, in the running program:
    ///
    ///   - the religation panel's fragment list printed "blunt" for a sticky
    ///     end, which is a wrong answer to a question a user asks a plasmid
    ///     editor precisely because they cannot see the answer themselves;
    ///   - the ligation search is sticky-only by default, so it refused the join
    ///     a ligase makes and reported "No sticky ends match. Blunt ends are
    ///     excluded" for a perfectly ordinary directional double digest.
    ///
    /// NOTHING COVERED THIS. Every `try_cut` test cut a molecule read from a
    /// string, and every one of those is blunt-ended, so the boundary that was
    /// wrong was the one no test ever exercised. The combined digest shipped
    /// with tests that counted fragments and never asked what their ends were.
    #[test]
    fn a_second_enzyme_does_not_flatten_the_first_ones_sticky_ends() {
        let eco = by_name("EcoRI").expect("in the table");
        let bam = by_name("BamHI").expect("in the table");
        // One EcoRI site and one BamHI site in a circle.
        let m = Dseq::new(
            "GAATTCTTTTTTTTTTTTTTTTTTTTGGATCCAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            true,
        );
        let once = try_cut(&m, eco).expect("EcoRI cuts");
        assert_eq!(once.len(), 1, "one cut in a circle linearises it");
        let aatt = End::Overhang {
            five_prime: true,
            bases: "AATT".into(),
        };
        let gatc = End::Overhang {
            five_prime: true,
            bases: "GATC".into(),
        };
        assert_eq!(
            (once[0].left_end(), once[0].right_end()),
            (aatt.clone(), aatt.clone()),
            "the premise: one EcoRI cut leaves two AATT overhangs"
        );

        let twice = try_cut(&once[0], bam).expect("BamHI cuts the fragment");
        assert_eq!(twice.len(), 2);
        assert_eq!(
            (twice[0].left_end(), twice[0].right_end()),
            (aatt.clone(), gatc.clone()),
            "the EcoRI end of the first piece was flattened to blunt"
        );
        assert_eq!(
            (twice[1].left_end(), twice[1].right_end()),
            (gatc, aatt),
            "the EcoRI end of the second piece was flattened to blunt"
        );

        // ORDER MUST NOT MATTER. Cutting with BamHI first and EcoRI second is
        // the same digest, and a fix that only repaired the second cut's outer
        // ends would give two different answers to one question.
        //
        // THIS PAIR CANNOT SEE THAT BREAK, which is worth saying out loud since
        // the assertion below reads as though it could. The two sites here are
        // 20 bp apart, so neither enzyme's cut lands anywhere near the other's
        // site and the order is irrelevant however `digest` is written — this
        // held even while `digest` was order-dependent. The property is really
        // tested by
        // [`two_enzymes_racing_for_one_site_answer_the_same_in_either_order`],
        // on sites that overlap.
        let other = digest(&m, [bam, eco]);
        let mut a: Vec<(usize, End, End)> = twice
            .iter()
            .map(|f| (f.len(), f.left_end(), f.right_end()))
            .collect();
        let mut b: Vec<(usize, End, End)> = other
            .iter()
            .map(|f| (f.len(), f.left_end(), f.right_end()))
            .collect();
        a.sort_by_key(|x| x.0);
        b.sort_by_key(|x| x.0);
        assert_eq!(a, b, "the two enzymes gave different answers in each order");
    }

    /// A digest is the same digest whichever order its enzymes are named in,
    /// including when they are racing for one site.
    ///
    /// SmaI and XmaI are the sharpest case there is: isoschizomers, both
    /// reading CCCGGG, cutting different bonds inside it. Whichever gets there
    /// first leaves the other nothing to bind, so a fold over the caller's list
    /// answered with whichever molecule the caller happened to name first.
    #[test]
    fn two_enzymes_racing_for_one_site_answer_the_same_in_either_order() {
        let sma = by_name("SmaI").expect("in the table");
        let xma = by_name("XmaI").expect("in the table");
        // 26 bp circle, one CCCGGG at 0-based 10.
        let m = Dseq::new("AAAAAAAAAACCCGGGAAAAAAAAAA", true);

        let forward = digest(&m, [sma, xma]);
        let backward = digest(&m, [xma, sma]);
        assert_eq!(
            shape(&forward),
            shape(&backward),
            "the same two enzymes gave different fragments in each order"
        );

        // And the answer is one enzyme's whole answer rather than a blend of
        // the two: the molecule SmaI alone makes, all 26 bases of it. Which of
        // the two it is, is the arbitrary part; that it is a real single-cut
        // product is the part that matters, and a merged digest does not have
        // it — see `digest`'s own doc for what that produced instead.
        assert_eq!(shape(&forward), shape(&cut(&m, sma)));
        assert_eq!(forward.len(), 1, "one cut in a circle linearises it");
        assert_eq!(forward[0].len(), m.len(), "no bases gained or lost");

        // The consequence the order used to decide, at the end a user sees.
        // `ligate` refuses blunt-to-blunt by default, so the XmaI answer
        // religates and the SmaI answer does not; before the sort, which of
        // those the religation panel reported was chosen by the argument order.
        let opts = ligate::Options::default();
        assert!(!opts.blunt, "the premise: blunt joins are off by default");
        let a = ligate::ligate(&forward, &opts).expect("a religation search");
        let b = ligate::ligate(&backward, &opts).expect("a religation search");
        assert_eq!(
            a.len(),
            b.len(),
            "religation answered differently for the two orders: {} and {}",
            a.len(),
            b.len()
        );
    }

    /// The real pUC19 polylinker, all eleven of its enzymes, read each way.
    ///
    /// Not a constructed pair. This is the sequence people actually digest, and
    /// it carries two overlapping cases at once: in GGTACCCGGG, KpnI and XmaI
    /// nick the *same* bond, and SmaI nicks two bases from XmaI. Reading the
    /// enzyme list in the order the sites occur and reading it backwards by
    /// name used to give 10 pieces and 9, and to disagree about the ends around
    /// that cluster — a 6-mer with a GTAC end and a blunt end in the first, a
    /// CCGG end in the second.
    ///
    /// WHAT THIS ASSERTS IS AGREEMENT, NOT CORRECTNESS, and the distinction is
    /// real here rather than pedantic. In the answer the orders now agree on,
    /// KpnI and SmaI have *both* cut GGTACCCGGG, and they cannot: their sites
    /// overlap by two bases, so the KpnI cut leaves the C at the start of
    /// SmaI's CCCGGG on the crick strand only, and there is no duplex site left
    /// to bind. `try_cut`'s duplex filter does not catch it because it tests the
    /// two NICKS against the fragment's duplex bounds, and SmaI's nick lands
    /// inside those bounds even though its site does not — the limitation the
    /// comment at the filter already names ("a recognition site is not a site
    /// there"), enforced only as far as the nicks. That is a different defect
    /// from this one, it predates it, and fixing it would change which
    /// fragments a real polylinker digest returns; this test is written so that
    /// it keeps passing either way.
    #[test]
    fn a_whole_polylinker_digests_the_same_read_either_way() {
        const MCS: &str = "GAATTCGAGCTCGGTACCCGGGGATCCTCTAGAGTCGACCTGCAGGCATGCAAGCTT";
        let names = [
            "EcoRI", "SacI", "KpnI", "XmaI", "SmaI", "BamHI", "XbaI", "SalI", "PstI", "SphI",
            "HindIII",
        ];
        let es: Vec<&pl_enzymes::Enzyme> = names.iter().filter_map(|n| by_name(n)).collect();
        assert_eq!(
            es.len(),
            names.len(),
            "the shipped table lost an MCS enzyme"
        );

        let m = Dseq::new(&format!("{MCS}{}", "A".repeat(60)), true);

        // Three orders a caller might plausibly hand over: the order the sites
        // occur in, and both alphabetical directions. `BTreeSet` gives the
        // second; a list typed out by hand gives the first.
        let mut by_name_up = es.clone();
        by_name_up.sort_by_key(|e| e.name);
        let mut by_name_down = by_name_up.clone();
        by_name_down.reverse();

        let expected = shape(&digest(&m, es.clone()));
        for (what, order) in [
            ("name ascending", by_name_up),
            ("name descending", by_name_down),
        ] {
            let got = digest(&m, order);
            assert_eq!(
                shape(&got),
                expected,
                "{what} gave {} fragments where the polylinker's own order gave {}",
                got.len(),
                expected.len()
            );
        }
    }

    /// Fragments as (length, left end, right end), in a total order.
    ///
    /// A digest is a set of pieces, not a list: `digest` is free to emit them
    /// rotated, and on a circle it does. Sorting on the length alone is not
    /// enough for a polylinker, where several pieces are the same size and a
    /// stable sort would then compare them in emission order — which is the
    /// very thing under test.
    fn shape(fs: &[Dseq]) -> Vec<String> {
        let mut v: Vec<String> = fs
            .iter()
            .map(|f| format!("{}|{:?}|{:?}", f.len(), f.left_end(), f.right_end()))
            .collect();
        v.sort();
        v
    }

    /// A recognition site lying in a single-stranded overhang is not a site.
    ///
    /// The other half of the same fix, and the half that could have been missed:
    /// once the outer boundaries follow the strands, a nick can be asked for
    /// outside the duplex — and `cut_positions` cannot refuse it, because it is
    /// handed a flattened string and has no way to know which bases are paired.
    /// An enzyme has nothing to bind to on one strand, so the cut does not
    /// happen.
    #[test]
    fn an_enzyme_does_not_cut_where_only_one_strand_is_there() {
        // A duplex whose left four bases are a watson-only overhang, with the
        // BamHI site placed so that it lies entirely inside that overhang.
        let f = Dseq::from_parts("GGATCCAAAACCCCGGGGTTTT", "AAAACCCCGGGGTTTT", -6, false);
        assert_eq!(
            f.left_end(),
            End::Overhang {
                five_prime: true,
                bases: "GGATCC".into()
            },
            "the premise: the site is on one strand only"
        );
        let out = try_cut(&f, by_name("BamHI").expect("in the table")).expect("not an error");
        assert_eq!(
            out.len(),
            1,
            "a site with no complementary strand was treated as a real one"
        );
        assert_eq!(out[0], f, "and the molecule came back unchanged");
    }

    #[test]
    fn a_blunt_cutter_gives_flush_fragments() {
        let d = Dseq::new("AAAAGATATCTTTT", false);
        let frags = cut(&d, by_name("EcoRV").unwrap());
        assert_eq!(frags.len(), 2);
        assert_eq!(frags[0].watson, "AAAAGAT");
        assert_eq!(frags[0].crick, "ATCTTTT");
        assert_eq!(frags[0].ovhg, 0);
        assert_eq!(frags[1].watson, "ATCTTTT");
        assert_eq!(frags[1].ovhg, 0);
    }

    #[test]
    fn one_cut_linearises_a_circle_rather_than_splitting_it() {
        let d = Dseq::new("AAAAGGATCCTTTTGGGG", true);
        let frags = cut(&d, by_name("BamHI").unwrap());
        assert_eq!(frags.len(), 1, "a single cut cannot make two pieces");
        assert!(!frags[0].circular);
        assert_eq!(frags[0].watson, "GATCCTTTTGGGGAAAAG");
        assert_eq!(frags[0].ovhg, -4);
    }

    #[test]
    fn two_cuts_on_a_circle_give_two_fragments() {
        let d = Dseq::new("AAAAGGATCCTTTTGGATCCGGGG", true);
        let frags = cut(&d, by_name("BamHI").unwrap());
        assert_eq!(frags.len(), 2);
        let mut w: Vec<&str> = frags.iter().map(|f| f.watson.as_str()).collect();
        w.sort_unstable();
        assert_eq!(w, vec!["GATCCGGGGAAAAG", "GATCCTTTTG"]);
        assert!(frags.iter().all(|f| f.ovhg == -4));
    }

    #[test]
    fn fragment_lengths_account_for_every_base() {
        let seq = "AAAAGGATCCTTTTGGATCCGGGGCCCC";
        let d = Dseq::new(seq, true);
        let frags = cut(&d, by_name("BamHI").unwrap());
        let total: usize = frags.iter().map(|f| f.watson.len()).sum();
        assert_eq!(
            total,
            seq.len(),
            "circular fragments must tile the molecule"
        );
    }

    #[test]
    fn a_complete_digest_religates_into_the_original() {
        // `docs/PLAN.md` §6 lists this as a validation criterion, and it did
        // not hold: `right_end()` read `ovhg` with the wrong sign, so it called
        // a freshly cut sticky end blunt and every consecutive pair reported
        // `ligates_with == false`. The cut molecule could not be put back
        // together — while every unit test passed, because they all used
        // `ovhg == 0`, the one value at which both sign conventions agree.
        for (seq, enzyme) in [
            ("AAAAGGATCCTTTTGGATCCGGGGCCCC", "BamHI"),
            ("TTTTGAATTCAAAAGAATTCCCCCGGGG", "EcoRI"),
            ("AAAACTGCAGTTTTCTGCAGGGGGCCCC", "PstI"), // 3' overhang
        ] {
            let d = Dseq::new(seq, true);
            let frags = cut(&d, by_name(enzyme).unwrap());
            assert!(frags.len() >= 2, "{enzyme} should cut {seq} twice");

            for (i, f) in frags.iter().enumerate() {
                let next = &frags[(i + 1) % frags.len()];
                assert!(
                    f.right_end().ligates_with(&next.left_end()),
                    "{enzyme} fragment {i} right end {:?} will not re-join {:?}",
                    f.right_end(),
                    next.left_end()
                );
                // A fragment cut by a sticky cutter has no blunt end.
                assert_ne!(f.right_end(), End::Blunt, "{enzyme} left a blunt end");
            }

            // Every base survives the round trip. `to_string_full` had no term
            // for crick's right-hand protrusion, so bases went missing here.
            let rebuilt: usize = frags.iter().map(|f| f.to_string_full().len()).sum();
            let overlap: usize = frags
                .iter()
                .map(|f| match f.left_end() {
                    End::Blunt => 0,
                    End::Overhang { ref bases, .. } => bases.len(),
                })
                .sum();
            assert_eq!(
                rebuilt - overlap,
                seq.len(),
                "{enzyme}: {rebuilt} bases across fragments (minus {overlap} of shared \
                 overhang) should reconstruct {} ",
                seq.len()
            );
        }
    }

    #[test]
    fn a_sticky_fragments_length_counts_each_base_once() {
        // `len()` also read `ovhg` with the wrong sign and over-reported every
        // sticky fragment by |ovhg| — a 9 nt BamHI fragment measured 13.
        let frags = cut(
            &Dseq::new("AAAAGGATCCTTTT", false),
            by_name("BamHI").unwrap(),
        );
        for f in &frags {
            assert_eq!(
                f.len(),
                f.to_string_full().len(),
                "len() and the full sequence disagree for {f:?}"
            );
        }
    }

    #[test]
    fn a_cut_whose_second_nick_falls_off_a_linear_end_is_not_a_cut() {
        // BsaI binds `GGTCTC` at 0-based 12 and nicks the top strand at 19,
        // which is on the molecule; the bottom nick is four bases further along
        // at 23, which is not. `cut_positions` already refuses to report a top
        // nick that reaches past a linear end -- "it binds; there is nothing
        // there to cut" -- and a bottom nick past the end is the same statement.
        // It used to be clamped back to 20, manufacturing
        // `{watson:"C", crick:"", ovhg:-4}`: a one-base strand claiming a
        // four-base overhang, whose `len()` said 4 while `to_string_full()`
        // returned one character, and whose `left_end()` panicked with "end byte
        // index 4 is out of bounds for string of length 1".
        let d = Dseq::new("AAAAAAAAAAAAGGTCTCAC", false);
        let frags = cut(&d, by_name("BsaI").unwrap());
        assert_eq!(frags.len(), 1, "half a cut is not a cut: {frags:?}");
        assert_eq!(frags[0], d, "an uncut molecule comes back unchanged");

        // The corruption was never confined to the last fragment: the sibling
        // came out 19 nt of watson against 20 nt of crick with `ovhg == 0`, a
        // phantom flush end where BsaI leaves four bases. No fragment of any
        // digest may lie about its own ends.
        for f in &frags {
            assert_eq!(
                f.len(),
                f.to_string_full().len(),
                "len() and the full sequence disagree for {f:?}"
            );
            let _ = f.left_end(); // both of these used to panic on the
            let _ = f.right_end(); // fragment this input produced
        }
    }

    #[test]
    fn a_type_iis_cut_with_room_for_both_nicks_still_cuts() {
        // The control for dropping half-cuts: a BsaI site with its full reach
        // on the molecule must still cut, and still leave the four-base 5'
        // overhang, or the fix above has simply disabled Type IIS digestion.
        let d = Dseq::new("AAAAAAAAAAAAGGTCTCACGTGCCCCCCCC", false);
        let frags = cut(&d, by_name("BsaI").unwrap());
        assert_eq!(frags.len(), 2, "{frags:?}");
        assert_eq!(frags[1].ovhg, -4, "BsaI leaves four bases");
        for f in &frags {
            assert_eq!(f.len(), f.to_string_full().len(), "{f:?}");
        }
        assert!(
            frags[0].right_end().ligates_with(&frags[1].left_end()),
            "a real cut re-closes"
        );
    }

    /// 23 characters, 24 bytes: a micro sign pasted into an otherwise ordinary
    /// BsaI substrate. `pl digest` reads the same file without complaint.
    const NOT_DNA: &str = "GGTCTCAAAA\u{B5}AAAAAAAAAAAA";

    #[test]
    fn a_molecule_that_is_not_dna_is_refused_rather_than_cut() {
        let bsai = by_name("BsaI").unwrap();

        // 1. The loud failure. BsaI's top nick lands at byte 7 and its bottom
        //    nick at byte 11, which is the *second* byte of the micro sign, so
        //    `full[0..11]` panicked with "end byte index 11 is not a char
        //    boundary" and `pl goldengate --enzyme BsaI` exited 101 on a file
        //    `pl digest` reports as "BsaI 1 cut at 8".
        assert!(cut(&Dseq::new(NOT_DNA, false), bsai).is_empty());

        // 2. The quiet one, which is worse and which moving the character out
        //    of the nick window exposes. `Dseq::new` builds crick with
        //    `reverse_complement`, which passes an unknown byte through
        //    *reversed*; C2 B5 comes back as B5 C2, which is not UTF-8, so
        //    `from_utf8_lossy` yields two three-byte replacement characters and
        //    crick measures four bytes longer than watson. `to_string_full`'s
        //    tail term `c - ovhg - w` then goes positive and appends
        //    `rc(&crick[..4])`. This 45-character input, whose micro sign sits
        //    23 bases from either nick, came back as two fragments the second
        //    of which held *fourteen* C where the input has ten -- four bases
        //    invented, exit 0, no diagnostic.
        let far = format!("GGTCTCA{}\u{B5}{}", "A".repeat(27), "C".repeat(10));
        let frags = cut(&Dseq::new(&far, false), bsai);
        assert!(frags.is_empty(), "{frags:?}");

        // 3. The circular branch neither panicked nor inserted; it re-encoded
        //    each raw byte with `as char`, so the two-byte micro sign came back
        //    out as four bytes of Latin-1 mojibake.
        assert!(cut(&Dseq::new(NOT_DNA, true), bsai).is_empty());

        // 4. `cut`'s signature cannot say why it gave nothing back, so the
        //    reason has to be reachable somewhere.
        assert_eq!(
            try_cut(&Dseq::new(NOT_DNA, false), bsai),
            Err(CutError::NotDna {
                what: "watson strand",
                found: '\u{B5}'
            })
        );
        assert!(try_cut(&Dseq::new(NOT_DNA, false), bsai)
            .unwrap_err()
            .to_string()
            .contains("not a DNA base"));
    }

    #[test]
    fn an_ambiguity_code_is_dna_and_still_cuts() {
        // The control for the guard above: it rejects bytes that are not ASCII,
        // not bases it dislikes. `N` is a legitimate IUPAC code, it is one byte
        // wide, and a molecule carrying one must still digest -- otherwise the
        // fix has quietly refused every draft assembly.
        let d = Dseq::new("GGTCTCAAAANAAAAAAAAAAAA", false);
        let frags = cut(&d, by_name("BsaI").unwrap());
        assert_eq!(frags.len(), 2, "{frags:?}");
        assert_eq!(frags[1].ovhg, -4);
        for f in &frags {
            assert_eq!(f.len(), f.to_string_full().len(), "{f:?}");
        }
    }

    #[test]
    fn two_nicks_closer_than_the_overhang_release_no_fragment() {
        // `TTTT GGTCTC AAAA GAGACC T20`: a sense BsaI site and an antisense one
        // facing inward across a 4 nt spacer. `cut_positions` correctly reports
        // both cuts, 1-based [10, 12], so the top nicks are two bases apart --
        // but BsaI leaves a four-base overhang, so the piece between them is not
        // a duplex at all. It is a 2 nt top-strand oligo and a separate,
        // non-complementary 2 nt bottom-strand oligo.
        //
        // It used to be emitted anyway, as `{watson:"CA", crick:"CT",
        // ovhg:-4}`: `len()` said 6 while `to_string_full()` returned the four
        // characters "CAAG" -- itself a chimera of bases 9-10 spliced to 13-14,
        // skipping 11-12 -- and `left_end()` and `right_end()` both panicked
        // with "end byte index 4 is out of bounds for string of length 2".
        // `pl goldengate --enzyme BsaI` printed the overhangs `["CA", "AAAG"]`
        // and "the overhangs are not all the same length; they cannot all
        // pair", which is impossible for BsaI and sends the designer to the
        // wrong place.
        let seq = format!("TTTTGGTCTCAAAAGAGACC{}", "T".repeat(20));
        let d = Dseq::new(&seq, false);
        let frags = cut(&d, by_name("BsaI").unwrap());
        assert_eq!(
            frags.len(),
            2,
            "the middle piece is not a fragment: {frags:?}"
        );

        // The two flanking fragments are the true products and are untouched.
        assert_eq!(frags[0].watson, "TTTTGGTCT");
        assert_eq!(frags[0].crick, "TTTGAGACCAAAA");
        assert_eq!(frags[0].ovhg, 0);
        assert_eq!(frags[1].watson, format!("AAAGAGACC{}", "T".repeat(20)));
        assert_eq!(frags[1].crick, format!("{}GGTCT", "A".repeat(20)));
        assert_eq!(frags[1].ovhg, -4);

        // No fragment of any digest may lie about its own ends.
        for f in &frags {
            assert_eq!(
                f.len(),
                f.to_string_full().len(),
                "len() and the full sequence disagree for {f:?}"
            );
            let _ = f.left_end(); // both of these panicked on the fragment
            let _ = f.right_end(); // this input used to produce
        }
    }

    #[test]
    fn a_circle_whose_nicks_crowd_each_other_drops_the_same_phantom_piece() {
        // The circular branch has no boundary filter of any kind -- its comment
        // says "every boundary is a nick", which is true and is exactly why the
        // nick-to-nick spacing matters here too. 30 bp circle, BsaI cuts at
        // 1-based [5, 8]: three apart, four-base overhang, so the piece between
        // them came out as `{watson:"TCA", crick:"TCT", ovhg:-4}` with `len()`
        // 7 against a six-character `to_string_full()`, and both ends panicking.
        let d = Dseq::new(&format!("GGTCTCAAAGAGACC{}", "T".repeat(15)), true);
        let frags = cut(&d, by_name("BsaI").unwrap());
        assert_eq!(frags.len(), 1, "{frags:?}");
        for f in &frags {
            assert_eq!(f.len(), f.to_string_full().len(), "{f:?}");
            let _ = f.left_end();
            let _ = f.right_end();
        }
    }

    #[test]
    fn a_dropout_with_room_for_an_insert_still_gives_three_fragments() {
        // The control for dropping crowded pieces, and the line between a
        // misdesigned cassette and a real one. Two inward BsaI sites need
        // 1 + 4 + insert + 4 + 1 spacer bases between them; at 11 the insert is
        // one base pair and the middle piece is a genuine duplex, so it must
        // survive. Deleting it would disable Golden Gate rather than vet it.
        let seq = format!("TTTTGGTCTC{}GAGACC{}", "A".repeat(11), "T".repeat(20));
        let frags = cut(&Dseq::new(&seq, false), by_name("BsaI").unwrap());
        assert_eq!(frags.len(), 3, "{frags:?}");
        assert_eq!(frags[1].ovhg, -4, "BsaI leaves four bases");
        for f in &frags {
            assert_eq!(f.len(), f.to_string_full().len(), "{f:?}");
            let _ = f.left_end();
            let _ = f.right_end();
        }
        assert!(frags[0].right_end().ligates_with(&frags[1].left_end()));
        assert!(frags[1].right_end().ligates_with(&frags[2].left_end()));
    }

    #[test]
    fn two_sites_nicking_the_same_bond_are_one_cut_not_an_empty_fragment() {
        // The one spacer inside the drop window where pydna still answers, and
        // it answers wrongly -- so this pins the divergence the comment in
        // `cut` claims, and would catch it silently reversing. At a 6 nt inward
        // spacer both BsaI sites nick the *identical* bond on both strands.
        // `cut_positions` reports that cut once, so there is no zero-length
        // piece for the spacing guard to drop and the answer is the same as it
        // was before the guard existed. pydna 5.5.16 lists the cutsite twice
        // and hands back a third fragment of `{watson:"", crick:"", ovhg:0}` on
        // a line, and two identical copies of the whole linearised molecule on
        // a circle -- both artifacts of applying one cut twice.
        let lin = format!("TTTTGGTCTC{}GAGACC{}", "A".repeat(6), "T".repeat(20));
        let f = cut(&Dseq::new(&lin, false), by_name("BsaI").unwrap());
        assert_eq!(f.len(), 2, "one bond, one cut, two pieces: {f:?}");
        assert!(!f.iter().any(|d| d.watson.is_empty()), "{f:?}");
        assert!(f[0].right_end().ligates_with(&f[1].left_end()));

        let circ = format!("GGTCTC{}GAGACC{}", "A".repeat(6), "T".repeat(25));
        let g = cut(&Dseq::new(&circ, true), by_name("BsaI").unwrap());
        assert_eq!(g.len(), 1, "a single cut linearises a circle once: {g:?}");
        for d in f.iter().chain(&g) {
            assert_eq!(d.len(), d.to_string_full().len(), "{d:?}");
        }
    }

    #[test]
    fn pl_enzymes_keeps_both_linear_nicks_on_the_molecule() {
        // This pins the invariant the `debug_assert` in `cut`'s linear branch
        // now states, and that this crate's boundary arithmetic depends on.
        // It replaces a `(0..=n)` filter that could not reject: `place()` in
        // pl-enzymes already requires `1 <= t <= n-1` AND `1 <= t + ovhg <= n-1`
        // for a linear molecule, which is strictly tighter, so the filter was a
        // check that could not fail and the `usable.is_empty()` return under it
        // was dead twice over.
        //
        // The dependency is on an *internal* comment in `cut_sites`, not on
        // `cut_positions`' public doc, so nothing but this test would notice if
        // pl-enzymes loosened again.
        for e in pl_enzymes::ENZYMES {
            let ovhg = overhang_of(e);
            for n in 1..=60usize {
                for background in ["A", "G"] {
                    for offset in 0..n.saturating_sub(e.site.len()) + 1 {
                        let mut seq = background.repeat(n).into_bytes();
                        let site = e.site.as_bytes();
                        if offset + site.len() > n {
                            continue;
                        }
                        seq[offset..offset + site.len()].copy_from_slice(site);
                        let s = String::from_utf8(seq).unwrap();
                        for t in pl_enzymes::cut_positions(s.as_bytes(), Topology::Linear, e)
                            .into_iter()
                            .map(|p| p as i64 - 1)
                        {
                            let n = n as i64;
                            assert!(
                                (1..n).contains(&t) && (1..n).contains(&(t + ovhg)),
                                "{}: cut at {t} with ovhg {ovhg} on a {n} bp linear molecule \
                                 puts a nick off the end",
                                e.name
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_primer_shorter_than_the_annealing_floor_does_not_anneal() {
        // `MIN_ANNEAL` used to be clamped with `.min(primer.len())`, which
        // exempted from the floor exactly the primers the floor exists for.
        // pydna answers this input with "No PCR product! ... limit=13"; we
        // answered with a confident, checksummed 36 bp product built from two
        // 8 nt primers -- below any usable Tm.
        let tmpl = Dseq::new("ACGGTTACGGGGGGGGGGGGGGGGGGGGTCAGGTCA", false);
        assert_eq!(
            pcr("ACGGTTAC", "TGACCTGA", &tmpl),
            Err(PcrError::ForwardNotFound)
        );
    }

    #[test]
    fn a_primer_exactly_at_the_annealing_floor_still_anneals() {
        // The control: 12 is the floor, not the first value above it. Removing
        // the clamp must not cost a primer that sits exactly on it.
        assert_eq!(MIN_ANNEAL, 12);
        let tmpl = dna(0x51de_0003, 400);
        let fwd = &tmpl[100..112];
        let rev = rc(&tmpl[300..312]);
        let p =
            pcr(fwd, &rev, &Dseq::new(&tmpl, false)).expect("12 nt is on the floor, not below it");
        assert_eq!(p.watson, tmpl[100..312].to_ascii_uppercase());
    }

    #[test]
    fn compatible_ends_ligate_and_incompatible_ones_do_not() {
        let bam = cut(
            &Dseq::new("AAAAGGATCCTTTT", false),
            by_name("BamHI").unwrap(),
        );
        // BglII AGATCT leaves the same GATC overhang as BamHI.
        let bgl = cut(
            &Dseq::new("AAAAAGATCTTTTT", false),
            by_name("BglII").unwrap(),
        );
        assert!(
            bam[0].right_end().ligates_with(&bgl[1].left_end()),
            "BamHI and BglII ends are famously compatible"
        );
        // A blunt end does not join a sticky one.
        let blunt = cut(
            &Dseq::new("AAAAGATATCTTTT", false),
            by_name("EcoRV").unwrap(),
        );
        assert!(!bam[0].right_end().ligates_with(&blunt[1].left_end()));
        assert!(blunt[0].right_end().ligates_with(&blunt[1].left_end()));
    }

    #[test]
    fn pcr_amplifies_the_span_between_the_primers() {
        let tmpl = "AAAACCCGGGTTTTACGTACGTAAGCTTCCCCGGGGAAAATTTT";
        let fwd = &tmpl[4..20];
        let rev = rc(&tmpl[24..40]);
        let product = pcr(fwd, &rev, &Dseq::new(tmpl, false)).unwrap();
        assert_eq!(product.watson, tmpl[4..40].to_uppercase());
    }

    #[test]
    fn a_five_prime_tail_ends_up_in_the_product() {
        // The reason anyone uses tails: adding a site the template lacks.
        let tmpl = "AAAACCCGGGTTTTACGTACGTAAGCTTCCCCGGGGAAAATTTT";
        let fwd = format!("GAATTC{}", &tmpl[4..20]);
        let rev = rc(&tmpl[24..40]);
        let product = pcr(&fwd, &rev, &Dseq::new(tmpl, false)).unwrap();
        assert!(product.watson.starts_with("GAATTC"));
        assert_eq!(product.watson.len(), 36 + 6);
        // ...and the new site is really there.
        assert_eq!(
            pl_enzymes::cut_positions(
                product.watson.as_bytes(),
                Topology::Linear,
                by_name("EcoRI").unwrap()
            ),
            vec![2]
        );
    }

    #[test]
    fn primers_that_do_not_anneal_are_refused() {
        let tmpl = Dseq::new("AAAACCCGGGTTTTACGTACGTAAGCTT", false);
        assert_eq!(
            pcr("TTTTTTTTTTTTTTTT", "AAGCTTAAGCTTAAGC", &tmpl),
            Err(PcrError::ForwardNotFound)
        );
    }

    #[test]
    fn a_primer_that_binds_twice_is_refused_rather_than_guessed_at() {
        // Found by differential testing: pydna declines this reaction, and the
        // first version of this code cheerfully returned a product. A
        // non-specific PCR gives a smear or the wrong band, so a confident
        // answer here is worse than no answer.
        // The motif must not be a tandem repeat. Sites are counted over the
        // primer's 3'-terminal `MIN_ANNEAL` bases, so `(ACGT)x5` -- the motif
        // this fixture used to carry -- puts the 12 nt seed at three
        // overlapping offsets inside each copy and the count is six, which is
        // correct but measures the repeat rather than the second site.
        let motif = "ACGTTGCAAGGTCCATGGAC";
        let tmpl = format!(
            "{}{motif}{}{motif}{}",
            "A".repeat(40),
            "C".repeat(60),
            "T".repeat(40)
        );
        let rev = rc(&tmpl[tmpl.len() - 20..]);
        match pcr(motif, &rev, &Dseq::new(&tmpl, false)) {
            Err(PcrError::NotSpecific { primer, sites }) => {
                assert_eq!(primer, "forward");
                assert_eq!(sites.len(), 2, "the motif appears twice: {sites:?}");
            }
            other => panic!("expected NotSpecific, got {other:?}"),
        }
    }

    /// Deterministic pseudo-random DNA.
    ///
    /// PCR fixtures must not repeat: a hand-written template like
    /// "ACGTTGCA" x 20 makes every primer bind in a dozen places, so the test
    /// measures the specificity check rather than the thing it meant to test.
    fn dna(seed: u64, n: usize) -> String {
        let mut x = seed | 1;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                b"ACGT"[(x % 4) as usize] as char
            })
            .collect()
    }

    #[test]
    fn an_amplicon_across_the_origin_is_ordinary() {
        // The origin of a plasmid is an arbitrary numbering choice, so an
        // amplicon that crosses it is routine. This used to return `Inverted`
        // -- "the primers face away from each other" -- which is both wrong and
        // actively misleading about the primer design.
        let tmpl = dna(0x51de_0001, 300);
        let n = tmpl.len();
        let fwd = &tmpl[n - 20..]; // the last 20 bases
        let rev = rc(&tmpl[..20]); // ...wrapping past base 1
        let p = pcr(fwd, &rev, &Dseq::new(&tmpl, true)).expect("a circle always has a path");
        assert_eq!(p.watson, format!("{fwd}{}", &tmpl[..20]));
        assert_eq!(p.len(), 40);
    }

    #[test]
    fn overlapping_primers_give_no_product_on_a_line_and_a_long_one_on_a_circle() {
        // Settled by pydna, not by us: on a linear template overlapping
        // footprints give nothing, and on a circular one the polymerase runs
        // the long way round. Both used to return the two primers concatenated
        // with the overlapping bases duplicated -- a plausible length and a
        // sequence that does not exist.
        let tmpl = dna(0x51de_0002, 400);
        let fwd = &tmpl[100..120];
        let rev = rc(&tmpl[110..130]); // begins inside the forward footprint

        assert!(matches!(
            pcr(fwd, &rev, &Dseq::new(&tmpl, false)),
            Err(PcrError::Inverted)
        ));

        if let Ok(p) = pcr(fwd, &rev, &Dseq::new(&tmpl, true)) {
            assert!(
                p.watson.len() > tmpl.len(),
                "a circular overlap should run nearly the whole plasmid, got {}",
                p.watson.len()
            );
        }
    }

    #[test]
    fn a_crafted_template_does_not_panic() {
        // Slicing `&tmpl[f_end..]` panicked whenever a footprint ran past the
        // end of a circular template, and one line of crafted stdin was enough
        // to kill `pl bench-adapter` -- taking a whole batch with it.
        let cases: [(&str, &str, &str, bool); 6] = [
            (
                "ACGTACGTACGTACGT",
                "ACGTACGTACGTACGT",
                "ACGTACGTACGTACGT",
                true,
            ),
            ("ACGT", "ACGTACGTACGTACGTACGT", "ACGT", true),
            ("", "ACGT", "ACGT", true),
            ("ACGTACGTACGTACGT", "TTTT", "AAAA", true),
            ("A", "A", "A", true),
            (
                "ACGTACGTACGTACGT",
                "ACGTACGTACGTACGT",
                "ACGTACGTACGTACGT",
                false,
            ),
        ];
        for (tmpl, fwd, rev, circular) in cases {
            // Any answer is acceptable here; a panic is not.
            let _ = pcr(fwd, rev, &Dseq::new(tmpl, circular));
        }
    }

    #[test]
    fn a_non_ascii_primer_is_rejected_rather_than_panicking() {
        // A non-breaking space pasted from a vendor's order sheet. `rc()`
        // decodes through `from_utf8_lossy`, so this became a multi-byte
        // replacement character and then panicked on a char boundary --
        // aborting a whole batch instead of rejecting one primer.
        let tmpl = "ACGTACGTACGTACGTACGTACGT";
        for bad in ["ACGT\u{a0}ACGT", "ACGT\u{3b4}ACGT", "\u{fffd}ACGT"] {
            assert!(matches!(
                pcr(bad, "ACGT", &Dseq::new(tmpl, false)),
                Err(PcrError::NotDna { .. })
            ));
            assert!(matches!(
                pcr("ACGT", bad, &Dseq::new(tmpl, false)),
                Err(PcrError::NotDna { .. })
            ));
        }
    }

    /// A 60-base template with one non-breaking space inserted at a chosen byte
    /// offset. The offset is the whole point of the fixture; see the tests
    /// below.
    ///
    /// `dna` is uppercase ACGT, so the insertion index and the byte offset of
    /// the stray character are the same number, and `watson.len()` is
    /// `60 + 2` for every case.
    fn stray(at: usize) -> String {
        let body = dna(0x51de_0913, 60);
        format!("{}\u{a0}{}", &body[..at], &body[at..])
    }

    /// The three offsets at which the old `to_string_full` died, and the reason
    /// they are not "somewhere near the end".
    ///
    /// `Dseq::new` builds crick with `reverse_complement`, which passes an
    /// unknown byte through *reversed*; `C2 A0` comes back as `A0 C2`, which is
    /// not UTF-8, so `from_utf8_lossy` yields two three-byte replacement
    /// characters and `crick` measures `w + 4`. The tail term `c - ovhg - w` is
    /// therefore 4 for every one of these, and the question is only whether
    /// byte 4 of `crick` is a character boundary. With the stray character at
    /// watson byte `p`, the reversal puts the replacement pair at crick byte
    /// `s = w - 2 - p`, occupying `[s, s+3)` and `[s+3, s+6)`. Byte 4 is inside
    /// one of those for `s` in {0, 2, 3} and is a boundary for `s = 1` -- that
    /// is, `p` in {w-2, w-4, w-5} splits and `p = w-3` does not.
    const SPLITS_A_CHARACTER: [usize; 3] = [60, 58, 57]; // w-2, w-4, w-5 for w = 62
    /// The offset one base further in, which does *not* split a character. It is
    /// here so the two tests below can state the trap rather than describe it.
    const LANDS_ON_A_BOUNDARY: usize = 59; // w-3

    /// `to_string_full` does not panic on a strand that is not DNA.
    ///
    /// PROVEN TO FAIL at f0e4a6f: the method ended
    /// `out.push_str(&rc(&self.crick[..tail as usize]))`, a `str` slice indexed
    /// by a byte count from the duplex arithmetic, so it panicked with "end byte
    /// index 4 is not a char boundary; it is inside '\u{FFFD}' (bytes 3..6 of
    /// string)" whenever `tail` landed inside a replacement character.
    ///
    /// Mutation that re-breaks it: in `to_string_full`, put the tail term back
    /// to `out.push_str(&rc(&self.crick[..tail as usize]));` (and, for the head
    /// term, `let head = &self.crick[self.crick.len().saturating_sub(self.ovhg
    /// as usize)..]; out.push_str(&rc(head));`).
    ///
    /// THE OFFSET IS THE TEST. A non-ASCII character in the MIDDLE of the
    /// sequence passes against the *unfixed* code -- the tail is then four
    /// bytes of ordinary ASCII, four phantom bases are appended, and nothing
    /// crashes -- so a fixture like `"ACGT\u{a0}ACGT..."` would be a check that
    /// cannot fail. It has to sit at `w-2`, `w-4` or `w-5`, and
    /// [`LANDS_ON_A_BOUNDARY`] is the neighbour that proves the window is
    /// narrow. This is the trap the next person will fall into; it cost the
    /// original guard its whole effect, because the check that was written for
    /// this case was placed below the call that died.
    #[test]
    fn to_string_full_does_not_panic_on_a_strand_that_is_not_dna() {
        for at in SPLITS_A_CHARACTER {
            let seq = stray(at);
            let d = Dseq::new(&seq, false);
            assert_eq!(d.watson.len(), 62, "the fixture's own arithmetic");
            assert_eq!(d.crick.len(), 66, "two bytes in, six bytes of U+FFFD out");
            // Garbage in, garbage out -- but the bases the user typed are still
            // at the front of it, and the process is still alive.
            let full = d.to_string_full();
            assert!(full.starts_with(&seq), "at {at}: {full:?}");
        }

        // The neighbour that does not split a character: at `w-3` the boundary
        // falls exactly on byte 4. It never panicked, and it is here so that a
        // maintainer who "reproduces" the defect one base over and sees nothing
        // knows why -- and so that the quiet half of the same defect is on the
        // record, since four bases that were never in the file are appended
        // here whether or not anything crashes.
        let seq = stray(LANDS_ON_A_BOUNDARY);
        let quiet = Dseq::new(&seq, false).to_string_full();
        assert!(quiet.starts_with(&seq), "{quiet:?}");
        assert_eq!(
            quiet.chars().count(),
            seq.chars().count() + 4,
            "the tail term appends four characters here: {quiet:?}"
        );

        // Every other offset, including the ones that never split anything.
        for at in 0..=60 {
            let _ = Dseq::new(&stray(at), false).to_string_full();
        }
    }

    /// The end-shape methods do not panic on a strand that is not DNA either.
    ///
    /// PROVEN TO FAIL at f0e4a6f: `left_end` read
    /// `self.watson[..((-self.ovhg) as usize).min(self.watson.len())]` and
    /// `right_end` read `self.crick[..((-d) as usize).min(self.crick.len())]`.
    /// Both indices were clamped -- that much was already deliberate, for a
    /// hand-built `Dseq` whose `|ovhg|` exceeds its strand -- and being in range
    /// is not the same as being on a character boundary. Observed: "end byte
    /// index 1 is not a char boundary; it is inside '\u{a0}' (bytes 0..2 of
    /// string)", from both methods.
    ///
    /// Mutation that re-breaks it: in `take_bytes`, replace the body's last line
    /// with `s[from..to].to_string()`, which is what the four call sites used to
    /// do inline.
    ///
    /// `Dseq`'s fields are public and `fragment()` builds one field by field, so
    /// "watson is clean and crick is not" is a state a caller can hold; it is
    /// not reachable through `Dseq::new`, which builds both from one string.
    #[test]
    fn the_end_shapes_do_not_panic_on_a_strand_that_is_not_dna() {
        // watson protrudes by one byte into a two-byte character.
        assert_eq!(
            Dseq::from_parts("\u{a0}ACGTACGT", "ACGTACGT", -1, false).left_end(),
            End::Overhang {
                five_prime: true,
                bases: "\u{fffd}".into()
            }
        );
        // ...and the mirror: crick runs one byte past watson on the right.
        assert_eq!(
            Dseq::from_parts("ACGTACGT", "\u{a0}ACGTACG", 0, false).right_end(),
            End::Overhang {
                five_prime: true,
                bases: "\u{fffd}".into()
            }
        );
        // The other two arms, which count in from the far end of a strand and
        // so split the character from the other side.
        assert_eq!(
            Dseq::from_parts("ACGTACGT", "ACGTACG\u{a0}", 1, false).left_end(),
            End::Overhang {
                five_prime: false,
                bases: "\u{fffd}".into()
            }
        );
        assert_eq!(
            Dseq::from_parts("ACGTACG\u{a0}", "ACGTACGT", 0, false).right_end(),
            End::Overhang {
                five_prime: false,
                bases: "\u{fffd}".into()
            }
        );
    }

    /// A template that is not DNA is refused by name, rather than crashing the
    /// process on the line above the guard.
    ///
    /// PROVEN TO FAIL at f0e4a6f: `pcr`'s template check ran THREE LINES TOO
    /// LATE. Line 716 was `let tmpl = template.to_string_full()` and the
    /// `NotDna { what: "template" }` guard was at 717-722, so for the offsets
    /// where `to_string_full` split a character the process died before the
    /// check could run: `pcr` aborted with "end byte index 4 is not a char
    /// boundary" at exactly [`SPLITS_A_CHARACTER`]. `rg -n '"template"'`
    /// returned one hit repo-wide -- the construction site -- so nothing
    /// asserted that this variant was ever produced, and the crate's own
    /// `a_non_ascii_primer_is_rejected_rather_than_panicking` uses an ASCII
    /// template in all six of its cases.
    ///
    /// The GUI calls `plan` synchronously in the clone panel's body on the UI
    /// thread, with no worker and no `catch_unwind` anywhere in `bins/pl-gui`,
    /// so a GenBank file carrying one non-breaking space near its end -- which
    /// `pl-fileio` preserves -- took the whole editor down at "choose PCR,
    /// paste two primers".
    ///
    /// Mutation that re-breaks it: delete the line
    /// `("template", template.watson.as_str()),` from `pcr`'s guard array.
    /// Observed at byte offset 0: `found: '\u{FFFD}'` where this asserts
    /// `'\u{a0}'` — only the crick strand is left to catch it, and crick is
    /// where `rc()` has already replaced the character with something the user
    /// never typed, which is the whole argument for checking the strands.
    ///
    /// Mutation that re-breaks the crick half: delete
    /// `("template", template.crick.as_str()),` instead. Observed: the
    /// crick-only case returns `Ok(Dseq { watson: "CGTGAG…", … })` — a
    /// confident 60 bp product from a template that is not DNA.
    ///
    /// Mutation that reproduces the original *panic* rather than a wrong
    /// answer: put `pcr` back to the two-entry primer array with the
    /// `if !tmpl.is_ascii()` block below `let tmpl = …`, and undo
    /// `to_string_full`'s byte terms as described on
    /// `to_string_full_does_not_panic_on_a_strand_that_is_not_dna`. That is the
    /// shipped 0.10.0 shape exactly, and this test then aborts the process at
    /// byte offset 57 with "end byte index 4 is not a char boundary; it is
    /// inside '\u{FFFD}' (bytes 3..6 of string)" instead of failing an
    /// assertion. Offsets 0 to 56 pass against it, because there the first
    /// non-ASCII character of the flattened duplex really is the one from
    /// watson — the late check answers correctly for every input that does not
    /// kill the process on the line above it.
    #[test]
    fn a_non_ascii_template_is_rejected_rather_than_panicking() {
        let body = dna(0x51de_0913, 60);
        let fwd = body[..20].to_string();
        let rev = rc(&body[40..]);

        // The control first, so the refusals below are about the stray byte and
        // not about a primer pair that never worked: the same primers on the
        // same template without the character amplify the whole 60 bases.
        let clean =
            pcr(&fwd, &rev, &Dseq::new(&body, false)).expect("one site each: this pair amplifies");
        assert_eq!(clean.watson.len(), 60);

        // Every insertion offset, refused identically. The character reported is
        // the one in the user's file, not the `U+FFFD` the flattened duplex
        // would have shown -- that is what checking the strands rather than
        // `to_string_full()` buys.
        for at in 0..=60 {
            assert_eq!(
                pcr(&fwd, &rev, &Dseq::new(&stray(at), false)),
                Err(PcrError::NotDna {
                    what: "template",
                    found: '\u{a0}'
                }),
                "at byte offset {at}"
            );
        }
        // ...and the three that used to abort the process are among them.
        for at in SPLITS_A_CHARACTER {
            assert!(matches!(
                pcr(&fwd, &rev, &Dseq::new(&stray(at), false)),
                Err(PcrError::NotDna { .. })
            ));
        }

        // Crick alone. `Dseq`'s fields are public, so a caller can hold a
        // molecule whose bottom strand carries the stray byte and whose top
        // strand does not; flattening it would hide the character in a term
        // `to_string_full` never reaches.
        let crick_only = Dseq::from_parts(&body, &format!("{}\u{a0}ACG", rc(&body)), 0, false);
        assert_eq!(
            pcr(&fwd, &rev, &crick_only),
            Err(PcrError::NotDna {
                what: "template",
                found: '\u{a0}'
            })
        );

        // The message names the input and the character, because "not DNA" on
        // its own sends the user looking through 60 bases that are all fine.
        let msg = pcr(&fwd, &rev, &Dseq::new(&stray(60), false))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("template"), "{msg}");
        assert!(msg.contains("not a DNA base"), "{msg}");
    }

    #[test]
    fn an_inverted_repeat_second_site_is_not_specific() {
        // The primer appears once on the top strand and once on the bottom, so
        // a top-strand-only search called it specific while pydna returns two
        // products. A primer that binds two sites gives a smear or the wrong
        // band; one confident product tells the user their experiment worked
        // when it did not.
        let motif = "ACGTTGCAAGGTCCAT";
        let tmpl = format!("{motif}{}{}{}", "A".repeat(40), rc(motif), "T".repeat(40));
        let rev = rc(&tmpl[tmpl.len() - 16..]);
        match pcr(motif, &rev, &Dseq::new(&tmpl, false)) {
            Err(PcrError::NotSpecific { sites, .. }) => assert!(sites.len() >= 2, "{sites:?}"),
            other => panic!("expected NotSpecific, got {other:?}"),
        }
    }

    #[test]
    fn a_second_site_that_matches_only_the_3_prime_seed_is_still_a_second_site() {
        // `anneal` returns the *longest* 3' suffix that matches anywhere, and
        // the specificity scan used to run at that length. So a template
        // carrying the forward primer's whole 20-mer once and its 3'-terminal
        // 14 nt somewhere else was called specific -- the 20-mer occurs once --
        // and `pcr` handed back a confident, checksummed 320 bp product.
        //
        // Three things already disagreed with that answer. Feeding this crate
        // the bare 14-mer against the same template makes it name both sites
        // itself. `pl primers` prints "2 sites: this primer is not specific to
        // one place" for the identical primer and template. And pydna 5.5.16 --
        // the oracle `bench/README.md` names for `pcr` -- reports forward sites
        // (120, footprint 20) and (270, footprint 14), builds products
        // [320, 170], and raises "PCR not specific! ... limit=13".
        let mut t = dna(0x5eed_1401, 420).into_bytes();
        let fwd = String::from_utf8(t[100..120].to_vec()).unwrap();
        // A second copy of the 3'-terminal 14 nt, behind different 5' context.
        t[256..270].copy_from_slice(&fwd.as_bytes()[6..20]);
        // ...and it must stop at 14: if base 255 happened to extend the match
        // to 15, `anneal` would find the second site by itself and the fixture
        // would prove nothing.
        t[255] = if fwd.as_bytes()[5] == b'A' {
            b'C'
        } else {
            b'A'
        };
        let tmpl = String::from_utf8(t).unwrap();
        let rev = rc(&tmpl[400..420]);

        match pcr(&fwd, &rev, &Dseq::new(&tmpl, false)) {
            Err(PcrError::NotSpecific { primer, sites }) => {
                assert_eq!(primer, "forward");
                // The 12 nt seed starts 8 into the primer, so the two sites are
                // the primary at 100 + 8 and the spliced copy at 256 + 2.
                assert_eq!(sites, vec![108, 258], "{sites:?}");
            }
            other => panic!("expected NotSpecific, got {other:?}"),
        }

        // The control: without the second copy the same primer pair is
        // specific, so the refusal above is about that copy and not about the
        // scan length rejecting everything.
        let clean = dna(0x5eed_1401, 420);
        let rev_clean = rc(&clean[400..420]);
        let product = pcr(&clean[100..120], &rev_clean, &Dseq::new(&clean, false))
            .expect("one site each: this pair amplifies");
        assert_eq!(product.watson.len(), 320);
    }

    #[test]
    fn the_reverse_primer_gets_the_same_scan_as_the_forward_one() {
        // `anneal_last` had the identical defect and the identical fix; a
        // change to one and not the other leaves half the bug. pydna, given
        // this input, reports reverse sites (300, footprint 20) and
        // (120, footprint 14) and builds products [280, 100].
        let mut t = dna(0x5eed_1402, 420).into_bytes();
        let rev_rc = String::from_utf8(t[300..320].to_vec()).unwrap();
        // The reverse primer's 3' end is the *start* of its reverse complement
        // in the top strand, so its 14 nt seed region is `rev_rc[..14]`.
        t[120..134].copy_from_slice(&rev_rc.as_bytes()[..14]);
        t[134] = if rev_rc.as_bytes()[14] == b'A' {
            b'C'
        } else {
            b'A'
        };
        let tmpl = String::from_utf8(t).unwrap();
        let fwd = tmpl[60..80].to_string();
        let rev = rc(&rev_rc);

        match pcr(&fwd, &rev, &Dseq::new(&tmpl, false)) {
            Err(PcrError::NotSpecific { primer, sites }) => {
                assert_eq!(primer, "reverse");
                assert_eq!(sites, vec![120, 300], "{sites:?}");
            }
            other => panic!("expected NotSpecific, got {other:?}"),
        }
    }

    #[test]
    fn the_error_says_where_the_primer_binds() {
        // Not a tandem repeat, for the reason given in
        // `a_primer_that_binds_twice_is_refused_rather_than_guessed_at`: the
        // count is over the 12 nt 3' seed, and `(ACGT)x5` seeds three times per
        // copy.
        let motif = "ACGTTGCAAGGTCCATGGAC";
        let tmpl = format!("{motif}{}{motif}", "C".repeat(50));
        let rev = rc(&tmpl[tmpl.len() - 20..]);
        let e = pcr(motif, &rev, &Dseq::new(&tmpl, false)).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("not specific"), "{msg}");
        assert!(msg.contains("2 sites"), "{msg}");
    }

    #[test]
    fn a_hopeless_primer_does_not_produce_a_hopeless_error_message() {
        // A poly-A primer against a poly-A tract really is non-specific: it
        // binds at about a hundred overlapping offsets. Listing them all would
        // make the message useless, so it is summarised.
        let tmpl = format!("{}{}", "A".repeat(120), "GC".repeat(40));
        let rev = rc(&tmpl[tmpl.len() - 20..]);
        let e = pcr(&"A".repeat(20), &rev, &Dseq::new(&tmpl, false)).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("not specific"), "{msg}");
        assert!(msg.contains("and "), "should summarise the tail: {msg}");
        assert!(msg.len() < 160, "message is {} chars: {msg}", msg.len());
    }

    #[test]
    fn primers_facing_apart_give_no_product() {
        let tmpl = "AAAACCCGGGTTTTACGTACGTAAGCTTCCCCGGGGAAAATTTT";
        // Swap them: the "forward" primer is downstream of the "reverse" one.
        let fwd = &tmpl[24..40];
        let rev = rc(&tmpl[4..20]);
        assert_eq!(
            pcr(fwd, &rev, &Dseq::new(tmpl, false)),
            Err(PcrError::Inverted)
        );
    }

    /// The enzymes the duplex sweep below runs over: the whole shipped table.
    ///
    /// A function rather than a literal in the test body so that
    /// [`the_duplex_sweep_runs_over_the_whole_shipped_table`] and the sweep
    /// itself cannot drift apart -- a coverage claim checked against a
    /// different list from the one that runs is not a coverage claim.
    fn sweep_enzymes() -> Vec<pl_enzymes::Enzyme> {
        pl_enzymes::ENZYMES.to_vec()
    }

    /// How many base pairs a fragment actually has: the overlap between the
    /// two strands.
    ///
    /// Watson occupies `[0, w)` and crick `[-ovhg, -ovhg + c)` -- the
    /// convention `Dseq::len`, `fragment()` and `to_string_full()` all use, and
    /// the one `xcheck_clone.py` asserts field-for-field against pydna. So the
    /// shared interval runs from `max(0, -ovhg)` to `min(w, c - ovhg)`.
    ///
    /// ONE FUNCTION BECAUSE THE EXPRESSION WAS WRITTEN OUT TWICE AND BOTH
    /// COPIES HAD THE SIGN WRONG. They read `w.min(c + ov) - 0.max(ov)`, which
    /// places crick at `[ovhg, ovhg + c)` -- the mirror image, and the one
    /// place in this crate that disagreed with `len()`. It happened to give the
    /// same verdict on the one fragment the sweep was written against
    /// (`{watson: "CCG", crick: "GAT", ovhg: -4}`, which really has no base
    /// pairs, and which both formulas score at -1), so nothing caught it while
    /// the sweep ran over eight enzymes that all leave the same overhang.
    /// Widening the sweep to the whole table produced 1404 false accusations
    /// against unchanged, correct production code -- `{watson: "TCGAGGCATG",
    /// crick: "CC", ovhg: -4}` from XhoI+SphI is a perfectly good fragment
    /// whose two strands pair over two bases, and the old expression scored it
    /// at -2. It could miss a real one, too: at `w = 10, c = 2, ovhg = +9` the
    /// strands do not meet and the old expression returns 1.
    fn paired_bases(f: &Dseq) -> i64 {
        let (w, c, ov) = (f.watson.len() as i64, f.crick.len() as i64, f.ovhg);
        w.min(c - ov) - 0.max(-ov)
    }

    /// The sweep's measuring instrument counts the overlap the duplex has.
    ///
    /// PROVEN TO FAIL at f0e4a6f: the expression was `w.min(c + ov) -
    /// 0.max(ov)`, written out inline at both of its call sites, and it read
    /// the sign of `ovhg` backwards.
    ///
    /// Mutation that re-breaks it: in `paired_bases`, put the body back to
    /// `w.min(c + ov) - 0.max(ov)`. The first case below then reports 1 where
    /// the fragment has 5 base pairs, and
    /// `no_double_digest_produces_a_fragment_with_no_base_pairs` goes red with
    /// 1404 fragments it wrongly calls empty.
    ///
    /// The fixtures are shipped fragments, not hand-invented ones: the first is
    /// the second half of the pydna reference digest asserted in
    /// `cutting_matches_the_pydna_reference_shape`, and the third came out of
    /// the whole-table sweep itself.
    #[test]
    fn the_sweeps_pairing_measure_counts_the_overlap_a_duplex_has() {
        // BamHI's right-hand fragment: nine bases of watson over five of crick,
        // four of which protrude as the GATC overhang, so five pairs.
        assert_eq!(
            paired_bases(&Dseq::from_parts("GATCCTTTT", "AAAAG", -4, false)),
            5
        );
        // Blunt: every base is paired.
        assert_eq!(paired_bases(&Dseq::new("GGATCC", false)), 6);
        // A 3' overhang, the sign the old expression got wrong in the other
        // direction: one base pair, and it is a real fragment.
        assert_eq!(paired_bases(&Dseq::from_parts("C", "GGGCC", 4, false)), 1);
        // ...and the fragment the whole sweep exists to forbid.
        assert_eq!(paired_bases(&Dseq::from_parts("CCG", "GAT", -4, false)), -1);
    }

    /// The sweep really does run over every shipped enzyme.
    ///
    /// PROVEN TO FAIL at f0e4a6f, where the sweep's doc claimed "every
    /// unambiguous shipped enzyme" and its body built the list from eight
    /// hand-written names -- 8 of 58, every one of them `ovhg == -4`. This test
    /// did not exist, and could not have passed if it had.
    ///
    /// Mutation that re-breaks it: in `sweep_enzymes`, replace the body with
    /// `pl_enzymes::ENZYMES[..8].to_vec()`. The count assertion fails at 8
    /// against 58, and so do the blunt and non-4 geometry assertions, because
    /// the first eight entries by name carry only `-4` and `+4`.
    ///
    /// The geometry assertions are the point rather than the count: they are
    /// what the old eight-name list failed, and they are stated as "at least
    /// one of each kind" so that adding an enzyme to the table cannot turn
    /// this red for no reason.
    #[test]
    fn the_duplex_sweep_runs_over_the_whole_shipped_table() {
        let es = sweep_enzymes();
        assert_eq!(
            es.len(),
            pl_enzymes::ENZYMES.len(),
            "the sweep is over the whole table or the sentence above it is false"
        );

        let mut geoms: Vec<i8> = es.iter().map(|e| e.ovhg).collect();
        geoms.sort_unstable();
        geoms.dedup();
        // Biopython's sign convention, as `Enzyme::ovhg` documents: negative is
        // a 5' overhang, positive a 3' one, zero blunt.
        assert!(
            geoms.iter().any(|&o| o < 0),
            "no 5'-overhang cutter in the sweep: {geoms:?}"
        );
        assert!(
            geoms.iter().any(|&o| o > 0),
            "no 3'-overhang cutter in the sweep -- the eight-name list had none \
             of these, and they are the ones `fragment()`'s head term exists \
             for: {geoms:?}"
        );
        assert!(
            geoms.contains(&0),
            "no blunt cutter in the sweep: {geoms:?}"
        );
        assert!(
            geoms.iter().any(|&o| o.abs() != 4),
            "every overhang in the sweep is four bases, so the spacing guard's \
             `|ovhg|` term is only ever tested at one value: {geoms:?}"
        );
        assert!(
            es.iter().any(|e| e.cuts_outside_its_site()),
            "no Type IIS enzyme in the sweep -- the class whose two nicks are \
             both outside the site, which is what `try_cut`'s duplex filter was \
             written for"
        );

        // Why the word "unambiguous" came out of the sweep's doc rather than
        // being honoured: it excluded nothing. If that ever stops being true,
        // the sweep's MCS-shaped cores start carrying ambiguity codes and this
        // is the line that says so.
        assert!(
            es.iter()
                .all(|e| e.site.bytes().all(|b| b"ACGT".contains(&b))),
            "an enzyme with a non-ACGT site is now shipped; the sweep builds its \
             fixtures by pasting sites together, so it now builds ambiguous DNA"
        );
    }

    /// Every fragment a double digest produces is a real duplex.
    ///
    /// PROVEN TO FAIL against the first version of `try_cut`'s duplex filter,
    /// which tested each nick against its OWN strand's extent rather than
    /// against the interval both share. The two coincide until something has
    /// already cut, so single digests were fine and every test passed.
    ///
    /// `AAAAAAAAAATCCGGATCCAAAAAAAAAA` cut by BamHI and BspEI came back as
    /// THREE fragments; the middle one was `{watson: "CCG", crick: "GAT",
    /// ovhg: -4}` -- six bases, three on each strand, sharing NOT ONE BASE
    /// PAIR. `len()` said 7 against six characters from `to_string_full()`.
    /// Those numbers are asserted in
    /// `the_fragment_the_duplex_filter_comment_names_is_six_bases` below,
    /// because this sweep enumerates enzyme pairs and never touches the named
    /// fragment's shape.
    ///
    /// Swept over the WHOLE SHIPPED TABLE rather than asserted on one case,
    /// because the defect is a property of overlapping sites and the pair that
    /// first showed it is not special.
    ///
    /// That sentence used to read "every unambiguous shipped enzyme" over a
    /// hand-written list of eight names, all of which have `ovhg == -4`. It
    /// swept one end geometry -- a 4-base 5' overhang -- out of the six the
    /// table holds, so not one 3'-overhang cutter, not one blunt cutter, not
    /// one 2- or 3-base overhang and not one Type IIS enzyme was ever run
    /// through it, and a maintainer reading the sentence to decide whether a
    /// change to `try_cut`'s boundary arithmetic was covered got a false yes.
    /// The qualifier was empty as well: no entry in the table has a non-ACGT
    /// site, so "unambiguous" excluded nothing. `sweep_enzymes` now supplies
    /// the list and [`the_duplex_sweep_runs_over_the_whole_shipped_table`]
    /// pins what it contains.
    ///
    /// Mutation that re-breaks this test: in `try_cut`, put the duplex filter
    /// back to the per-strand test --
    ///
    /// ```text
    /// let inside = |t: &i64| *t > w_lo && *t < w_hi
    ///                        && t + ovhg > c_lo && t + ovhg < c_hi;
    /// ```
    ///
    /// -- and the first named case alone takes it down, on `len()` 7 against
    /// `to_string_full()` 6 for BamHI+BspEI.
    ///
    /// The cost of the narrow list, measured rather than asserted: with that
    /// same mutation applied AND the `len()`/`to_string_full()` assertion in
    /// the loop below neutralised -- it otherwise aborts before anything is
    /// counted -- the `bad` assertion reports **11** malformed fragments for
    /// the eight names and **216** for the whole table. Both numbers are from
    /// running it here. The "128" in `try_cut`'s own comment is an older
    /// measurement of a different sweep and is left as the historical record it
    /// is; it is not reproducible from this test.
    #[test]
    fn no_double_digest_produces_a_fragment_with_no_base_pairs() {
        let es = sweep_enzymes();

        // BOTH ENZYMES IN ONE CALL. An earlier version of this test digested
        // sequentially -- `digest(seq, [a])` then `digest(frag, [b])` -- and
        // PASSED against the broken bounds, because that path re-enters
        // `try_cut` with a fresh single enzyme each time. The defect lives in
        // the multi-enzyme call, where the second enzyme's nicks are tested
        // against a molecule the first has already made sticky.
        let mut bad = Vec::new();
        // The case that first showed it, asserted by name so it cannot be lost
        // in the sweep.
        let mut cases: Vec<(String, String)> =
            vec![("BamHI+BspEI".into(), "AAAAAAAAAATCCGGATCCAAAAAAAAAA".into())];
        for a in &es {
            for b in &es {
                if a.name == b.name {
                    continue;
                }
                // Overlapping sites: the tail of one against the head of the
                // other, which is what an MCS looks like.
                let (sa, sb) = (&a.site, &b.site);
                for k in 1..sa.len().min(sb.len()) {
                    if sa[sa.len() - k..] == sb[..k] {
                        cases.push((
                            format!("{}+{}", a.name, b.name),
                            format!("AAAAAAAAAA{sa}{}AAAAAAAAAA", &sb[k..]),
                        ));
                    }
                }
                cases.push((
                    format!("{}+{}", a.name, b.name),
                    format!("AAAAAAAAAA{sa}{sb}AAAAAAAAAA"),
                ));
            }
        }
        for (what, seq) in &cases {
            for f in digest(&Dseq::new(seq, false), &es) {
                if paired_bases(&f) <= 0 {
                    bad.push(format!(
                        "{what} on {seq}: {{watson:{:?}, crick:{:?}, ovhg:{}}}",
                        f.watson, f.crick, f.ovhg
                    ));
                }
                assert_eq!(
                    f.len(),
                    f.to_string_full().len(),
                    "{what}: len() disagrees with to_string_full()"
                );
            }
        }
        assert!(
            bad.is_empty(),
            "{} fragment(s) have no base pairs at all:
  {}",
            bad.len(),
            bad.join(
                "
  "
            )
        );
    }

    /// The fragment `try_cut`'s duplex-filter comment names, counted rather
    /// than recalled.
    ///
    /// PROVEN TO FAIL against the number that comment and the test above
    /// carried until now. Both said "eleven bases of watson and crick"; the
    /// fragment is `CCG` over `GAT`, which is six. Written as the assertion
    /// `assert_eq!(bases, 11)` against this same fixture, the observed failure
    /// was:
    ///
    /// ```text
    /// assertion `left == right` failed: watson "CCG" + crick "GAT"
    ///   left: 6
    ///  right: 11
    /// ```
    ///
    /// The sweep above no longer asserts on this fragment — it enumerates
    /// enzyme pairs, and this one survives only as a named case whose *shape*
    /// nothing checks. So the prose was the only record of what the defect
    /// looked like, and the prose was wrong in the one place a reader would use
    /// to confirm they had reproduced it: someone re-deriving BspEI's and
    /// BamHI's nicks counts six, finds no reading that gives eleven, and cannot
    /// tell a correct re-derivation from a wrong one.
    ///
    /// Three of the four numbers around it were right, which is what made the
    /// fourth easy to walk past: `len()` = 7 and `to_string_full()` = 6 are
    /// asserted here too, from the shipped implementations rather than from the
    /// comment, and so is the 3-base overhang `left_end()` reports for an
    /// enzyme that always leaves 4.
    #[test]
    fn the_fragment_the_duplex_filter_comment_names_is_six_bases() {
        let f = Dseq::from_parts("CCG", "GAT", -4, false);
        let bases = f.watson.len() + f.crick.len();
        assert_eq!(bases, 6, "watson {:?} + crick {:?}", f.watson, f.crick);

        // Not one base pair, measured with `paired_bases` -- the sweep's own
        // instrument, so the premise here and the rule up there are one
        // statement. This used to be a second, inline copy of that expression,
        // and both copies read the sign of `ovhg` backwards; they agreed on
        // this fragment, which is why the error survived. See `paired_bases`.
        assert!(
            paired_bases(&f) <= 0,
            "the premise: the two strands do not overlap at all"
        );

        // The three neighbouring numbers, each from the shipped code.
        assert_eq!(f.len(), 7, "`len()` spans the gap between the strands");
        assert_eq!(f.to_string_full().len(), 6, "and the text does not");
        assert_eq!(
            f.left_end(),
            End::Overhang {
                five_prime: true,
                bases: "CCG".into()
            },
            "3 bases from an enzyme that always leaves 4"
        );
    }
}
