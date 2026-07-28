//! The notes report, at the boundary a caller actually sees.
//!
//! `snapgene::parse` builds `Document::unrepresentable_notes` and `load_all`
//! copies it into `LoadReport::unrepresentable_notes`; every consumer in the
//! workspace — `pl info`, `pl convert`, the GUI's SnapGene panel — reads the
//! second one. Nothing crossed that copy. Replacing the assignment in
//! `load_all` with `Vec::new()` left `cargo test --workspace` completely green:
//! the unit tests call `parse_notes` directly, and the corpus test reads
//! `Document`, so the one line that carries the report to its readers was held
//! up only by `reference/python/tests/xcheck_rust.py`, which is corpus-gated and
//! does not run in CI when `PL_CORPUS` is unset. A channel whose whole purpose
//! is to be seen needs a test at the surface it is seen through.
//!
//! std only, no dev-dependencies, like every other test here.

use pl_fileio::{load_all, load_with_report, snapgene, Format};

/// A `.dna` with a header, four bases and the block 6 payload given.
fn dna(notes: &str) -> Vec<u8> {
    let mut header = snapgene::MAGIC.to_vec();
    header.extend_from_slice(&1u16.to_be_bytes()); // DNA
    header.extend_from_slice(&15u16.to_be_bytes()); // export version
    header.extend_from_slice(&19u16.to_be_bytes()); // import version
    snapgene::write_blocks(&[
        snapgene::Block {
            kind: snapgene::block::HEADER,
            payload: header,
        },
        snapgene::Block {
            kind: snapgene::block::SEQUENCE,
            payload: vec![0x01, b'A', b'C', b'G', b'T'],
        },
        snapgene::Block {
            kind: snapgene::block::NOTES,
            payload: notes.as_bytes().to_vec(),
        },
    ])
}

#[test]
fn a_nested_note_reaches_the_load_report_and_not_only_the_document() {
    // The shape three of the 33 real `.dna` files on this machine carry: a
    // published citation, stored one level below a note.
    let bytes = dna(
        r#"<Notes><Type>Synthetic</Type><References><Reference pubMedID="9335267"/></References></Notes>"#,
    );

    let (mols, fmt, report) = load_all(&bytes).expect("a .dna we just wrote");
    assert_eq!(fmt, Format::SnapGene);
    assert_eq!(
        report.unrepresentable_notes,
        vec!["Notes/References/Reference".to_string()]
    );
    // The note itself is kept — this report is about what is *missing* from a
    // molecule that otherwise came through, not about a failed read.
    assert_eq!(mols[0].notes.len(), 2);
    assert_eq!(mols[0].note("Type"), Some("Synthetic"));

    // `load_with_report` is the one `pl info` and `pl convert` call. It is a
    // separate function with its own `LoadReport` construction, so asserting on
    // `load_all` alone would leave the actual production route uncovered.
    let (_, _, report) = load_with_report(&bytes).unwrap();
    assert_eq!(
        report.unrepresentable_notes,
        vec!["Notes/References/Reference".to_string()]
    );
}

#[test]
fn the_other_two_shapes_travel_the_same_channel() {
    // `unrepresentable_notes` carries three spellings and every consumer prints
    // them under one heading, so all three have to arrive.
    let (_, _, report) = load_with_report(&dna(
        r#"<Notes version="3"><Comments>Grown at 37 <sup>o</sup>C overnight</Comments></Notes>"#,
    ))
    .unwrap();
    assert_eq!(
        report.unrepresentable_notes,
        vec![
            "Notes@version".to_string(),
            "Notes/Comments/sup".to_string(),
            "Notes/Comments/text()".to_string(),
        ]
    );
}

#[test]
fn a_flat_notes_block_reports_nothing_at_all() {
    // The other half of a report that means something: it has to be quiet on the
    // ordinary file, or the notice becomes noise and gets ignored. This payload
    // is the shape of most real block 6s, whitespace between tags included.
    let (mol, _, report) = load_with_report(&dna(
        "<Notes>\n<Type>Synthetic</Type>\n<Created UTC=\"22:0:0\">2022.12.13</Created>\n</Notes>",
    ))
    .unwrap();
    assert!(
        report.unrepresentable_notes.is_empty(),
        "got {:?}",
        report.unrepresentable_notes
    );
    assert_eq!(mol.notes[1].attr("UTC"), Some("22:0:0"));
}

#[test]
fn a_genbank_file_leaves_the_notes_channel_empty_rather_than_borrowing_it() {
    // The two `unrepresentable_*` fields are deliberately separate: a GenBank
    // location form that cannot be represented must not arrive under a heading
    // about notes, and vice versa. `1^2` is one of the forms `parse_location`
    // reports.
    let gb = "LOCUS       x                          4 bp    DNA     linear   SYN 26-JUL-2026\n\
              FEATURES             Location/Qualifiers\n     misc_feature    1^2\n\
              ORIGIN\n        1 acgt\n//\n";
    let (_, fmt, report) = load_with_report(gb.as_bytes()).unwrap();
    assert_eq!(fmt, Format::GenBank);
    assert!(report.unrepresentable_notes.is_empty());
    assert_eq!(
        report.unrepresentable_locations,
        vec!["misc_feature: 1^2".to_string()]
    );
}
