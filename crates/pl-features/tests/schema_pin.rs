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

    let rs_features: Vec<String> = pl_features::FEATURE_COLUMNS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rs_provenance: Vec<String> = pl_features::PROVENANCE_COLUMNS
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

    println!(
        "schema pinned across languages: {} feature column(s), {} provenance column(s)",
        rs_features.len(),
        rs_provenance.len()
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
