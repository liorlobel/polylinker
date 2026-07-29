//! The safety argument for the fast path, checked rather than argued.
//!
//! The off-target scan skips `pl_primer::find_bindings` whenever a sorted 2-bit
//! seed index says a candidate's 3' seed occurs at most once. That is only
//! sound if the index count is an **upper bound** on the number of bindings
//! `find_bindings` would report — otherwise a primer with a second site is
//! reported as unique, which is precisely the failure the specificity check
//! exists to prevent, and it would be silent.
//!
//! The argument is: with `seed_mismatch: false`, `find_bindings` reports a
//! binding only where the seed matches exactly, and discards any candidate
//! whose footprint came out shorter than the seed. So every binding it can
//! report has an exact seed match and therefore appears in the index. This
//! checks it over templates chosen to have repeats, because on random sequence
//! the bound is trivially satisfied at zero and one and would prove nothing.

use pl_design::specificity::{params, SeedIndex};
use pl_primer::find_bindings;

fn lcg(n: usize, seed: u64) -> Vec<u8> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            b"ACGT"[((s >> 24) & 3) as usize]
        })
        .collect()
}

/// PROVEN TO FAIL: with the circular branch of `SeedIndex::build` removed, so
/// that seeds straddling the origin are never indexed, this reports
/// `index bound 0 is below 1 bindings for ACGACATGCGAAGTAAGCAAGG (circular
/// true)` - a primer that anneals across the origin waved through as unique.
#[test]
fn the_index_count_is_never_below_what_find_bindings_reports() {
    let sp = params(12, pl_thermo::Method::default());
    let mut checked = 0usize;
    let mut with_bindings = 0usize;
    let mut multi = 0usize;

    let mut inverted_cases = 0usize;
    let mut wrapping_sites = 0usize;
    for trial in 0..300u64 {
        // Two thirds of the templates carry a duplication, so the interesting
        // case -- more than one binding -- actually occurs. Without it every
        // count would be 1 and the inequality would hold vacuously.
        //
        // One of those thirds is an **inverted** repeat, and that is not
        // decoration. A direct repeat is found by the forward query alone, so a
        // prefilter that forgot to look up the seed's reverse complement would
        // pass a test built only from direct repeats -- measured: with the rc
        // query removed, the direct-repeat-only version of this file still
        // passed. An inverted repeat puts the second site on the minus strand,
        // where only the rc query can see it.
        let mut t = lcg(600, trial * 7 + 1);
        let circular = trial % 2 == 0;
        match trial % 3 {
            1 => {
                let block = t[100..250].to_vec();
                t[400..550].copy_from_slice(&block);
            }
            2 => {
                let block = pl_core::iupac::reverse_complement(&t[100..250]);
                t[400..550].copy_from_slice(&block);
                inverted_cases += 1;
            }
            _ => {}
        }
        let ix = SeedIndex::build(&t, sp.seed_len, circular).expect("unambiguous");

        // Candidates from every part of the molecule, plus -- on a circle --
        // one whose own site **straddles the origin**. Without that last case
        // the index could skip the wrap-spanning seeds entirely and this test
        // would not notice: measured, with the circular branch of
        // `SeedIndex::build` removed, the version of this loop that only took
        // `t[start..start + 22]` still passed.
        let mut probes: Vec<Vec<u8>> = (0..t.len() - 25)
            .step_by(17)
            .map(|s| t[s..s + 22].to_vec())
            .collect();
        if circular {
            for back in [1usize, 8, 15, 21] {
                let mut p = t[t.len() - back..].to_vec();
                p.extend_from_slice(&t[..22 - back]);
                probes.push(p);
            }
        }
        for primer in &probes {
            let primer = &primer[..];
            let seed = &primer[primer.len() - sp.seed_len..];
            let rc = pl_core::iupac::reverse_complement(seed);
            let bound = ix.count(seed) + ix.count(&rc);

            let mut found = find_bindings(primer, &t, circular, &sp);
            found.dedup_by_key(|b| (b.start, b.end));
            checked += 1;
            if !found.is_empty() {
                with_bindings += 1;
            }
            if found.len() > 1 {
                multi += 1;
            }
            assert!(
                bound >= found.len(),
                "index bound {bound} is below {} bindings for {} (circular {circular})",
                found.len(),
                String::from_utf8_lossy(primer)
            );
            if found.iter().any(|b| b.end < b.start) {
                wrapping_sites += 1;
            }
        }
    }

    // The counts that make the assertion above mean something. Without them a
    // build where `find_bindings` returned nothing at all would pass.
    assert!(checked > 5_000, "only {checked} candidates checked");
    assert_eq!(
        with_bindings, checked,
        "a primer drawn from the template must bind to it"
    );
    assert!(
        multi > 100,
        "only {multi} multi-site cases; the duplication fixtures are not doing their job"
    );
    assert!(
        inverted_cases > 50,
        "only {inverted_cases} inverted repeats"
    );
    assert!(
        wrapping_sites > 50,
        "only {wrapping_sites} origin-straddling sites; the circular probes are not \
         doing their job and the index's wrap handling is untested"
    );
}

/// The escalation path and the slow path agree.
///
/// The fast path is only ever allowed to say "at most one site"; whenever it
/// says more, `find_bindings` gives the authoritative answer. This checks that
/// running with and without the index gives the same verdict on every
/// candidate — which is the property a user would assume and which nothing else
/// asserts.
///
/// PROVEN TO FAIL: with the reverse-complement query dropped from the
/// prefilter, this reports `the index changed the answer for
/// CATCCGGGGACATGACTTTTAA at 590` - a primer whose second site is on the
/// minus strand, reported as unique.
#[test]
fn using_the_index_never_changes_the_verdict() {
    use pl_design::specificity::scan;
    use pl_primer::Strand;

    let sp = params(12, pl_thermo::Method::default());
    let mut t = lcg(800, 4242);
    // One direct repeat and one **inverted** one. The inverted half is what
    // makes the reverse-complement query in the prefilter load-bearing: without
    // it a candidate whose second site is on the minus strand is waved through
    // as unique, and with direct repeats alone this test could not tell.
    let block = t[100..250].to_vec();
    t[300..450].copy_from_slice(&block);
    let rc = pl_core::iupac::reverse_complement(&t[100..250]);
    t[600..750].copy_from_slice(&rc);
    let ix = SeedIndex::build(&t, sp.seed_len, false).unwrap();

    let mut agreed = 0usize;
    let mut unique = 0usize;
    for start in 0..t.len() - 22 {
        let primer = &t[start..start + 22];
        let intended = (start as u64 + 1, start as u64 + 22, Strand::Forward);
        let fast = scan(primer, &t, false, intended, Some(&ix), &sp);
        let slow = scan(primer, &t, false, intended, None, &sp);
        assert_eq!(
            fast.is_unique(),
            slow.is_unique(),
            "the index changed the answer for {} at {start}",
            String::from_utf8_lossy(primer)
        );
        agreed += 1;
        if fast.is_unique() {
            unique += 1;
        }
    }
    assert!(agreed > 700);
    // Both verdicts occur, so the agreement is not agreement on one answer.
    assert!(
        unique > 100 && unique < agreed,
        "{unique} of {agreed} unique"
    );
}
