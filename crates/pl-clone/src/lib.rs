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
            // watson protrudes: a 5' overhang on the top strand
            std::cmp::Ordering::Less => End::Overhang {
                five_prime: true,
                bases: self.watson[..(-self.ovhg) as usize].to_string(),
            },
            // crick protrudes on the left, which is crick's 3' side
            std::cmp::Ordering::Greater => End::Overhang {
                five_prime: false,
                bases: self.crick[self.crick.len() - self.ovhg as usize..].to_string(),
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
            // watson runs past crick on the right: a 3' overhang on top
            std::cmp::Ordering::Greater => End::Overhang {
                five_prime: false,
                bases: self.watson[(w - d) as usize..].to_string(),
            },
            // crick runs past: a 5' overhang on the bottom strand
            std::cmp::Ordering::Less => End::Overhang {
                five_prime: true,
                bases: self.crick[..(-d) as usize].to_string(),
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
            let head = &self.crick[self.crick.len() - self.ovhg as usize..];
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

/// Where an enzyme cuts each strand, as offsets from the start of its site.
///
/// For the palindromic Type IIP enzymes in `pl-enzymes` the bottom-strand cut
/// is the mirror of the top-strand one, so it is derived rather than stored:
/// `GAATTC` cut at 1 on top is cut at 6 - 1 = 5 on the bottom, leaving the
/// four-base 5' overhang everyone knows as an EcoRI end.
fn strand_cuts(e: &Enzyme) -> (i64, i64) {
    let top = e.cut_offset as i64;
    let bottom = e.site.len() as i64 - top;
    (top, bottom)
}

/// The overhang an enzyme leaves: positive for 5', negative for 3', 0 blunt.
pub fn overhang_of(e: &Enzyme) -> i64 {
    let (top, bottom) = strand_cuts(e);
    bottom - top
}

/// Cut a molecule with one enzyme, returning the fragments with their ends.
///
/// A linear molecule with *k* cuts gives *k + 1* fragments; a circular one
/// gives *k*, because the first and last are the same piece — and a single cut
/// therefore linearises rather than fragmenting.
pub fn cut(seq: &Dseq, enzyme: &Enzyme) -> Vec<Dseq> {
    let full = seq.to_string_full();
    let n = full.len() as i64;
    if n == 0 {
        return Vec::new();
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
        return vec![seq.clone()];
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
        out.push(fragment(
            &full,
            (top_b[i], top_b[i + 1]),
            (bot_b[i], bot_b[i + 1]),
            n,
            seq.circular,
        ));
    }
    out
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
    /// A primer anneals in more than one place.
    ///
    /// This is an error, not a detail. A reaction whose primer binds three
    /// sites gives a smear or the wrong band, and a tool that answers with one
    /// confident product has told the user their experiment worked when it did
    /// not. `docs/PLAN.md` §7.12.2 puts this in hazard tier 1: silent,
    /// expensive, and hard to notice until the gel.
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
    NotSpecific {
        /// Which primer: "forward" or "reverse".
        primer: &'static str,
        /// 1-based positions where its 3' footprint anneals.
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
    let (_, f_len, f_top) =
        anneal(&tmpl, &fwd, template.circular).ok_or(PcrError::ForwardNotFound)?;
    let (_, r_len, r_top) =
        anneal_last(&tmpl, &rev_rc, template.circular).ok_or(PcrError::ReverseNotFound)?;

    // Specificity is judged over **both strands**.
    //
    // Searching only the top strand called a primer specific when its second
    // site was an inverted repeat: absent from the top strand, present on the
    // bottom, and pydna returns two products for exactly that input. Positions
    // are deduplicated, because a self-complementary footprint matches itself
    // on both strands and would otherwise count one real site twice.
    //
    // Known deviation, taken deliberately: pydna accepts a primer whose
    // reverse-complement site lies upstream, where the two extensions diverge
    // and no artifact forms. We refuse it. `docs/PLAN.md` §7.12.2 puts a
    // silently wrong PCR product in hazard tier 1 — a false "not specific"
    // costs a re-run, a false "specific" costs a cloning experiment.
    let sites_on_both_strands = |footprint: &str, top: &[usize]| -> Vec<usize> {
        let mut all = top.to_vec();
        all.extend(find_all(&tmpl, &rc(footprint), template.circular));
        all.sort_unstable();
        all.dedup();
        all
    };
    let f_sites = sites_on_both_strands(&fwd[fwd.len() - f_len..], &f_top);
    let r_sites = sites_on_both_strands(&rev_rc[..r_len], &r_top);

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

    // Exactly one site each by now, so the geometry has one answer.
    let f_start = f_sites[0] % n;
    let r_start = r_sites[0] % n;
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

/// Where the longest annealing suffix of `primer` binds.
///
/// Returns `(first start, matched length, every start)`. All the sites are
/// returned, not just the first, because how many there are decides whether
/// the reaction has a product at all.
fn anneal(tmpl: &str, primer: &str, circular: bool) -> Option<(usize, usize, Vec<usize>)> {
    for take in (MIN_ANNEAL.min(primer.len())..=primer.len()).rev() {
        let foot = &primer[primer.len() - take..];
        let sites = find_all(tmpl, foot, circular);
        if !sites.is_empty() {
            return Some((sites[0], take, sites));
        }
    }
    None
}

/// As [`anneal`], but for a probe matched by its 5' end — the reverse
/// primer's 3' end is the *start* of its reverse complement in the top strand.
fn anneal_last(tmpl: &str, probe: &str, circular: bool) -> Option<(usize, usize, Vec<usize>)> {
    for take in (MIN_ANNEAL.min(probe.len())..=probe.len()).rev() {
        let foot = &probe[..take];
        let sites = find_all(tmpl, foot, circular);
        if !sites.is_empty() {
            return Some((*sites.last().unwrap(), take, sites));
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
        let motif = "ACGTACGTACGTACGTACGT";
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
                assert_eq!(sites.len(), 2, "the motif appears twice");
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
    fn the_error_says_where_the_primer_binds() {
        let motif = "ACGTACGTACGTACGTACGT";
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
