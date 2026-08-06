//! Asking the library questions, and saying what was not asked.
//!
//! # The coverage footer is not decoration
//!
//! Every answer, including a successful one, carries a [`Coverage`]. A result
//! of "17 matched" is otherwise an unfalsifiable claim: it reads identically
//! whether 3,047 records were searched or 41 of them were quietly skipped for
//! having no bases. The footer is what turns it into an audited one, and the
//! arithmetic — searched plus every excluded bucket equals the total — is a
//! property test rather than a hope, because a footer that lies is worse than
//! no footer. It balances for every query that names a sequence criterion,
//! which is the only kind that scans. A `--name`-only query scans nothing, so
//! its footer would read "0 of 3,047 records searched" beside seventeen
//! matches; that is why the callers suppress it rather than print it. See
//! [`Coverage`], which states the precondition, and
//! `a_query_with_no_sequence_criterion_searches_nothing_and_excludes_nothing`,
//! which pins the shape.
//!
//! # Substring, not tokens
//!
//! `--name` and `--text` are case-insensitive ASCII substring matches with no
//! minimum length. That is a deliberate rejection of full-text indexing: FTS5's
//! default tokenizer returns nothing for `uc` against `pUC ori`, and its
//! trigram tokenizer returns nothing for any query under three characters. A
//! plasmid name is not word-shaped. Measured, the whole searchable text of the
//! real corpus is about a megabyte and a substring pass over it takes tens of
//! microseconds, so there is nothing here for an index to speed up.

use crate::scan::{find_in_row_capped, Hit, Motif};
use crate::{Row, State, Topology};

/// Hit coordinates one result set will keep, across every match in it.
///
/// **A bound on memory, never on the answer.** `total_hits` and
/// [`Match::hits_total`] are exact past this point; what stops is the storing.
///
/// The number is 24 MB of `Hit` at 24 bytes each, and it is set against the
/// biology rather than against a display. Over the 24 Mbase corpus this is
/// built for, a fully specified 6-mer yields about 5,700 hits, a 5-mer with one
/// degenerate position about 47,000, a 4-cutter like `GATC` about 94,000 and a
/// 3-mer about 375,000 — so every motif anyone is actually cloning with fits
/// whole, several times over. Past that, one- and two-base patterns yield
/// 12,002,567 and 24,000,000 hits and used to retain 299.8 MB and 590.0 MB, on
/// the UI thread, in a Library tab that re-runs its query on every repaint.
pub const MAX_RETAINED_HITS: u64 = 1_000_000;

/// What a query looked at, and what it could not.
///
/// Reported alongside every result. `searched + Σ(excluded) == total` holds
/// **whenever the query carried a sequence criterion**, and is asserted for
/// that case in `coverage_arithmetic_always_balances`, which draws
/// `motif: Some(..)` on every one of its 600 cases and so speaks only for it.
///
/// With no motif the equality is `0 == total`, deliberately: nothing is
/// scanned, so there is no scan for a record to be excluded *from*, and
/// `searched`, `excluded` and `filtered_out` all stay zero while `total` counts
/// the library — see [`run`] and
/// `a_query_with_no_sequence_criterion_searches_nothing_and_excludes_nothing`.
/// Stating the equality unconditionally, as this doc once did, invites a new
/// consumer to render `searched / total` as a coverage bar, which would read
/// 0% for every `--name` and `--text` query. Gate on the motif, as both shipped
/// callers do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    pub total: usize,
    /// Records whose sequence was actually scanned.
    pub searched: usize,
    /// Excluded, by reason, in the order they should be printed. Only non-zero
    /// buckets appear.
    pub excluded: Vec<(State, usize)>,
    /// Of the records searched, how many hold ambiguity codes.
    ///
    /// Carried because the match predicate is asymmetric: an `N` in a plasmid
    /// can *hide* a site, so a user reading "no hits" over records containing
    /// `N` deserves to know that.
    pub ambiguous_records: usize,
    /// Of the records searched, how many were scanned as circular only because
    /// their file never said.
    pub assumed_circular: usize,
    /// Records the non-sequence criteria ruled out before any scanning.
    ///
    /// Without this the footer does not balance: a `--name` filter that leaves
    /// one record out of nine says "1 of 9 records searched" and accounts for
    /// none of the other eight, which reads as eight records silently skipped.
    /// They were not skipped; they were not asked about.
    pub filtered_out: usize,
}

impl Coverage {
    /// Every record not searched, whatever the reason.
    pub fn excluded_total(&self) -> usize {
        self.filtered_out + self.excluded.iter().map(|(_, n)| n).sum::<usize>()
    }

    /// Human-readable, in the shape the CLI and the GUI both print.
    ///
    /// **Only meaningful when the query carried a sequence criterion.** With
    /// none, `searched` is 0 by design and the first line renders "0 of 3,047
    /// records searched" beside a full page of name matches — precisely the
    /// lying footer this module opens by condemning. The gating lives in the
    /// callers rather than here because an empty string would be
    /// indistinguishable from a library of zero records; a third caller has to
    /// gate too.
    pub fn describe(&self) -> String {
        let mut s = format!("{} of {} records searched", self.searched, self.total);
        if self.filtered_out > 0 {
            s.push_str(&format!(
                "
{:>7} ruled out by the other criteria before scanning",
                self.filtered_out
            ));
        }
        for (state, n) in &self.excluded {
            s.push_str(&format!("\n{:>7} {}", n, reason(*state)));
        }
        if self.ambiguous_records > 0 {
            s.push_str(&format!(
                "\n{:>7} contain ambiguity codes, which can hide a site",
                self.ambiguous_records
            ));
        }
        if self.assumed_circular > 0 {
            s.push_str(&format!(
                "\n{:>7} have undeclared topology and were scanned as circular",
                self.assumed_circular
            ));
        }
        s
    }
}

fn reason(state: State) -> &'static str {
    match state {
        State::Ok => "searched",
        State::NoBases => "have no sequence (a declared length, no bases)",
        State::AnnotationTrack => "are annotation tracks (coordinates, no bases)",
        State::NotASequenceFile => "are not sequence files (chromatograms and the like)",
        State::Unreadable => "could not be read",
        State::NotDownloaded => "are cloud placeholders that were not downloaded",
        State::TooLarge => "are past the size cap",
        State::SuspectParse => "parsed to nothing recognisable",
    }
}

/// Filters applied to a result set. All are conjunctive.
///
/// These refine; they are never a primary query. Asking "show me everything
/// between 3 and 5 kb" over three thousand plasmids is not a question anyone
/// actually has.
#[derive(Debug, Clone, Default)]
pub struct Filters {
    pub topology: Option<Topology>,
    pub state: Option<State>,
    pub min_len: Option<u64>,
    pub max_len: Option<u64>,
    pub min_features: Option<u32>,
    pub max_features: Option<u32>,
}

impl Filters {
    pub fn accepts(&self, r: &Row) -> bool {
        if let Some(t) = self.topology {
            if r.topology != t {
                return false;
            }
        }
        if let Some(s) = self.state {
            if r.state != s {
                return false;
            }
        }
        // Length means bases present, except for a record that declared one and
        // carried none -- for which the declared length is the only length
        // there is, and filtering it out would hide the file entirely.
        let len = if r.length > 0 {
            r.length
        } else {
            r.declared_len
        };
        if let Some(v) = self.min_len {
            if len < v {
                return false;
            }
        }
        if let Some(v) = self.max_len {
            if len > v {
                return false;
            }
        }
        if let Some(v) = self.min_features {
            if r.n_features < v {
                return false;
            }
        }
        if let Some(v) = self.max_features {
            if r.n_features > v {
                return false;
            }
        }
        true
    }
}

/// Case-insensitive ASCII substring.
///
/// Not tokenized and with no minimum length, which is the point: this is the
/// query Explorer already answers, and the one where an index that tokenizes
/// silently returns nothing for `uc` against `pUC ori`.
pub fn contains_fold(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.len() > h.len() {
        return false;
    }
    h.windows(n.len())
        .any(|w| w.iter().zip(n).all(|(a, b)| a.eq_ignore_ascii_case(b)))
}

/// One record that matched, with its hits if the query had any.
#[derive(Debug, Clone, PartialEq)]
pub struct Match<'a> {
    pub row: &'a Row,
    /// Hit coordinates in `(start, strand)` order — **the first `n` of them**,
    /// where `n` is whatever [`MAX_RETAINED_HITS`] left for this record.
    /// Usually all of them; see `hits_total` before printing a count.
    pub hits: Vec<Hit>,
    /// How many hits this record really has.
    ///
    /// Equal to `hits.len()` unless the result set exhausted its retention
    /// budget. A caller that prints `hits.len()` as "the number of sites" is
    /// printing a display artefact as a fact about a plasmid.
    pub hits_total: u64,
}

/// Everything a search returns.
#[derive(Debug, Clone)]
pub struct Results<'a> {
    pub matches: Vec<Match<'a>>,
    pub coverage: Coverage,
    /// Total hits across every match, before any display cap. Always computed,
    /// because "showing 200 of 6,088,143" is a different statement from
    /// "showing 200".
    pub total_hits: u64,
    /// Hits that were found and counted but whose coordinates were not kept,
    /// because the result set reached [`MAX_RETAINED_HITS`].
    ///
    /// Non-zero means some `Match::hits` is a prefix of that record's sites,
    /// and a caller drawing them has to say so. Zero for every query anyone
    /// runs on purpose.
    pub dropped_hits: u64,
}

/// A whole-library query.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub name: Option<String>,
    pub text: Option<String>,
    pub motif: Option<Motif>,
    pub filters: Filters,
    /// Invert the **sequence** criteria only.
    ///
    /// Deliberately narrow, and the single most likely silent wrong answer in
    /// the feature. "Plasmids with no BsaI site" must not quietly include the
    /// records whose bases we never had: a record we could not search is not
    /// evidence of absence, so it is **not** a match for `absent` and it is
    /// counted in the footer instead.
    pub absent: bool,
}

/// Run a query over a library.
///
/// Results are ordered by `(path, record)`, which the rows already are, so the
/// answer is deterministic without a final sort.
pub fn run<'a>(rows: &'a [Row], packed: &[u8], q: &Query) -> Results<'a> {
    let mut cov = Coverage {
        total: rows.len(),
        ..Default::default()
    };
    let mut buckets: Vec<(State, usize)> = Vec::new();
    let mut matches = Vec::new();
    let mut total_hits = 0u64;
    let mut retained = 0u64;
    let mut dropped_hits = 0u64;

    for row in rows {
        // Filters and text criteria apply to every record, searchable or not:
        // a chromatogram should still be findable by name.
        let ruled_out = !q.filters.accepts(row)
            || q.name
                .as_ref()
                .is_some_and(|n| !(contains_fold(&row.path, n) || contains_fold(&row.name, n)))
            || q.text
                .as_ref()
                .is_some_and(|t| !contains_fold(&row.text, t));
        if ruled_out {
            // Only counted when there is something to be excluded *from*: with
            // no sequence criterion nothing is searched, so a footer would be
            // reporting on a scan that never happened.
            if q.motif.is_some() {
                cov.filtered_out += 1;
            }
            continue;
        }

        let Some(motif) = &q.motif else {
            // No sequence criterion: everything surviving the filters matches,
            // and nothing was searched, so nothing is excluded either.
            matches.push(Match {
                row,
                hits: Vec::new(),
                hits_total: 0,
            });
            continue;
        };

        if !row.state.searchable() {
            let slot = buckets.iter_mut().find(|(s, _)| *s == row.state);
            match slot {
                Some((_, n)) => *n += 1,
                None => buckets.push((row.state, 1)),
            }
            // Never a match for `absent`: we did not look.
            continue;
        }

        cov.searched += 1;
        if row.ambiguous > 0 {
            cov.ambiguous_records += 1;
        }
        if !row.topology.declared() {
            cov.assumed_circular += 1;
        }

        // `--absent` needs to know whether there is a site, not where: one hit
        // is enough to answer, and the count comes back exact regardless.
        // Without this an inverted query over a poly-N record still built --
        // and threw away -- millions of coordinates.
        let cap = if q.absent {
            1
        } else {
            (MAX_RETAINED_HITS - retained) as usize
        };
        let (hits, n_hits) = find_in_row_capped(motif, packed, row, cap);
        let found = n_hits > 0;
        if found != q.absent {
            total_hits += n_hits;
            let kept = if q.absent { Vec::new() } else { hits };
            retained += kept.len() as u64;
            dropped_hits += n_hits - kept.len() as u64;
            matches.push(Match {
                row,
                hits: kept,
                hits_total: n_hits,
            });
        }
    }

    // Stable, so two runs print the same footer.
    buckets.sort_by_key(|(s, _)| s.as_str());
    cov.excluded = buckets;
    // The documented arithmetic, checked where it is produced rather than only
    // in a property test that draws one half of the domain. Guarded on the
    // motif because with no sequence criterion nothing is scanned and the
    // footer is all zeroes by design, not by accident.
    debug_assert!(
        q.motif.is_none() || cov.searched + cov.excluded_total() == cov.total,
        "coverage does not balance: {} searched + {} excluded != {} total",
        cov.searched,
        cov.excluded_total(),
        cov.total
    );
    Results {
        matches,
        coverage: cov,
        total_hits,
        dropped_hits,
    }
}

/// Records sharing a molecular identity, as groups of two or more.
///
/// Uses the key the caller stored — `cdseguid` for circular, `ldseguid` for
/// linear — so "these three files are one molecule" is exact rather than a
/// similarity judgement.
///
/// This is **not** a deduplication wizard and must never be presented as one.
/// It finds exact duplicates over the alphabets those keys admit; a
/// same-backbone-different-insert pair and a one-point-mutation pair are both
/// invisible to it, so "no duplicates" must never be shown as "no redundancy".
///
/// Its real use is a correctness requirement of bulk import rather than an
/// audit: without it, importing 3,000 files into a library already holding
/// 2,400 of them yields 5,400 entries.
pub fn identity_groups(rows: &[Row], key_of: impl Fn(&Row) -> Option<String>) -> Vec<Vec<&Row>> {
    let mut keyed: Vec<(String, &Row)> = rows
        .iter()
        .filter_map(|r| key_of(r).map(|k| (k, r)))
        .collect();
    keyed.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.path.cmp(&b.1.path))
            .then(a.1.record.cmp(&b.1.record))
    });
    let mut out: Vec<Vec<&Row>> = Vec::new();
    for (key, row) in keyed {
        match out.last_mut() {
            Some(g) if key_of(g[0]).as_deref() == Some(key.as_str()) => g.push(row),
            _ => out.push(vec![row]),
        }
    }
    out.retain(|g| g.len() > 1);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nibble;

    fn rng(state: &mut u64) -> u64 {
        *state ^= *state >> 12;
        *state ^= *state << 25;
        *state ^= *state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    const ALL_STATES: [State; 8] = [
        State::Ok,
        State::NoBases,
        State::AnnotationTrack,
        State::NotASequenceFile,
        State::Unreadable,
        State::NotDownloaded,
        State::TooLarge,
        State::SuspectParse,
    ];

    /// Three records: one with the site, one without, one an annotation track.
    fn library() -> (Vec<Row>, Vec<u8>) {
        let seqs: [&[u8]; 2] = [b"AAAGAATTCAAA", b"CCCCCCCCCCCC"];
        let mut all = Vec::new();
        let mut rows = Vec::new();
        let mut off = 0u64;
        for (i, s) in seqs.iter().enumerate() {
            all.extend_from_slice(s);
            rows.push(Row {
                path: format!("p{i}.gb"),
                name: format!("plasmid {i}"),
                state: State::Ok,
                topology: Topology::Circular,
                length: s.len() as u64,
                n_features: i as u32,
                text: if i == 0 {
                    "AmpR ori".into()
                } else {
                    "lacZ".into()
                },
                seq_off: off,
                seq_bases: s.len() as u64,
                ..Default::default()
            });
            off += s.len() as u64;
        }
        rows.push(Row {
            path: "track.gb".into(),
            name: "coords only".into(),
            state: State::AnnotationTrack,
            declared_len: 3000,
            n_features: 12,
            text: "AmpR".into(),
            ..Default::default()
        });
        (rows, nibble::pack(&all))
    }

    /// Test 16: the footer must balance, over every mix of states.
    #[test]
    fn coverage_arithmetic_always_balances() {
        let mut st = 0x7777_3333_9999_1111u64;
        for case in 0..600 {
            let n = (rng(&mut st) % 40) as usize;
            let seq = b"GAATTCAAAA";
            let mut packed_all = Vec::new();
            let mut rows = Vec::new();
            let mut off = 0u64;
            for i in 0..n {
                let state = ALL_STATES[(rng(&mut st) % 8) as usize];
                let searchable = state.searchable();
                if searchable {
                    packed_all.extend_from_slice(seq);
                }
                rows.push(Row {
                    path: format!("f{i}.gb"),
                    state,
                    topology: Topology::Circular,
                    seq_off: if searchable { off } else { 0 },
                    seq_bases: if searchable { seq.len() as u64 } else { 0 },
                    ..Default::default()
                });
                if searchable {
                    off += seq.len() as u64;
                }
            }
            let packed = nibble::pack(&packed_all);
            // Filters and text criteria are varied too, because they are the
            // other way a record ends up unsearched. A footer counting only
            // the unsearchable ones would say "1 of 9 records searched" and
            // account for none of the other eight, which reads as eight
            // records silently skipped.
            let q = Query {
                motif: Some(Motif::new("GAATTC").unwrap()),
                name: match rng(&mut st) % 4 {
                    0 => Some("f1".into()),
                    1 => Some("nothing matches this".into()),
                    _ => None,
                },
                filters: Filters {
                    topology: match rng(&mut st) % 4 {
                        0 => Some(Topology::Circular),
                        1 => Some(Topology::Linear),
                        _ => None,
                    },
                    min_len: if rng(&mut st).is_multiple_of(3) {
                        Some(5)
                    } else {
                        None
                    },
                    ..Default::default()
                },
                absent: rng(&mut st).is_multiple_of(5),
                ..Default::default()
            };
            let r = run(&rows, &packed, &q);
            assert_eq!(
                r.coverage.searched + r.coverage.excluded_total(),
                r.coverage.total,
                "case {case}: {} searched + {} excluded != {} total",
                r.coverage.searched,
                r.coverage.excluded_total(),
                r.coverage.total
            );
            // And nothing can match that was never looked at.
            assert!(
                r.matches.len() <= r.coverage.searched,
                "case {case}: more matches than records searched"
            );
        }
    }

    /// Test 15: the file that parses but is not a molecule. Six assertions,
    /// because getting one right while getting another wrong is the normal
    /// outcome.
    #[test]
    fn an_annotation_track_is_listed_excluded_and_never_a_false_absence() {
        let (rows, packed) = library();
        let motif = Motif::new("GAATTC").unwrap();

        // 1. It is not a motif match.
        let r = run(
            &rows,
            &packed,
            &Query {
                motif: Some(motif.clone()),
                ..Default::default()
            },
        );
        assert!(r.matches.iter().all(|m| m.row.path != "track.gb"));

        // 2. It is counted in the footer, under its own reason.
        assert_eq!(r.coverage.total, 3);
        assert_eq!(r.coverage.searched, 2);
        assert_eq!(r.coverage.excluded, vec![(State::AnnotationTrack, 1)]);

        // 3. It is NOT a match for --absent. We never looked, and a record we
        //    could not search is not evidence of absence.
        let r = run(
            &rows,
            &packed,
            &Query {
                motif: Some(motif.clone()),
                absent: true,
                ..Default::default()
            },
        );
        let absent: Vec<&str> = r.matches.iter().map(|m| m.row.path.as_str()).collect();
        assert_eq!(
            absent,
            vec!["p1.gb"],
            "only the record we searched and did not find the site in"
        );
        assert!(
            !absent.contains(&"track.gb"),
            "a record with no bases must never be reported as lacking a site"
        );

        // 4. It is still findable by name.
        let r = run(
            &rows,
            &packed,
            &Query {
                name: Some("track".into()),
                ..Default::default()
            },
        );
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].row.path, "track.gb");

        // 5. And by text.
        let r = run(
            &rows,
            &packed,
            &Query {
                text: Some("ampr".into()),
                ..Default::default()
            },
        );
        let found: Vec<&str> = r.matches.iter().map(|m| m.row.path.as_str()).collect();
        assert!(found.contains(&"track.gb"), "{found:?}");

        // 6. And by its declared length, which is the only length it has.
        let r = run(
            &rows,
            &packed,
            &Query {
                filters: Filters {
                    min_len: Some(2000),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].row.path, "track.gb");
    }

    #[test]
    fn absent_inverts_the_sequence_criteria_and_nothing_else() {
        let (rows, packed) = library();
        let q = |absent| Query {
            motif: Some(Motif::new("GAATTC").unwrap()),
            name: Some("plasmid".into()),
            absent,
            ..Default::default()
        };
        let present: Vec<&str> = run(&rows, &packed, &q(false))
            .matches
            .iter()
            .map(|m| m.row.path.as_str())
            .collect();
        let missing: Vec<&str> = run(&rows, &packed, &q(true))
            .matches
            .iter()
            .map(|m| m.row.path.as_str())
            .collect();
        assert_eq!(present, vec!["p0.gb"]);
        // The name filter still applies, so track.gb is out for that reason,
        // and p1 is in for lacking the site.
        assert_eq!(missing, vec!["p1.gb"]);
    }

    #[test]
    fn substring_matching_finds_what_a_tokenizer_would_miss() {
        // The queries that decided the storage engine. FTS5's default
        // tokenizer returns nothing for `uc` against `pUC ori`, and its trigram
        // tokenizer returns nothing for any query under three characters.
        assert!(contains_fold("pUC ori", "uc"));
        assert!(contains_fold("pSC101 ori", "101"));
        assert!(contains_fold("Rep101(Ts)", "101"));
        assert!(contains_fold("T7 terminator", "or"));
        assert!(contains_fold("AmpR promoter", "AMPR"));
        assert!(contains_fold("anything", ""));
        assert!(!contains_fold("short", "much longer needle"));
        assert!(!contains_fold("", "x"));
        // Case folding is ASCII only, which is stated rather than assumed.
        assert!(contains_fold("LACZ", "lacz"));
    }

    #[test]
    fn a_query_with_no_sequence_criterion_searches_nothing_and_excludes_nothing() {
        // Otherwise a name search over a library of chromatograms would print
        // a footer full of exclusions that had no bearing on the question.
        let (rows, packed) = library();
        let r = run(
            &rows,
            &packed,
            &Query {
                // `.gb` rather than `p`: `track.gb` is neither named nor
                // pathed with a `p`, and the point here is that all three
                // records reach the answer.
                name: Some(".gb".into()),
                ..Default::default()
            },
        );
        assert_eq!(r.coverage.searched, 0);
        assert!(r.coverage.excluded.is_empty());
        assert_eq!(r.total_hits, 0);
        assert_eq!(r.matches.len(), 3);
    }

    #[test]
    fn the_true_hit_total_is_computed_even_when_the_caller_will_truncate() {
        // "showing 200 of 6,088,143" is a different statement from "showing
        // 200", and only one of them is honest.
        let seq = vec![b'A'; 500];
        let packed = nibble::pack(&seq);
        let rows = vec![Row {
            path: "poly.gb".into(),
            state: State::Ok,
            topology: Topology::Linear,
            length: 500,
            seq_bases: 500,
            ..Default::default()
        }];
        let r = run(
            &rows,
            &packed,
            &Query {
                motif: Some(Motif::new("A").unwrap()),
                ..Default::default()
            },
        );
        // `A` is not palindromic, so + and - are separate scans; the minus
        // strand of a poly-A is poly-T and matches nothing.
        assert_eq!(r.total_hits, 500);
        assert_eq!(r.matches[0].hits.len(), 500);
    }

    #[test]
    fn a_result_set_stops_retaining_hits_past_its_budget_and_says_how_many_it_dropped() {
        // The Library tab re-runs its query on the UI thread on every repaint,
        // and a one-base motif over the 24 Mbase corpus used to build a
        // 12,002,567-element `Vec<Hit>` -- 299.8 MB -- before a single row was
        // drawn. Only the display was ever capped. Nothing may be lost to the
        // bound except coordinates: the counts stay exact.
        let half = (MAX_RETAINED_HITS / 2) as usize;
        let seq = vec![b'A'; half];
        let mut all = Vec::new();
        let mut rows = Vec::new();
        for i in 0..3 {
            all.extend_from_slice(&seq);
            rows.push(Row {
                path: format!("poly{i}.gb"),
                state: State::Ok,
                topology: Topology::Linear,
                length: half as u64,
                seq_off: (i * half) as u64,
                seq_bases: half as u64,
                ..Default::default()
            });
        }
        let packed = nibble::pack(&all);
        let r = run(
            &rows,
            &packed,
            &Query {
                // Not palindromic: the minus strand of a poly-A is poly-T and
                // matches nothing, so the arithmetic below is one hit per base.
                motif: Some(Motif::new("A").unwrap()),
                ..Default::default()
            },
        );

        assert_eq!(r.matches.len(), 3, "every record still matched");
        assert_eq!(
            r.total_hits,
            MAX_RETAINED_HITS + half as u64,
            "the count is exact past the bound; it is the storing that stops"
        );
        for m in &r.matches {
            assert_eq!(
                m.hits_total, half as u64,
                "{}: its own count must be exact too",
                m.row.path
            );
        }
        let kept: u64 = r.matches.iter().map(|m| m.hits.len() as u64).sum();
        assert_eq!(kept, MAX_RETAINED_HITS, "retention stopped at the bound");
        assert_eq!(
            r.dropped_hits, half as u64,
            "and the drop is reported rather than swallowed"
        );
        // The kept ones are a prefix, not an arbitrary sample.
        assert_eq!(r.matches[0].hits[0].start, 1);
        assert!(r.matches[2].hits.is_empty(), "the budget was already spent");
    }

    #[test]
    fn a_query_within_the_budget_keeps_every_hit_and_drops_nothing() {
        // The control. The bound must be invisible to every query anyone runs
        // on purpose -- a 6-mer over the whole measured corpus is about 5,700
        // hits -- or it has traded one silent wrong answer for another.
        let (rows, packed) = library();
        let r = run(
            &rows,
            &packed,
            &Query {
                motif: Some(Motif::new("GAATTC").unwrap()),
                ..Default::default()
            },
        );
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].hits.len(), 1);
        assert_eq!(r.matches[0].hits_total, 1);
        assert_eq!(r.total_hits, 1);
        assert_eq!(r.dropped_hits, 0, "nothing was near the bound");
    }

    #[test]
    fn capping_a_single_record_keeps_the_first_hits_and_still_counts_them_all() {
        // Directly against `find_in_row_capped`, because `run`'s budget only
        // ever exercises the cap at one boundary.
        let seq = b"GAATTCGAATTCGAATTCGAATTC";
        let packed = nibble::pack(seq);
        let row = Row {
            state: State::Ok,
            topology: Topology::Linear,
            length: seq.len() as u64,
            seq_bases: seq.len() as u64,
            ..Default::default()
        };
        let motif = Motif::new("GAATTC").unwrap();
        let full = crate::scan::find_in_row(&motif, &packed, &row);
        assert_eq!(full.len(), 4, "the uncapped call is unchanged");

        let (hits, total) = crate::scan::find_in_row_capped(&motif, &packed, &row, 2);
        assert_eq!(total, 4, "every hit is counted, capped or not");
        assert_eq!(hits, full[..2].to_vec(), "and the kept ones are the first");

        let (hits, total) = crate::scan::find_in_row_capped(&motif, &packed, &row, 0);
        assert!(hits.is_empty());
        assert_eq!(total, 4, "a cap of zero still counts");

        // Two strands, collected separately: each is capped on its own, so the
        // merged list has to be cut again or a cap of 2 returns 4.
        //          1234567890123456789012
        let seq = b"ATGATGAAAAAACATCATAAAA";
        let packed = nibble::pack(seq);
        let row = Row {
            state: State::Ok,
            topology: Topology::Linear,
            length: seq.len() as u64,
            seq_bases: seq.len() as u64,
            ..Default::default()
        };
        let motif = Motif::new("ATG").unwrap();
        let full = crate::scan::find_in_row(&motif, &packed, &row);
        assert_eq!(full.len(), 4, "two on each strand");
        let (hits, total) = crate::scan::find_in_row_capped(&motif, &packed, &row, 2);
        assert_eq!(total, 4);
        assert_eq!(
            hits,
            full[..2].to_vec(),
            "a cap of 2 is 2, not 2 per strand"
        );
    }

    #[test]
    fn identity_groups_are_groups_of_two_or_more_in_a_stable_order() {
        let rows = vec![
            Row {
                path: "b.gb".into(),
                ..Default::default()
            },
            Row {
                path: "a.gb".into(),
                ..Default::default()
            },
            Row {
                path: "c.gb".into(),
                ..Default::default()
            },
            Row {
                path: "solo.gb".into(),
                ..Default::default()
            },
        ];
        let key = |r: &Row| match r.path.as_str() {
            "solo.gb" => Some("zzz".to_string()),
            "c.gb" => None, // no key at all: never grouped
            _ => Some("same".to_string()),
        };
        let groups = identity_groups(&rows, key);
        assert_eq!(groups.len(), 1, "only the pair, not the singleton");
        let paths: Vec<&str> = groups[0].iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["a.gb", "b.gb"], "sorted within the group");
    }

    #[test]
    fn ambiguity_and_assumed_circularity_are_counted_not_assumed_away() {
        let seq = b"AAANGAATTCAAA";
        let packed = nibble::pack(seq);
        let rows = vec![Row {
            path: "n.gb".into(),
            state: State::Ok,
            topology: Topology::Undeclared,
            length: seq.len() as u64,
            seq_bases: seq.len() as u64,
            ambiguous: 1,
            ..Default::default()
        }];
        let r = run(
            &rows,
            &packed,
            &Query {
                motif: Some(Motif::new("GAATTC").unwrap()),
                ..Default::default()
            },
        );
        assert_eq!(r.coverage.searched, 1);
        assert_eq!(r.coverage.ambiguous_records, 1);
        assert_eq!(r.coverage.assumed_circular, 1);
        let d = r.coverage.describe();
        assert!(d.contains("ambiguity codes"), "{d}");
        assert!(d.contains("undeclared topology"), "{d}");
    }
}
