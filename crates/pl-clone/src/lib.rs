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

pub(crate) fn rc(s: &str) -> String {
    String::from_utf8_lossy(&reverse_complement(s.as_bytes())).into_owned()
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
            // and every internal producer keeps `|ovhg| <= strand`.
            std::cmp::Ordering::Less => End::Overhang {
                five_prime: true,
                bases: self.watson[..((-self.ovhg) as usize).min(self.watson.len())].to_string(),
            },
            // crick protrudes on the left, which is crick's 3' side
            std::cmp::Ordering::Greater => End::Overhang {
                five_prime: false,
                bases: self.crick[self.crick.len().saturating_sub(self.ovhg as usize)..]
                    .to_string(),
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
            // clamped like `left_end`, so a malformed `Dseq` returns the bases
            // that exist rather than panicking; a no-op for any well-formed one.
            std::cmp::Ordering::Greater => End::Overhang {
                five_prime: false,
                bases: self.watson[((w - d).max(0) as usize).min(self.watson.len())..].to_string(),
            },
            // crick runs past: a 5' overhang on the bottom strand
            std::cmp::Ordering::Less => End::Overhang {
                five_prime: true,
                bases: self.crick[..((-d) as usize).min(self.crick.len())].to_string(),
            },
        }
    }

    /// The molecule as a single string, taking watson where it exists and
    /// filling from crick where it does not. Loses the end shapes, so it is for
    /// display and checksums rather than for cloning decisions.
    pub fn to_string_full(&self) -> String {
        let w = self.watson.len() as i64;
        let c = self.crick.len() as i64;
        let mut out = String::with_capacity(self.len());

        // crick protruding past watson on the left is crick's 3' end.
        if self.ovhg > 0 {
            // Clamped like `left_end`: a malformed `Dseq` with `ovhg` past the
            // crick length must not underflow this index.
            let head = &self.crick[self.crick.len().saturating_sub(self.ovhg as usize)..];
            out.push_str(&rc(head));
        }
        out.push_str(&self.watson);

        // ...and on the right it is crick's 5' end, which had no term at all.
        // Without it a sequential double digest *deleted bases*: cutting a
        // 15 nt molecule with BamHI then EcoRI summed to 11 nt, and the missing
        // GATC was not recoverable from any strand.
        let tail = (c - self.ovhg - w).min(c);
        if tail > 0 {
            out.push_str(&rc(&self.crick[..tail as usize]));
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
        // On a linear molecule the outermost boundaries are the molecule's own
        // ends, on both strands. Using `start + ovhg` there would invent an
        // overhang the molecule does not have, and lose the bases beyond it.
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
        // A `debug_assert` over the *tight* range says the same thing honestly
        // and trips if pl-enzymes ever loosens -- which it could, because
        // `cut_positions`' public doc does not promise this; only an internal
        // comment in `cut_sites` does.
        debug_assert!(
            tops.iter()
                .all(|t| (1..n).contains(t) && (1..n).contains(&(t + ovhg))),
            "pl-enzymes gave a linear cut with a nick off the molecule: \
             tops={tops:?} ovhg={ovhg} n={n}"
        );
        let mut t = vec![0i64];
        t.extend(tops.iter().copied());
        t.push(n);
        let mut b = vec![0i64];
        b.extend(tops.iter().map(|x| x + ovhg));
        b.push(n);
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
    /// Checked before any searching: `rc()` decodes through
    /// `from_utf8_lossy`, so a non-ASCII byte -- a non-breaking space pasted
    /// from a vendor's order sheet is the realistic case -- became a multi-byte
    /// replacement character and then panicked on a char boundary, aborting a
    /// whole batch rather than rejecting one primer.
    NotDna {
        what: &'static str,
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
    // ASCII up front. `rc()` goes through `from_utf8_lossy`, so a non-ASCII
    // byte anywhere -- a non-breaking space pasted from a vendor's order sheet
    // is the realistic case -- became a multi-byte replacement character and
    // then panicked on a char boundary deep inside the search, aborting a whole
    // batch run rather than rejecting one primer.
    for (what, s) in [("forward primer", forward), ("reverse primer", reverse)] {
        if !s.is_ascii() {
            return Err(PcrError::NotDna {
                what,
                found: s.chars().find(|c| !c.is_ascii()).unwrap_or('?'),
            });
        }
    }

    let tmpl = template.to_string_full().to_ascii_uppercase();
    if !tmpl.is_ascii() {
        return Err(PcrError::NotDna {
            what: "template",
            found: tmpl.chars().find(|c| !c.is_ascii()).unwrap_or('?'),
        });
    }
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
}
