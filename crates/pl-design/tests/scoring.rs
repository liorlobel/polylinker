//! Does the search bound rank on the same things the reported ranking does?
//!
//! `pair::cap` cuts each side to `max_per_side` survivors, and the report tells
//! the user that the cut is "by the per-oligo terms of the same score the pairs
//! are ranked by ... and by nothing else". That sentence is a claim about the
//! code, and it was false: `cap` weighted `Candidate::self_dimer_any` against
//! `DG_DIMER_ANY`, a quantity `score` has no term for, that never reached the
//! report, the JSON or the ranking, and that was read in exactly one place in
//! the whole crate — here. So the bound ordered candidates on a criterion the
//! reported ranking never applied, which is the "second, hidden set of criteria"
//! the comment at the call site says it avoids. The bound is not hypothetical:
//! it bit on an ordinary 3 kb / 400 bp run, 534 forward and 757 reverse
//! survivors both truncated to 200, so the inaccurate sentence was printed.
//!
//! # Why this reads source
//!
//! The honest test of "same terms" is a test about the *terms*, and terms are
//! not observable from outside: two scoring functions can agree on every input
//! a test can construct and still disagree on a quantity no fixture happens to
//! vary. `purity.rs` established the pattern in this crate and gives the same
//! justification — the rule is about what the crate ships, and a test is never
//! linked into the library.
//!
//! What is checked is narrow and mechanical: every field of [`Candidate`] that
//! `cap`'s sort key reads must also be read by `score`. Not the reverse —
//! `score` legitimately reads more, because it has a partner.

/// Fields that are coordinates rather than criteria.
///
/// `lo`, `hi` and `side` identify an oligo; reading one is a tie-break or an
/// index, never a preference. `bases` is the oligo itself.
const NOT_A_CRITERION: &[&str] = &["side", "lo", "hi", "bases"];

/// Names `Candidate` used to carry and must not quietly regain a use for.
///
/// `self_dimer_any` is the whole reason this file exists, and leaving it out
/// made the primary test below unable to fail: with the field and its `cap`
/// term restored — the shipped code — the scanner had no name to look for and
/// reported clean. A test whose coverage is derived from the current struct
/// cannot see a regression that reintroduces something the struct no longer
/// has, so the removed name is carried here deliberately.
const REMOVED: &[&str] = &["self_dimer_any"];

const SRC: &str = include_str!("../src/pair.rs");
const OLIGO_SRC: &str = include_str!("../src/oligo.rs");

/// Every field of `Candidate`, read off the struct plus [`REMOVED`].
///
/// Parsed rather than listed so a field added tomorrow is covered today.
fn candidate_fields() -> Vec<String> {
    let decl = body(OLIGO_SRC, "pub struct Candidate");
    let mut out: Vec<String> = decl
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub "))
        .filter_map(|r| r.split_once(':'))
        .map(|(name, _)| name.trim().to_string())
        .filter(|n| !n.contains('(') && !n.is_empty())
        .collect();
    out.extend(REMOVED.iter().map(|s| s.to_string()));
    out.sort();
    out.dedup();
    out
}

/// The body of a function or closure, from `head` to the line that closes it.
///
/// Brace counting rather than a parser: `pair.rs` is this repository's own file
/// and its braces balance, so the simple thing is the correct thing here.
fn body<'a>(src: &'a str, head: &str) -> &'a str {
    let start = src
        .find(head)
        .unwrap_or_else(|| panic!("{head:?} is not in pair.rs any more; this test needs updating"));
    let rest = &src[start..];
    let open = rest.find('{').expect("a body");
    let mut depth = 0usize;
    for (i, ch) in rest[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[open..open + i + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces after {head:?}");
}

/// Which `Candidate` fields a chunk of source reads, via `x.field`.
fn fields_read(chunk: &str) -> Vec<String> {
    candidate_fields()
        .into_iter()
        .filter(|f| !NOT_A_CRITERION.contains(&f.as_str()))
        .filter(|f| {
            // `k.tm`, `f.tm`, `r.tm` -- a binding, then the field. Anchored on
            // the dot, and on the character after the name, so `c.tm_opt` and
            // `c.tm_max` (constraint fields) cannot match the field `tm`.
            chunk.match_indices(&format!(".{f}")).any(|(i, _)| {
                let after = chunk[i + 1 + f.len()..].chars().next().unwrap_or(' ');
                !after.is_alphanumeric() && after != '_'
            })
        })
        .collect()
}

/// PROVEN TO FAIL: restoring `Candidate::self_dimer_any` and the
/// `+ unit(k.self_dimer_any.dg, c.dg_dimer_any)` term to `cap`'s structure sum
/// — the shipped code — reports
/// `the search bound ranks on ["self_dimer_any"], which the pair ranking never
/// applies`.
///
/// It fails for the right reason and not merely because a name changed: with
/// the term restored to BOTH functions it passes again, and with the term in
/// neither it passes. The assertion is about the difference.
#[test]
fn the_search_bound_ranks_on_nothing_the_reported_ranking_ignores() {
    let cap_key = body(SRC, "let key = |k: &Candidate|");
    let score_body = body(SRC, "fn score(");

    let capped = fields_read(cap_key);
    let scored = fields_read(score_body);
    assert!(
        !capped.is_empty() && !scored.is_empty(),
        "the scanner found nothing, so it is proving nothing: cap {capped:?} score {scored:?}"
    );

    let extra: Vec<&String> = capped.iter().filter(|f| !scored.contains(f)).collect();
    assert!(
        extra.is_empty(),
        "the search bound ranks on {extra:?}, which the pair ranking never applies. \
         Either add the term to score(), or drop it from cap() -- the report tells the \
         user the cut uses the per-oligo half of the same score, and that has to be true."
    );
}

/// The scanner is capable of seeing a term, so the test above is not vacuous.
#[test]
fn the_scanner_finds_what_it_is_looking_for() {
    let mut got = fields_read("unit(k.hairpin.dg, c.dg_hairpin) + (k.tm - c.tm_opt)");
    got.sort();
    assert_eq!(got, vec!["hairpin".to_string(), "tm".to_string()]);
    // A constraint field of a similar name is not a candidate field.
    assert!(fields_read("c.tm_opt + c.tm_max").is_empty());
    // Coordinates are excluded on purpose: cap() sorts on `k.lo` as a
    // tie-break, which is not a criterion and must not be reported as one.
    assert!(fields_read("k.lo, k.hi").is_empty());
    // And the removed name is still findable, which is the property that makes
    // the primary test able to fail at all.
    assert_eq!(
        fields_read("unit(k.self_dimer_any.dg, c.dg_dimer_any)"),
        vec!["self_dimer_any".to_string()]
    );
}

/// The field list really comes off the struct, so a new field is covered.
#[test]
fn the_field_list_is_read_from_the_struct_and_not_typed_out() {
    let f = candidate_fields();
    for want in ["tm", "gc", "hairpin", "self_dimer_three", "clamp", "lo"] {
        assert!(f.iter().any(|x| x == want), "{want} missing from {f:?}");
    }
    assert!(
        f.iter().any(|x| x == "self_dimer_any"),
        "the removed name has to stay findable: {f:?}"
    );
}
