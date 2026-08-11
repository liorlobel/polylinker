//! The curated feature database, loaded ONCE for the whole process.
//!
//! `pl_features::Db::builtin()` parses three `include_str!`'d tables and
//! `Annotator::new` builds two k-mer indexes over the result. Neither depends
//! on a molecule — the annotator's own doc says so: "Building the index is the
//! expensive part and does not depend on the molecule, so it is done once and
//! reused across every file opened." A window with six tabs open would
//! otherwise pay for six of them, and every re-annotation after every edit
//! would pay again.
//!
//! Measured on this machine, release build, by differencing `pl` invocations
//! against a `pl --version` floor of ~30 ms: parsing the tables is about 4 ms
//! and building the two indexes about 13 ms. Small — and it is per *scan* that
//! it would be paid, which is the number that matters. It is also work with a
//! guaranteed-identical answer every time, which is the definition of something
//! that belongs behind a `OnceLock`.
//!
//! # Why `&'static` and not an `Arc`
//!
//! [`pl_features::annotate::Annotator`] borrows its `Db` (`Annotator<'a>`), so
//! an `Arc<Db>` plus an `Annotator` beside it is a self-referential struct and
//! is not expressible without unsafe. A process-lifetime `OnceLock` gives
//! `&'static Db` instead, which a worker thread can capture directly and which
//! costs nothing to clone. It is never freed, which is correct: it is
//! `include_str!`'d data that is live from the first annotation to exit.
//!
//! # What is deliberately NOT here
//!
//! The genetic code. [`pl_features::annotate::Config::default`] carries NCBI
//! table 11, which is what `pl annotate` defaults to and what the "Feature
//! annotation" methods page in `pl-doc` reads off the default and prints. The
//! application has a per-document `doc_code`, read from the file's modal
//! `/transl_table`, and wiring THAT in here would mean one annotator per code —
//! two fresh indexes built for a value the indexes do not depend on. The code
//! decides only which codons may open a reading frame, which matters solely for
//! the peptide-fusion gate. A user who needs another table has
//! `pl annotate --code`.

use std::sync::OnceLock;

use pl_features::annotate::{Annotator, Config};
use pl_features::{Db, LoadError};

/// The parsed tables, and whatever the loader could not read.
pub struct Library {
    /// Every row, including any a curator has not signed off.
    pub all: Db,
    /// The subset a release may ship: signed off AND licence-cleared. See
    /// [`pl_features::Db::reviewed`], which is emphatic that how many rows that
    /// is, is a property of `features/SIGNOFF.tsv` and not of the code.
    pub reviewed: Db,
    /// Rows the loader refused, kept rather than dropped.
    ///
    /// `pl annotate` prints these to stderr on every run. A GUI has no stderr
    /// anybody reads, so they are carried here and shown in the proposals
    /// panel: a database that silently lost rows produces a confidently short
    /// answer, which is the failure this whole crate is arranged against.
    pub errors: Vec<LoadError>,
}

static LIBRARY: OnceLock<Library> = OnceLock::new();

/// The tables. Parsed on the first call, from whichever thread makes it.
pub fn library() -> &'static Library {
    LIBRARY.get_or_init(|| {
        let (all, errors) = Db::builtin();
        let reviewed = all.reviewed();
        Library {
            all,
            reviewed,
            errors,
        }
    })
}

static REVIEWED: OnceLock<Annotator<'static>> = OnceLock::new();
static EVERYTHING: OnceLock<Annotator<'static>> = OnceLock::new();

/// The annotator, over the shippable subset or over the whole table.
///
/// **`unreviewed = false` is the default everywhere and matches `pl annotate`
/// exactly**, whose own default is `all.reviewed()` and whose `--include-proposed`
/// is the escape hatch. [`pl_features::Db::reviewed`] states the rule this
/// implements: "A caller that wants the proposed rows too has to ask for them
/// by name, and owes the user that sentence." The application asks by name
/// through a checkbox and prints the sentence beside it.
///
/// Two annotators and not one because the record indexes differ: an
/// [`pl_features::annotate::Annotation`]'s `record` field indexes the `Db` its
/// annotator was built over, so a hit produced against `reviewed` and resolved
/// against `all` names a different feature — plausibly, and with nothing
/// wrong-looking on screen. Whoever holds the hits must hold the `Db` too;
/// [`db`] is what returns the matching one.
///
/// Built lazily, so a user who never turns the unreviewed rows on never pays
/// for the second pair of indexes. That saving used to be the whole of it,
/// because the two tables held identical contents; since 2026-08-10 they do
/// not — 109 rows against 89 — so the second index is now genuinely a second
/// index and the laziness is buying real work, not just a clone.
pub fn annotator(unreviewed: bool) -> &'static Annotator<'static> {
    if unreviewed {
        EVERYTHING.get_or_init(|| Annotator::new(&library().all, Config::default()))
    } else {
        REVIEWED.get_or_init(|| Annotator::new(&library().reviewed, Config::default()))
    }
}

/// The database [`annotator`] of the same argument indexes into.
pub fn db(unreviewed: bool) -> &'static Db {
    if unreviewed {
        &library().all
    } else {
        &library().reviewed
    }
}

/// "3 record(s) could not be read from the feature database", or nothing.
///
/// A pure function over the count so the sentence can be tested: the shipped
/// tables load clean, so the only way to see this rendered is to break them,
/// and a line nobody can reach is a line nobody has read.
pub fn load_error_note(errors: &[LoadError]) -> Option<String> {
    if errors.is_empty() {
        return None;
    }
    let first: Vec<String> = errors.iter().take(3).map(|e| e.to_string()).collect();
    Some(format!(
        "{} row(s) could not be read from the feature database, so this search was made \
         over less than the whole of it: {}{}",
        errors.len(),
        first.join("; "),
        if errors.len() > first.len() {
            format!(" (and {} more)", errors.len() - first.len())
        } else {
            String::new()
        }
    ))
}

/// "No promoter, terminator or origin of replication is in it yet."
///
/// **Computed from the table, never written down.** `features/README.md` is
/// candid about this gap — "Promoters, origins and terminators have no
/// automatable source that gives a defensible boundary" — but that file does
/// not ship in the application, and a user does not read it before opening a
/// plasmid. What they do is open a plasmid, watch `AmpR` and `lacI` light up,
/// see no `ori`, and conclude their plasmid has no `ori`. The tool caused that
/// inference by demonstrating that it knows what features are, so the tool owes
/// them the sentence.
///
/// Which parts are absent is [`pl_features::Db::absent_common_kinds`], which is
/// computed from the table and is shared with the methods paragraph `pl-doc`
/// writes. Only the wording is here: a panel and a methods section are read by
/// people doing different things, and one sentence serving both would serve
/// neither — but they must not be able to disagree about the FACT, which is why
/// only one of them owns it.
///
/// Returns `None` when nothing is missing, so the panel says nothing rather
/// than something empty.
pub fn coverage_note(db: &Db) -> Option<String> {
    let missing = db.absent_common_kinds();
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "This database is not comprehensive, and no {} is in it yet — nothing found \
         here is a statement that your molecule has none.",
        join_and(&missing)
    ))
}

/// "a, b or c". Its own function so the three-item case is not a `join(", ")`
/// that reads as a list of two.
fn join_and(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [a] => (*a).to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

/// "2 record(s) are too short to seed and cannot be found: X, Y".
///
/// The GUI's spelling of the line `pl annotate` writes to stderr, and it exists
/// for the reason [`pl_features::annotate::Annotator::unseedable`] gives:
/// "a caller that believes it searched the whole database when it did not will
/// report a confident empty result."
pub fn unseedable_note(names: &[String]) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    Some(format!(
        "{} record(s) are too short to seed and cannot be found by this search: {}",
        names.len(),
        names.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The database is parsed once, whoever asks and however often.
    ///
    /// PROVEN TO FAIL by replacing `library()`'s body with a bare
    /// `Box::leak(Box::new(...))` per call, which type-checks and gives every
    /// caller a valid `&'static Library`:
    ///
    /// ```text
    /// the database was parsed twice
    /// ```
    ///
    /// Pointer identity is the only observable that can see this. A second
    /// parse produces an equal `Db` with equal contents, so nothing about the
    /// ANSWER differs — the cost is 17 ms and two indexes per document, per
    /// re-annotation, and a correctness test cannot see any of it.
    #[test]
    fn the_database_is_loaded_once_for_the_process() {
        let a = library();
        let b = library();
        assert!(
            std::ptr::eq(a, b),
            "the database was parsed twice; see this module's header for what that costs"
        );
        // ...and so is each annotator, which is where the 13 ms lives.
        assert!(std::ptr::eq(annotator(false), annotator(false)));
        assert!(std::ptr::eq(annotator(true), annotator(true)));
        // The two are DIFFERENT objects over different tables, which is the
        // property `db()` exists to keep straight.
        assert!(!std::ptr::eq(annotator(false), annotator(true)));
        assert!(std::ptr::eq(db(false), &library().reviewed));
        assert!(std::ptr::eq(db(true), &library().all));
    }

    /// A hit's `record` indexes the database its annotator was built over.
    ///
    /// PROVEN TO FAIL by making `db()` ignore its argument and always return
    /// `&library().all`. Pointer identity, not equality, is what catches it:
    /// `Db::reviewed` CLONES, so `all` and `reviewed` are two objects. When
    /// this was written they also held equal contents in equal order, and the
    /// comment said the two would diverge "the day a contributed row lands
    /// `proposed`". That day was 2026-08-10: the table now holds 109 rows and
    /// 89 signatures, so the two orders differ from `PLF:1014` onwards and a
    /// mismatched pair would put one record's name, id and review status
    /// against another record's coordinates — which is no longer a hypothetical
    /// and is exactly what pointer identity is here to prevent.
    #[test]
    fn each_annotator_is_paired_with_the_table_it_indexes() {
        assert!(std::ptr::eq(annotator(false).db(), db(false)));
        assert!(std::ptr::eq(annotator(true).db(), db(true)));
    }

    /// The shipped tables load clean, and the note says nothing when they do.
    #[test]
    fn the_shipped_database_loads_without_errors() {
        let lib = library();
        assert!(
            lib.errors.is_empty(),
            "the compiled-in tables no longer parse: {:?}",
            lib.errors
        );
        assert!(load_error_note(&lib.errors).is_none());
        assert!(
            !lib.all.records.is_empty(),
            "an empty database would make the whole feature findable-by-nothing"
        );
    }

    /// ...and when they do not, the user is told how many and which.
    ///
    /// The reachable-by-nothing half of the pair above. `LoadError` is a public
    /// struct, so the state can be constructed even though the shipped tables
    /// cannot produce it.
    #[test]
    fn a_database_that_lost_rows_says_so() {
        let one = vec![LoadError {
            file: "features.tsv",
            line: 12,
            problem: "unknown class 'plasmid'".into(),
        }];
        let note = load_error_note(&one).expect("one error is reported");
        assert!(note.contains("1 row(s)"), "{note}");
        assert!(note.contains("features.tsv:12"), "{note}");
        assert!(!note.contains("more"), "one error has no tail: {note}");

        let many: Vec<LoadError> = (0..5)
            .map(|i| LoadError {
                file: "features.tsv",
                line: i,
                problem: "bad".into(),
            })
            .collect();
        let note = load_error_note(&many).expect("five errors are reported");
        assert!(note.contains("5 row(s)"), "{note}");
        assert!(
            note.contains("(and 2 more)"),
            "a truncated list must say it is truncated: {note}"
        );
    }

    /// The gap the panel warns about is the gap the shipped table really has.
    ///
    /// The point of computing this rather than writing it down: the sentence
    /// and the data cannot disagree. This test pins both directions — the note
    /// names the three things that are absent from the shipped table, and a
    /// table that had them says nothing.
    ///
    /// PROVEN TO FAIL by dropping the `filter` from `coverage_note`, so that
    /// the sentence names all three whatever the table holds. The first half of
    /// this test still passes — today's table really does lack all three — and
    /// the second half is what catches it:
    ///
    /// ```text
    /// a database with no gap still apologises for one
    /// ```
    #[test]
    fn the_coverage_caveat_is_read_off_the_table_rather_than_asserted() {
        let lib = library();
        let note = coverage_note(&lib.reviewed).expect("the shipped table has all three gaps");
        for word in ["promoter", "terminator", "origin of replication"] {
            assert!(note.contains(word), "{word} is not mentioned: {note}");
        }
        assert!(
            note.contains("not comprehensive"),
            "the caveat does not say what it is: {note}"
        );
        // And the claim really is a claim about the data: the three INSDC keys
        // are absent from THE TABLE THE NOTE WAS COMPUTED OVER.
        //
        // `lib.reviewed`, and not `lib.all`. This read `lib.all` until
        // 2026-08-10, which was the same assertion while every row was signed
        // and became a wrong one the moment `proposed` promoter and
        // terminator rows landed: the note above is about what a default search
        // covers, and a proposed row is not in a default search. Asserting over
        // `all` made this test fail for a change that made the panel MORE
        // right, which is the wrong way round.
        for key in ["promoter", "terminator", "rep_origin"] {
            assert!(
                !lib.reviewed.records.iter().any(|r| r.genbank_key == key),
                "a `{key}` row is now signed off, so the caveat above is stale prose"
            );
        }
        // The other side of the same fact, so that this test notices if the two
        // tables silently become one again: the full table DOES hold promoters
        // and terminators, and a user who ticks "search unreviewed rows" is
        // shown a shorter caveat computed from it.
        assert!(
            lib.all.records.iter().any(|r| r.genbank_key == "promoter"),
            "the full table holds no promoter row at all; the Class B rows have gone \
             missing, or their genbank_key has changed under this test. HOW MANY of \
             them there are is deliberately not asserted: stage_classb refuses a row \
             whose extent only one submission corroborates, so the count is a property \
             of the evidence and moves without anything here being wrong"
        );
        let opted_in = coverage_note(&lib.all).expect("origins are still absent from both");
        assert!(
            !opted_in.contains("promoter"),
            "the opted-in caveat still apologises for promoters the table now holds: \
             {opted_in}"
        );

        // A table with all three says nothing at all.
        let mut db = lib.reviewed.clone();
        db.records.truncate(3);
        for (r, key) in db
            .records
            .iter_mut()
            .zip(["promoter", "terminator", "rep_origin"])
        {
            r.genbank_key = key.into();
        }
        assert!(
            coverage_note(&db).is_none(),
            "a database with no gap still apologises for one"
        );
        // ...and one still missing is still named, on its own, without a
        // dangling "or".
        db.records[0].genbank_key = "CDS".into();
        let note = coverage_note(&db).expect("one gap remains");
        assert!(note.contains("no promoter is in it"), "{note}");
        assert!(
            !note.contains(" or "),
            "a one-item list has no 'or': {note}"
        );
    }

    #[test]
    fn an_unsearchable_record_is_named_rather_than_silently_missing() {
        assert!(unseedable_note(&[]).is_none());
        let note = unseedable_note(&["AmpR".into(), "KanR".into()]).expect("two are reported");
        assert!(note.contains("2 record(s)"), "{note}");
        assert!(note.contains("AmpR, KanR"), "{note}");
    }
}
