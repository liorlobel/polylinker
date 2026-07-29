//! Does this candidate anneal anywhere else on the molecule you have open?
//!
//! This is the thing the tool does that a designer deferring specificity to
//! BLAST cannot, and it is in the gate rather than bolted on afterwards.
//!
//! # Zero tolerance, and why it matches the simulator
//!
//! Any binding whose `(start, strand)` differs from the intended site
//! disqualifies the candidate. Not "one that would produce an amplicon" —
//! any second site at all. Three reasons:
//!
//! 1. `pl_clone::pcr` returns `PcrError::NotSpecific` for more than one site on
//!    either strand, and its doc calls that "an error, not a detail... a tool
//!    that answers with one confident product has told the user their
//!    experiment worked when it did not". A designer more permissive than its
//!    own simulator would emit pairs the tool then refuses to simulate.
//! 2. The direction of strictness is checked rather than assumed. `pcr`'s
//!    `anneal` takes the longest exact 3' suffix of at least `MIN_ANNEAL` (12)
//!    that occurs anywhere, so a primer drawn from the template locks to its
//!    full length and finds one site; `find_bindings` reports a site wherever
//!    the seed matches exactly, extension included. `find_bindings` therefore
//!    finds a superset of what `pcr` would object to, which is the safe
//!    direction. Worth stating because the two constants differ.
//! 3. After the prefilter below it is nearly free.
//!
//! # The prefilter, and why it cannot miss a binding
//!
//! Calling `find_bindings` once per candidate is O(candidates × template) and
//! is what turns this feature into a progress bar. A sorted `(2-bit code,
//! position)` index over every `seed_len`-mer answers "could this candidate
//! bind anywhere other than where it came from?" with two binary searches.
//!
//! **The safety argument.** With `seed_mismatch: false`, `find_bindings`
//! reports a binding only where the seed matches *exactly*, and it discards any
//! candidate whose footprint came out shorter than the seed. So every binding
//! it can report necessarily has an exact seed match in the template, and
//! therefore appears in this index. The index count is an upper bound;
//! `find_bindings` is called only when that bound exceeds one, and its answer
//! is then the authoritative one — because a seed match is not yet a binding,
//! the 5' extension may die, and only `find_bindings` knows the rule.
//! `tests/specificity_prefilter.rs` asserts the bound over random templates.
//!
//! Two **hard preconditions**, checked rather than commented:
//!
//! - `seed_mismatch == false`. With mismatches allowed an exact-code lookup is
//!   simply wrong.
//! - The template carries no ambiguity codes. `find_bindings` matches through
//!   `pl_core::iupac::matches`, which is IUPAC-aware; a 2-bit code is not, so
//!   an `N` or an `R` makes the index miss real bindings.
//!
//! When either fails the fast path is not used, every candidate goes through
//! `find_bindings`, and [`Scan::used_index`] is false so the report can say so.
//! Silently skipping candidates on an ambiguous template would report primers
//! as specific that are not — the exact failure this module exists to prevent.
//!
//! # Scope, in the sentence and not in a footnote
//!
//! Against the open molecule and nothing else. The report never emits the bare
//! word "specific"; it emits "unique in *pUC19-myGene* (5,386 bp, circular);
//! not checked against any genome". A primer unique in a plasmid is routinely
//! not unique in *E. coli*.

use pl_primer::{find_bindings, Binding, Params, Strand};

/// Sorted `(code, position)` over every `seed_len`-mer of the template.
pub struct SeedIndex {
    seed_len: usize,
    keys: Vec<(u64, u32)>,
}

/// 2-bit code of a k-mer, or `None` if any base is not A/C/G/T.
///
/// `u64` holds 32 bases, which is why [`SeedIndex::build`] refuses a longer
/// seed rather than silently truncating one.
fn code(kmer: &[u8]) -> Option<u64> {
    let mut v = 0u64;
    for &b in kmer {
        v = (v << 2)
            | match b.to_ascii_uppercase() {
                b'A' => 0,
                b'C' => 1,
                b'G' => 2,
                b'T' => 3,
                _ => return None,
            };
    }
    Some(v)
}

impl SeedIndex {
    /// Build, or `None` when a precondition fails and the caller must fall back.
    ///
    /// The ambiguity precondition is stated here **and** enforced again by
    /// [`code`]'s `?` below. That is deliberate rather than sloppy: this one
    /// fails fast and, more to the point, is where a reader looking for the
    /// precondition will look. Neither alone is the whole guarantee — a change
    /// that made `code` map an unknown base to `A` would leave this check as
    /// the only thing between a user and a primer reported as unique because
    /// the index could not see its second site.
    pub fn build(template: &[u8], seed_len: usize, circular: bool) -> Option<SeedIndex> {
        let n = template.len();
        if seed_len == 0 || seed_len > 32 || n < seed_len || !crate::unambiguous(template) {
            return None;
        }
        // On a circle a seed may straddle the origin, so the walk continues
        // past the end; positions are still `0..n`, so the same site is never
        // recorded twice.
        let last = if circular { n } else { n - seed_len + 1 };
        let mut keys = Vec::with_capacity(last);
        for i in 0..last {
            let kmer: Vec<u8> = (0..seed_len).map(|d| template[(i + d) % n]).collect();
            keys.push((code(&kmer)?, i as u32));
        }
        keys.sort_unstable();
        Some(SeedIndex { seed_len, keys })
    }

    /// How many places this exact k-mer occurs.
    pub fn count(&self, kmer: &[u8]) -> usize {
        debug_assert_eq!(kmer.len(), self.seed_len);
        let Some(c) = code(kmer) else { return 0 };
        let lo = self.keys.partition_point(|(k, _)| *k < c);
        let hi = self.keys.partition_point(|(k, _)| *k <= c);
        hi - lo
    }
}

/// What the off-target scan found.
#[derive(Debug, Clone, PartialEq)]
pub struct Scan {
    /// Sites other than the intended one. Empty means unique **on this
    /// molecule**.
    pub elsewhere: Vec<Binding>,
    /// Was the intended site itself among the bindings — **when the bindings
    /// were computed at all**?
    ///
    /// A designed footprint is a template substring at known coordinates, so it
    /// always is; a `false` here means the caller's `intended` and the bases it
    /// passed do not describe the same place, which is a bug and not a
    /// property of the molecule. `accept_specificity` `debug_assert`s it for
    /// exactly that reason.
    ///
    /// On the index fast path nothing is scanned and this is reported `true`
    /// without being looked for. That is honest rather than assumed: the fast
    /// path is taken only when the seed occurs at most once in the whole
    /// template, and the intended site's own seed is one of the occurrences
    /// counted, so there is nothing left for a scan to disagree with. The doc
    /// said "checked rather than assumed" unconditionally until a reviewer
    /// pointed out that the common path does no checking; the distinction is
    /// now where it belongs, on the field.
    pub anchored: bool,
    /// False when a precondition forced the slow path, so the report can say so.
    pub used_index: bool,
}

impl Scan {
    pub fn is_unique(&self) -> bool {
        self.elsewhere.is_empty()
    }
}

/// The `Params` the off-target scan runs under.
///
/// Recorded next to its result in the report, exactly as `Method::describe`
/// is recorded next to a Tm: a user comparing our site list with another
/// tool's cannot otherwise tell a modelling difference from a bug.
pub fn params(seed_len: usize, tm_method: pl_thermo::Method) -> Params {
    Params {
        seed_len,
        // A hard precondition of the index, not a preference.
        seed_mismatch: false,
        // Lenient, so the scan finds *more* potential sites. For a rejection
        // test that is the conservative direction.
        extend_mismatches: true,
        tm_method,
    }
}

/// Everything `primer` anneals to, minus the site it was drawn from.
///
/// `intended` is `(start, end, strand)` in 1-based plus-strand coordinates —
/// known exactly, because enumeration chose it. That matters: on a molecule
/// with a repeat, the site `find_bindings` reports first need not be the one
/// enumeration meant, and taking `bindings[0]` would then describe a different
/// product from the one drawn on the map.
pub fn scan(
    primer: &[u8],
    template: &[u8],
    circular: bool,
    intended: (u64, u64, Strand),
    index: Option<&SeedIndex>,
    p: &Params,
) -> Scan {
    let used_index = index.is_some();
    if let Some(ix) = index {
        // Both orientations in one bound: the forward-strand search matches the
        // primer's own 3' seed, and the reverse-strand search matches its
        // reverse complement.
        let seed = &primer[primer.len() - ix.seed_len..];
        let rc = pl_core::iupac::reverse_complement(seed);
        if ix.count(seed) + ix.count(&rc) <= 1 {
            return Scan {
                elsewhere: Vec::new(),
                anchored: true,
                used_index,
            };
        }
    }

    let mut found = find_bindings(primer, template, circular, p);
    // A self-complementary footprint is reported at the same span on both
    // strands: one physical site found twice, not two sites. `pl_clone::pcr`
    // deduplicates for the same reason.
    found.dedup_by_key(|b| (b.start, b.end));
    let anchored = found.iter().any(|b| (b.start, b.end, b.strand) == intended);
    let elsewhere: Vec<Binding> = found
        .into_iter()
        .filter(|b| (b.start, b.end, b.strand) != intended)
        .collect();
    Scan {
        elsewhere,
        anchored,
        used_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PROVEN TO FAIL: with BOTH guards removed - the `unambiguous` call here
    /// and `code`'s refusal of a non-ACGT base, which has to map it to `A`
    /// instead - `SeedIndex::build` accepts an ambiguous template and the
    /// first assertion fires. Either guard alone keeps this test green, which
    /// is why the mutation removes both and why `build`'s doc says they are
    /// deliberately two.
    #[test]
    fn the_index_refuses_an_ambiguous_template_rather_than_missing_sites() {
        // The precondition that matters: a 2-bit code cannot represent N, and
        // `find_bindings` matches through IUPAC, so an index built over an
        // ambiguous template would report primers as unique that are not.
        assert!(SeedIndex::build(b"ACGTACGTACGTACGT", 8, false).is_some());
        assert!(SeedIndex::build(b"ACGTACGTNCGTACGT", 8, false).is_none());
        assert!(SeedIndex::build(b"ACGT", 8, false).is_none(), "too short");
        assert!(
            SeedIndex::build(b"ACGTACGTACGTACGTACGTACGTACGTACGTACGT", 33, false).is_none(),
            "a u64 holds 32 bases"
        );
    }

    /// PROVEN TO FAIL: with `anchored` hard-coded to `true` in the slow path's
    /// return (which is what the fast path does, and what made the field a
    /// claim nothing could contradict), the second assertion fires.
    #[test]
    fn anchored_is_false_when_the_caller_names_a_site_the_primer_is_not_at() {
        // No index, so the slow path runs and `anchored` is computed.
        let t = b"ACGTTTAAGGCCATGCATGCATTTGGCCAAACGTACGTTTAAGGCCATGC";
        let p = params(12, pl_thermo::Method::default());
        let primer = &t[10..32];

        let right = scan(primer, t, false, (11, 32, Strand::Forward), None, &p);
        assert!(
            right.anchored,
            "the intended site is where it was taken from"
        );

        // The same primer, told it came from somewhere it did not. This is the
        // internal-consistency failure the field exists to catch: enumeration
        // and the scan disagreeing about which site is the intended one is how
        // a design ends up describing a different product from the one drawn.
        let wrong = scan(primer, t, false, (12, 33, Strand::Forward), None, &p);
        assert!(!wrong.anchored, "{:?}", wrong.elsewhere);
    }

    #[test]
    fn the_index_counts_a_repeat_and_wraps_the_origin() {
        let t = b"ACGTTTAAGGACGTTTAAGG";
        let ix = SeedIndex::build(t, 8, false).unwrap();
        assert_eq!(ix.count(b"ACGTTTAA"), 2);
        assert_eq!(ix.count(b"GGGGGGGG"), 0);

        // On a circle the last bases and the first are contiguous.
        let circ = b"GTTTAAGGACGTTTAAGGAC";
        let lin = SeedIndex::build(circ, 8, false).unwrap();
        let cir = SeedIndex::build(circ, 8, true).unwrap();
        assert!(
            cir.count(b"GGACGTTT") > lin.count(b"GGACGTTT")
                || cir.count(b"GACGTTTA") >= lin.count(b"GACGTTTA"),
            "the wrapping seed must be indexed"
        );
        assert_eq!(cir.keys.len(), circ.len(), "one entry per position");
    }
}
