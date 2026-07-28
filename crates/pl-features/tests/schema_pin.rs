//! The cross-language schema pin: `features/build/lib_columns.py` vs this crate.
//!
//! There are two definitions of the same schema in two languages, and until now
//! nothing compared them. The pin was real but indirect: `build.py` writes the
//! TSV header out of `FEATURE_COLUMNS`, and `Db::parse` compares the header of
//! the *generated* file against the Rust constant. So a Rust-side rename failed
//! immediately, and a Python-side rename left the whole suite green until
//! somebody rebuilt — proven by mutation, renaming `patent_flag` on the Python
//! side only and watching 65 + 4 tests pass.
//!
//! `lib_columns.py` meanwhile carried a docstring saying it was "kept in step
//! with crates/pl-features/src/lib.rs by a test that reads this file". No test
//! read it. This one does.
//!
//! Parsing Python from Rust is ugly, and it is the right ugly: the alternative
//! is a third copy of the column list, which is one more thing to drift.

use std::path::PathBuf;

fn lib_columns_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../features/build/lib_columns.py")
}

/// Pull one `NAME = [ "a", "b", ... ]` list out of the Python source.
///
/// Deliberately strict. A parser that silently returns an empty list on a
/// format it does not recognise turns this test into one that cannot fail, and
/// an empty list would compare equal to nothing and be reported as a mismatch —
/// which is the wrong error, pointing at the wrong file.
fn parse_python_list(src: &str, name: &str) -> Vec<String> {
    let start = src
        .find(&format!("{name} = ["))
        .unwrap_or_else(|| panic!("lib_columns.py has no `{name} = [`; the schema moved"));
    let rest = &src[start..];
    let open = rest.find('[').expect("no [");
    let close = rest.find(']').expect("no closing ] for the list");
    let body = &rest[open + 1..close];

    let mut out = Vec::new();
    for piece in body.split(',') {
        let t = piece.trim();
        if t.is_empty() {
            continue;
        }
        let unquoted = t
            .strip_prefix('"')
            .and_then(|x| x.strip_suffix('"'))
            .unwrap_or_else(|| panic!("{name}: {t:?} is not a plain double-quoted string"));
        out.push(unquoted.to_string());
    }
    assert!(
        !out.is_empty(),
        "{name} parsed as empty; the parser is broken"
    );
    out
}

#[test]
fn the_python_builder_and_the_rust_loader_agree_on_the_schema() {
    let path = lib_columns_path();
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let py_features = parse_python_list(&src, "FEATURE_COLUMNS");
    let py_provenance = parse_python_list(&src, "PROVENANCE_COLUMNS");
    let py_signoff = parse_python_list(&src, "SIGNOFF_COLUMNS");
    let py_signed = parse_python_list(&src, "SIGNED_COLUMNS");

    let rs_features: Vec<String> = pl_features::FEATURE_COLUMNS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rs_provenance: Vec<String> = pl_features::PROVENANCE_COLUMNS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rs_signoff: Vec<String> = pl_features::SIGNOFF_COLUMNS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rs_signed: Vec<String> = pl_features::SIGNED_COLUMNS
        .iter()
        .map(|s| s.to_string())
        .collect();

    // Named per side, so the failure message says which file to edit rather
    // than leaving a reader to diff two lists by eye.
    assert_eq!(
        py_features, rs_features,
        "features.tsv columns disagree.\n  \
         features/build/lib_columns.py: {py_features:?}\n  \
         crates/pl-features/src/lib.rs:  {rs_features:?}"
    );
    assert_eq!(
        py_provenance, rs_provenance,
        "provenance.tsv columns disagree.\n  \
         features/build/lib_columns.py: {py_provenance:?}\n  \
         crates/pl-features/src/lib.rs:  {rs_provenance:?}"
    );
    assert_eq!(
        py_signoff, rs_signoff,
        "SIGNOFF.tsv columns disagree.\n  \
         features/build/lib_columns.py: {py_signoff:?}\n  \
         crates/pl-features/src/lib.rs:  {rs_signoff:?}"
    );
    // The one that matters most, because it is not merely a header: both
    // languages compute a sha256 over these columns IN THIS ORDER, and the two
    // digests must agree or every signature in the repository lapses the moment
    // the other side recomputes it. A drift here is silent in both files.
    assert_eq!(
        py_signed, rs_signed,
        "the columns a sign-off covers disagree, so the two implementations of \
         the content digest cannot agree.\n  \
         features/build/lib_columns.py: {py_signed:?}\n  \
         crates/pl-features/src/lib.rs:  {rs_signed:?}"
    );

    // The digest excludes exactly the build's own bookkeeping. Asserted rather
    // than left implicit: `date_added` inside the digest would invalidate every
    // sign-off in the repository on every build, because build.py stamps it
    // from the clock.
    let excluded: Vec<&String> = rs_features
        .iter()
        .filter(|c| !rs_signed.contains(c))
        .collect();
    assert_eq!(
        excluded,
        vec!["id", "review_status", "curator", "date_added"],
        "the sign-off digest covers the wrong set of columns"
    );

    println!(
        "schema pinned across languages: {} feature column(s), {} provenance column(s), \
         {} sign-off column(s), {} signed column(s)",
        rs_features.len(),
        rs_provenance.len(),
        rs_signoff.len(),
        rs_signed.len()
    );
}

/// The two implementations of the sign-off digest must agree, byte for byte.
///
/// There are two of them — `Db::content_digest` here and `content_digest` in
/// `features/build/build.py` — and until this test nothing compared them. The
/// failure they can have is the worst shape available: each side is
/// individually green, and every signature in the repository verifies where it
/// was written and lapses everywhere else. The column list is pinned above;
/// this pins the *bytes*, which is where the framing, the length prefix, the
/// provenance ordering and the patent-flag canonicalisation all live.
///
/// The literal is one fixture row, hashed once, and asserted from both sides.
/// It is not "whatever the code produces" — `build.py`'s own self-test asserts
/// the identical string, so changing one side to match the other is a visible
/// two-file edit rather than a silent re-baseline.
#[test]
fn the_two_implementations_of_the_content_digest_agree() {
    const FH: &str = "id\tname\taliases\tclass\tgenbank_key\treference_nt\treference_aa\tboundary_rule\tboundary_evidence\tdescription\treview_status\tcurator\tdate_added\tpatent_flag\tnotes";
    const PH: &str =
        "record_id\tfield\tsource_db\tsource_accession\tlicence\turl\tretrieved\tsha256";
    // The alias cell is ` a | b |` and the patent flag is `TRUE`, both
    // deliberately. The fixture used to be `a|b` with `0`, and it could not see
    // either divergence the two implementations actually had:
    //
    //   * `Db::parse` trims aliases and drops empties; build.py joined them
    //     verbatim. A one-space curation typo therefore produced a signature
    //     that verified in every Python gate and LAPSED in the shipped binary,
    //     silently destroying a human's approval in the product.
    //   * `parse_flag` lowercases and accepts eleven spellings; build.py used a
    //     case-sensitive membership test over five. `TRUE` hashed as 0 on one
    //     side and 1 on the other.
    //
    // Both are invisible to a fixture that uses only canonical values, which is
    // exactly why they survived. A pin that only exercises the easy case is not
    // a pin.
    let f = format!(
        "{FH}\nPLF:0000\tx\t a | b |\tcds\tCDS\tATGTAA\tM\torf_atg_to_stop\tX.1:1-6:+\td\t\
         proposed\t\t2026-07-28\tTRUE\tn\n"
    );
    let p = format!(
        "{PH}\nPLF:0000\treference_nt\tena\tX.1\tINSDC-free\thttps://www.ebi.ac.uk/\t\
         2026-07-28\tabc\n"
    );
    let (db, _) = pl_features::Db::parse(&f, &p, "");
    assert_eq!(db.records.len(), 1, "the fixture row must load");
    assert_eq!(
        db.content_digest(&db.records[0]),
        "16ab78984c715df5cfa3cab396e8a6e11a56abe34318d10028648297194a947d",
        "the Rust and Python content digests have drifted. The same literal is \
         asserted by features/build/build.py's self_test; if you changed the \
         digest deliberately, change BOTH, and note that every existing \
         signature lapses."
    );

    // ...and the vector is not degenerate: perturbing one signed column must
    // move it, or the assertion above would hold for a digest that ignores its
    // input.
    let moved = f.replace("ATGTAA", "ATGTAG");
    let (db2, _) = pl_features::Db::parse(&moved, &p, "");
    assert_ne!(
        db2.content_digest(&db2.records[0]),
        db.content_digest(&db.records[0])
    );
}

/// The parser itself must be able to fail, or the test above proves nothing.
#[test]
fn the_pin_notices_a_rename_on_the_python_side() {
    let src = std::fs::read_to_string(lib_columns_path()).expect("read lib_columns.py");
    let mutated = src.replacen("\"patent_flag\"", "\"patent_flags\"", 1);
    assert_ne!(
        mutated, src,
        "the mutation did not apply; the fixture moved"
    );

    let py = parse_python_list(&mutated, "FEATURE_COLUMNS");
    let rs: Vec<String> = pl_features::FEATURE_COLUMNS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_ne!(
        py, rs,
        "a renamed Python column still compared equal to the Rust constant, so \
         the comparison above cannot fail and is worth nothing"
    );
}
