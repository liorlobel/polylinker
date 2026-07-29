//! Tests for the editing model.
//!
//! Every one of these runs without an egui context. That is the whole reason
//! the model is a separate module: a design that can only be exercised by
//! driving a window does not get exercised.

use super::*;
use pl_core::{Feature, Segment, Topology};

/// Time, in the units `egui::InputState::time` uses. Tests that mean "still
/// typing" pass increasing values inside [`Run::IDLE_SECONDS`].
const T0: f64 = 100.0;

fn mol(seq: &str, circular: bool) -> Molecule {
    Molecule {
        seq: seq.as_bytes().to_vec(),
        topology: if circular {
            Topology::Circular
        } else {
            Topology::Linear
        },
        ..Default::default()
    }
}

fn feature(m: &mut Molecule, name: &str, start: u64, end: u64) {
    let mut f = Feature::new(name, "misc_feature");
    f.segments.push(Segment::new(start, end));
    m.features.push(f);
}

fn doc(seq: &str, circular: bool) -> Document {
    Document::of_molecule(mol(seq, circular))
}

fn seq_of(d: &Document) -> String {
    String::from_utf8(d.molecule().seq.clone()).unwrap()
}

fn coords(d: &Document, name: &str) -> (u64, u64) {
    let f = d
        .molecule()
        .features
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no feature {name}; have {:?}", names(d)));
    (f.start(), f.end())
}

fn names(d: &Document) -> Vec<String> {
    d.molecule()
        .features
        .iter()
        .map(|f| f.name.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Typing
// ---------------------------------------------------------------------------

#[test]
fn typing_a_base_inserts_it_at_the_caret() {
    // The typed base is deliberately one that appears nowhere near the caret.
    // Typing a `T` into "ACGT|ACGT" gives "ACGTTACGT" whether it lands at
    // position 4 or at position 5, so that fixture could not tell a correct
    // caret from one off by one — which is the only thing this test is for.
    let mut d = doc("ACGTACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    e.type_text(&mut d, "N", T0);
    e.commit(&mut d);

    assert_eq!(seq_of(&d), "ACGTNACGT");
    assert_eq!(e.caret, 5, "the caret follows what was typed");
    assert_eq!(d.log.path().len(), 1, "one operation, through the log");
}

#[test]
fn the_caret_is_a_gap_and_the_op_carries_caret_plus_one() {
    // The single off-by-one in this whole surface, pinned. `apply` validates
    // `1 <= at <= n + 1` and splices at `at - 1`, so caret space and `at` space
    // are in bijection with no gaps and no second shift anywhere.
    let run = Run {
        start: 0,
        removed: 0,
        inserted: "X".into(),
        kind: RunKind::Insert,
        last_input: 0.0,
    };
    assert_eq!(
        run.to_op(),
        Some(OpKind::InsertAt {
            at: 1,
            seq: "X".into()
        })
    );
    let end = Run { start: 12, ..run };
    assert_eq!(
        end.to_op(),
        Some(OpKind::InsertAt {
            at: 13,
            seq: "X".into()
        })
    );
}

#[test]
fn typing_at_the_very_end_and_the_very_start_both_work() {
    let mut d = doc("ACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    e.type_text(&mut d, "Z", T0); // rejected
    e.type_text(&mut d, "G", T0);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ACGTG");

    e.place(&mut d, 0, false);
    e.type_text(&mut d, "A", T0 + 3.0);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "AACGTG");
}

#[test]
fn typing_over_a_selection_replaces_it_in_one_operation() {
    let mut d = doc("AAAACCCCGGGG", false);
    let mut e = SeqEdit::new();
    e.sel = Some(Selection {
        anchor: 4,
        head: 8,
        through_origin: false,
    });
    e.caret = 8;
    e.type_text(&mut d, "TT", T0);
    e.commit(&mut d);

    assert_eq!(seq_of(&d), "AAAATTGGGG");
    assert_eq!(d.log.path().len(), 1);
    assert!(matches!(
        d.log.path()[0].kind,
        OpKind::ReplaceRange {
            start: 5,
            len: 4,
            ..
        }
    ));
    assert_eq!(e.caret, 6);
}

// ---------------------------------------------------------------------------
// Case
// ---------------------------------------------------------------------------

#[test]
fn case_is_preserved_through_an_edit() {
    // `Molecule::seq` is case-preserved by contract: lowercase conventionally
    // marks a soft-masked region or a non-annealing primer tail, and the File
    // tab counts those bases and labels them. An editor that upper-cases on the
    // first keystroke has destroyed information and visibly zeroed a counter
    // the application already shows.
    let mut d = doc("ACGTACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 8;
    e.type_text(&mut d, "acgtNn", T0);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ACGTACGTacgtNn", "byte for byte as typed");

    // ...and typing over a lowercase selection does not inherit its case.
    let mut d = doc("atg", false);
    let mut e = SeqEdit::new();
    e.sel = Some(Selection {
        anchor: 0,
        head: 3,
        through_origin: false,
    });
    e.type_text(&mut d, "ATG", T0);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ATG");
}

#[test]
fn pasting_mixed_case_changes_not_one_character() {
    let r = sanitise_paste(">x\nACGTacgtNnRy\n");
    assert_eq!(r.bases, "ACGTacgtNnRy");
}

// ---------------------------------------------------------------------------
// Undo
// ---------------------------------------------------------------------------

#[test]
fn undo_restores_the_sequence_and_the_annotation_coordinates() {
    let mut m = mol("AAAACCCCGGGGTTTT", false);
    feature(&mut m, "gg", 9, 12); // the GGGG
    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();

    assert_eq!(coords(&d, "gg"), (9, 12));
    e.caret = 0;
    e.type_text(&mut d, "TTT", T0);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "TTTAAAACCCCGGGGTTTT");
    assert_eq!(coords(&d, "gg"), (12, 15), "the feature followed its bases");

    d.undo().unwrap();
    e.restore(&d);
    assert_eq!(seq_of(&d), "AAAACCCCGGGGTTTT");
    assert_eq!(
        coords(&d, "gg"),
        (9, 12),
        "and undo brought the coordinates back, not just the bases"
    );
    assert!(e.caret <= d.molecule().len());
}

// ---------------------------------------------------------------------------
// The insertion boundary
// ---------------------------------------------------------------------------

#[test]
fn a_feature_after_the_insertion_moves_and_one_before_it_does_not() {
    let mut m = mol("AAAACCCCGGGGTTTT", false);
    feature(&mut m, "before", 1, 4);
    feature(&mut m, "after", 13, 16);
    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();

    e.caret = 8; // between the C run and the G run
    e.type_text(&mut d, "NNNNN", T0);
    e.commit(&mut d);

    assert_eq!(coords(&d, "before"), (1, 4), "untouched");
    assert_eq!(coords(&d, "after"), (18, 21), "moved by exactly 5");
}

#[test]
fn an_insertion_at_a_features_first_base_pushes_it_rather_than_extending_it() {
    // The correct and non-obvious half of the boundary rule: `remap_annotations`
    // shifts coordinates `>= at`, so an insertion at the caret immediately 5' of
    // a feature leaves the inserted bases OUTSIDE it. Getting this backwards
    // silently grows a promoter by whatever was pasted in front of it.
    let mut m = mol("ACGTACGTACGT", false);
    feature(&mut m, "f", 5, 8);
    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();

    e.caret = 4; // at == 5 == f.start()
    e.type_text(&mut d, "TT", T0);
    e.commit(&mut d);
    assert_eq!(coords(&d, "f"), (7, 10), "pushed, not extended");

    // ...and one gap further in really does extend it.
    let mut m = mol("ACGTACGTACGT", false);
    feature(&mut m, "f", 5, 8);
    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();
    e.caret = 5; // at == 6, inside
    e.type_text(&mut d, "TT", T0);
    e.commit(&mut d);
    assert_eq!(coords(&d, "f"), (5, 10), "extended");
}

// ---------------------------------------------------------------------------
// Deleting
// ---------------------------------------------------------------------------

#[test]
fn backspace_and_delete_take_one_base_from_the_right_side_of_the_caret() {
    let mut d = doc("ABCDEF", false);
    let mut e = SeqEdit::new();
    e.caret = 3;
    e.backspace(&mut d, T0);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ABDEF");
    assert_eq!(e.caret, 2);

    let mut d = doc("ABCDEF", false);
    let mut e = SeqEdit::new();
    e.caret = 3;
    e.delete_forward(&mut d, T0);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ABCEF");
    assert_eq!(e.caret, 3);
}

#[test]
fn backspace_at_the_start_of_a_line_does_nothing_and_says_why() {
    let mut d = doc("ACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 0;
    e.backspace(&mut d, T0);
    assert_eq!(seq_of(&d), "ACGT");
    assert!(!d.edited(), "a no-op must not enter the history");
    assert!(e
        .notice
        .as_deref()
        .unwrap()
        .contains("nothing before base 1"));
}

#[test]
fn backspace_at_base_one_of_a_circle_names_the_alternative_rather_than_wrapping() {
    // Deleting the LAST base because the user pressed Backspace on row 1 is an
    // edit whose entire visible effect is off-screen.
    let mut d = doc("ACGTACGTACGT", true);
    let mut e = SeqEdit::new();
    e.caret = 0;
    e.backspace(&mut d, T0);
    assert_eq!(seq_of(&d), "ACGTACGTACGT");
    let msg = e.notice.as_deref().unwrap();
    assert!(msg.contains("base 12"), "{msg}");
    assert!(msg.contains("Set origin"), "{msg}");
}

#[test]
fn an_empty_selection_never_becomes_a_zero_length_range_op() {
    // `DeleteRange { len: 0 }` is refused by the engine; `ReplaceRange { len: 0 }`
    // is accepted and would describe an insertion as "replace 0 bp at 5 with
    // 1 bp" in a provenance log that is never rewritten.
    let m = mol("ACGTACGT", false);
    let (ops, _) = ops_for_range_edit(&m, Selection::point(4), None);
    assert!(ops.is_empty());
    let (ops, _) = ops_for_range_edit(&m, Selection::point(4), Some("A"));
    assert_eq!(
        ops,
        vec![OpKind::InsertAt {
            at: 5,
            seq: "A".into()
        }],
        "an insertion is spelled InsertAt, always"
    );
}

#[test]
fn selecting_everything_and_deleting_is_permitted() {
    let mut m = mol("AAAACCCCGGGG", false);
    feature(&mut m, "f", 1, 12);
    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();
    e.sel = Some(Selection {
        anchor: 0,
        head: 12,
        through_origin: false,
    });
    e.backspace(&mut d, T0);
    assert_eq!(seq_of(&d), "");
    assert!(d.molecule().features.is_empty());
    assert!(e.notice.as_deref().unwrap().contains("removed"));
}

// ---------------------------------------------------------------------------
// The origin
// ---------------------------------------------------------------------------

#[test]
fn an_origin_crossing_selection_deletes_the_bases_it_names() {
    // 12 bp circle. Select bases 11, 12, 1, 2 — the arc from caret 10 forwards
    // through the origin to caret 2 — and delete it.
    let mut m = mol("ABCDEFGHIJKL", true);
    feature(&mut m, "wrapper", 11, 2); // itself crosses the origin
    feature(&mut m, "inner", 5, 8); // EFGH
    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();

    e.sel = Some(Selection {
        anchor: 10,
        head: 2,
        through_origin: true,
    });
    e.backspace(&mut d, T0);

    assert_eq!(seq_of(&d), "CDEFGHIJ", "exactly K, L, A and B went");
    assert_eq!(
        names(&d),
        vec!["inner".to_string()],
        "the feature whose every base was deleted is removed, not clamped"
    );
    assert_eq!(coords(&d, "inner"), (3, 6));
    assert_eq!(
        &d.molecule().seq[2..6],
        b"EFGH",
        "and it still names the bases it always named"
    );
    assert_eq!(e.caret, 0);
    assert_eq!(
        d.log.path().len(),
        2,
        "a rotation and a deletion; the history says so honestly"
    );
    assert!(e.notice.as_deref().unwrap().contains("renumbered"));
}

#[test]
fn an_origin_crossing_selection_is_rotate_then_one_range_op() {
    let m = mol("ABCDEFGHIJKL", true);
    let (ops, caret) = ops_for_range_edit(
        &m,
        Selection {
            anchor: 10,
            head: 2,
            through_origin: true,
        },
        None,
    );
    assert_eq!(
        ops,
        vec![
            OpKind::Rotate { origin: 11 },
            OpKind::DeleteRange { start: 1, len: 4 },
        ]
    );
    assert_eq!(caret, 0);
}

#[test]
fn a_wrap_flag_at_the_ends_is_canonicalised_before_it_reaches_rotate() {
    // Without this, `origin = hi + 1 = n + 1` reaches `Rotate` and the engine
    // refuses it ("origin at 13 is outside a 12 bp molecule") — which the user
    // would read as the editor breaking on an ordinary selection at the end of
    // the sequence.
    let s = Selection {
        anchor: 4,
        head: 12,
        through_origin: true,
    }
    .canonical(12, true);
    assert!(!s.through_origin);
    assert_eq!(
        (s.lo(), s.hi()),
        (0, 4),
        "the arc really is just bases 1..=4"
    );

    let s = Selection {
        anchor: 0,
        head: 4,
        through_origin: true,
    }
    .canonical(12, true);
    assert!(!s.through_origin);
    assert_eq!((s.lo(), s.hi()), (4, 12));

    let s = Selection {
        anchor: 4,
        head: 8,
        through_origin: true,
    }
    .canonical(12, false);
    assert!(!s.through_origin, "a line has no origin to cross");
}

#[test]
fn a_crossing_selections_length_is_the_bases_not_the_caret_difference() {
    // Printing the caret difference reports 4,921 for a 465 bp selection.
    let s = Selection {
        anchor: 40,
        head: 4961,
        through_origin: true,
    };
    assert_eq!(s.base_count(5386), 465);
    let s = Selection {
        through_origin: false,
        ..s
    };
    assert_eq!(s.base_count(5386), 4921);
}

#[test]
fn copying_a_crossing_selection_reads_it_in_origin_crossing_order() {
    let m = mol("ABCDEFGHIJKL", true);
    let mut e = SeqEdit::new();
    e.sel = Some(Selection {
        anchor: 10,
        head: 2,
        through_origin: true,
    });
    assert_eq!(e.copy(&m).unwrap().0, "KLAB");
    e.sel = Some(Selection {
        anchor: 10,
        head: 2,
        through_origin: false,
    });
    assert_eq!(e.copy(&m).unwrap().0, "CDEFGHIJ");
}

#[test]
fn an_empty_selection_copies_nothing_rather_than_the_whole_molecule() {
    let m = mol("ACGTACGT", true);
    let mut e = SeqEdit::new();
    e.sel = Some(Selection::point(3));
    assert_eq!(e.copy(&m), None);
}

// ---------------------------------------------------------------------------
// Caret transport
// ---------------------------------------------------------------------------

#[test]
fn reverse_complement_reflects_the_caret_as_a_gap_not_as_a_base() {
    // `pl-core` reflects BASES with `p -> n + 1 - p`; GAPS reflect with
    // `c -> n - c`. On "AAAAGGGG" -> "CCCCTTTT" the caret at the A|G boundary
    // must land on the C|T boundary. Using the base formula puts it one base
    // out at every position, including at both ends.
    let n = 8;
    assert_eq!(transport(4, &OpKind::ReverseComplement, n), 4);
    assert_eq!(transport(0, &OpKind::ReverseComplement, n), 8);
    assert_eq!(transport(8, &OpKind::ReverseComplement, n), 0);
    assert_eq!(transport(1, &OpKind::ReverseComplement, n), 7);
}

#[test]
fn rotation_carries_the_caret_the_same_way_it_carries_a_feature() {
    // "ABCDEFGHIJKL" rotated to start at K: the gap 3' of base 11 (caret 11)
    // becomes the gap 3' of base 1.
    let n = 12;
    assert_eq!(transport(11, &OpKind::Rotate { origin: 11 }, n), 1);
    assert_eq!(transport(12, &OpKind::Rotate { origin: 11 }, n), 2);
    // Caret 0 means "the gap 3' of base n", which is the same junction as n.
    assert_eq!(transport(0, &OpKind::Rotate { origin: 1 }, n), 12);
}

// ---------------------------------------------------------------------------
// Coalescing
// ---------------------------------------------------------------------------

#[test]
fn a_run_of_typed_bases_is_one_operation_and_one_undo() {
    // Design B's measured reason: one operation per keystroke costs 89.5 kB of
    // retained snapshot per keystroke on a 4.6 Mb genome — 197 MB after 2,000
    // keystrokes, growing linearly and, by the log's central promise, never
    // evicted. The same 2,000 keystrokes coalesced stay flat at 18 MB.
    let mut d = doc("ACGTACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 8;

    let typed = "ATGCATGCATGCATGCATGC";
    for (i, c) in typed.chars().enumerate() {
        e.type_text(&mut d, &c.to_string(), T0 + i as f64 * 0.1);
    }
    assert_eq!(
        d.log.path().len(),
        0,
        "nothing is in the log while the run is open"
    );
    assert_eq!(
        e.effective_len(d.molecule()),
        8 + typed.len() as u64,
        "but the user can see every character"
    );

    e.commit(&mut d);
    assert_eq!(seq_of(&d), format!("ACGTACGT{typed}"));
    assert_eq!(
        d.log.path().len(),
        1,
        "twenty keystrokes, one operation, one history row"
    );

    d.undo().unwrap();
    e.restore(&d);
    assert_eq!(
        seq_of(&d),
        "ACGTACGT",
        "and one Ctrl+Z takes all twenty back"
    );
}

#[test]
fn a_run_breaks_when_the_caret_moves_when_it_goes_idle_and_at_the_cap() {
    // Moving the caret.
    let mut d = doc("ACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    e.type_text(&mut d, "AA", T0);
    e.place(&mut d, 0, false);
    e.type_text(&mut d, "TT", T0 + 0.2);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "TTACGTAA");
    assert_eq!(
        d.log.path().len(),
        2,
        "two things the user did, two entries"
    );

    // A pause to think.
    let mut d = doc("ACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    e.type_text(&mut d, "A", T0);
    e.type_text(&mut d, "T", T0 + Run::IDLE_SECONDS + 0.1);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ACGTAT");
    assert_eq!(d.log.path().len(), 2);

    // The cap, so one Ctrl+Z never swallows an unbounded amount of typing.
    let mut d = doc("ACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    for i in 0..Run::MAX_CHARS + 10 {
        e.type_text(&mut d, "A", T0 + i as f64 * 0.001);
    }
    e.commit(&mut d);
    assert_eq!(d.molecule().len(), 4 + Run::MAX_CHARS as u64 + 10);
    assert_eq!(d.log.path().len(), 2);
}

#[test]
fn backspace_during_a_typing_run_starts_a_new_run() {
    // Design B rule 3: a different op kind intervening breaks the run. The user
    // would be surprised if one Ctrl+Z undid both the typing and the deleting.
    let mut d = doc("ACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    e.type_text(&mut d, "GGG", T0);
    e.backspace(&mut d, T0 + 0.1);
    e.backspace(&mut d, T0 + 0.2);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ACGTG");
    assert_eq!(d.log.path().len(), 2);

    d.undo().unwrap();
    assert_eq!(seq_of(&d), "ACGTGGG", "the deleting comes back first");
}

#[test]
fn a_held_backspace_is_one_deletion() {
    let mut d = doc("ACGTACGTACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 12;
    for i in 0..6 {
        e.backspace(&mut d, T0 + i as f64 * 0.03);
    }
    assert_eq!(e.effective_len(d.molecule()), 6);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ACGTAC");
    assert_eq!(d.log.path().len(), 1);
    assert!(matches!(
        d.log.path()[0].kind,
        OpKind::DeleteRange { start: 7, len: 6 }
    ));
    assert_eq!(e.caret, 6);
}

#[test]
fn the_open_run_is_visible_before_it_is_committed() {
    // The view splices the pending text into the visible rows only. If this
    // ever disagrees with what commit produces, the user is looking at a
    // document the log does not have.
    let mut d = doc("AAAACCCC", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    e.type_text(&mut d, "GGG", T0);

    let mut shown = String::new();
    e.row_text(d.molecule(), 0, e.effective_len(d.molecule()), &mut shown);
    assert_eq!(shown, "AAAAGGGCCCC");

    e.commit(&mut d);
    assert_eq!(seq_of(&d), shown);
}

#[test]
fn a_pending_backspace_run_is_visible_too() {
    let mut d = doc("ABCDEFGH", false);
    let mut e = SeqEdit::new();
    e.caret = 6;
    e.backspace(&mut d, T0);
    e.backspace(&mut d, T0 + 0.05);
    let mut shown = String::new();
    e.row_text(d.molecule(), 0, e.effective_len(d.molecule()), &mut shown);
    assert_eq!(shown, "ABCDGH", "E and F, the two bases left of the caret");
    e.commit(&mut d);
    assert_eq!(seq_of(&d), shown);
}

// ---------------------------------------------------------------------------
// Rejecting input
// ---------------------------------------------------------------------------

#[test]
fn a_non_iupac_character_is_refused_named_and_never_inserted() {
    // Design C: reject the character, tell the user, do not swallow it.
    // `Molecule::validate` inspects coordinates and never looks at the
    // sequence — measured, after `InsertAt { seq: "zzz" }` it returns `[]` —
    // so the corruption gate cannot see a junk base and the keystroke is the
    // only moment this can be said.
    let mut d = doc("ACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 2;
    e.type_text(&mut d, "Z", T0);
    e.commit(&mut d);

    assert_eq!(seq_of(&d), "ACGT", "nothing was inserted");
    assert!(!d.edited(), "and no operation was recorded");
    assert_eq!(e.caret, 2, "the caret did not move");
    let msg = e.notice.as_deref().unwrap();
    assert!(msg.contains('Z'), "the character itself is named: {msg}");
    assert!(msg.contains("not a nucleotide"), "{msg}");
}

#[test]
fn the_accepted_alphabet_is_iupac_including_u_and_both_cases() {
    for c in "ACGTURYSWKMBDHVNacgturyswkmbdhvn".chars() {
        assert!(is_base(c), "{c} is a nucleotide code");
    }
    for c in "ZQEXOJ1234-. *>\u{2019}".chars() {
        assert!(!is_base(c), "{c} is not");
    }
}

#[test]
fn a_mixed_text_event_inserts_what_it_can_and_reports_the_rest() {
    // `egui::Event::Text` carries a String and may hold more than one char.
    let mut d = doc("ACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    e.type_text(&mut d, "AZTQG", T0);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ACGTATG");
    let msg = e.notice.as_deref().unwrap();
    assert!(msg.contains("ignored"), "{msg}");
}

// ---------------------------------------------------------------------------
// Paste
// ---------------------------------------------------------------------------

#[test]
fn a_fasta_record_pastes_its_bases_and_nothing_else() {
    let r = sanitise_paste(">pUC19 cloning vector\nACGTACGTAC\nGTACGTACGT\n");
    assert_eq!(r.bases, "ACGTACGTACGTACGTACGT");
    assert!(r.rejected.is_empty());
    assert!(r.refused.is_none());
    assert!(
        r.dropped.iter().any(|d| d.contains("pUC19")),
        "the header is reported verbatim: {:?}",
        r.dropped
    );
}

#[test]
fn a_genbank_origin_block_pastes_its_bases_and_not_its_position_numbers() {
    let text = "LOCUS       x   24 bp\nFEATURES\nORIGIN\n\
                \x20       1 gaattcgcgg ccgcttctag\n\
                \x20      21 agcg\n//\n";
    let r = sanitise_paste(text);
    assert_eq!(r.bases, "gaattcgcggccgcttctagagcg");
    assert!(r.rejected.is_empty(), "{:?}", r.rejected);
    assert!(
        r.dropped.iter().any(|d| d.contains("ORIGIN")),
        "{:?}",
        r.dropped
    );
    assert!(
        r.dropped.iter().any(|d| d.contains("features")),
        "a whole record is only pasted as bases, and that is said: {:?}",
        r.dropped
    );
}

#[test]
fn digits_in_plain_text_are_rejected_rather_than_stripped() {
    // The asymmetry is the whole point of doing structure before characters:
    // stripping digits is safe only where the structure says they are
    // coordinates. `ACGT1234` is junk or a truncated identifier.
    let r = sanitise_paste("ACGT1234");
    assert_eq!(r.rejected.len(), 4);
    assert!(r.rejected.iter().any(|x| x.ch == '1'));
}

#[test]
fn two_fasta_records_are_refused_rather_than_concatenated() {
    let r = sanitise_paste(">a\nACGT\n>b\nTTTT\n");
    assert!(r.refused.unwrap().contains("2 FASTA records"));
    assert!(r.bases.is_empty());
}

#[test]
fn alignment_gaps_and_smart_quotes_need_consent() {
    let r = sanitise_paste("ACGT--ACGT\u{2019}");
    assert!(!r.rejected.is_empty());
    let dash = r.rejected.iter().find(|x| x.ch == '-').unwrap();
    assert_eq!(dash.count, 2);
    assert_eq!(dash.first_at, 5);
    assert!(r.consent_question().contains("U+2019"));
    // No transliteration: nothing was folded into a hyphen or an A.
    assert_eq!(r.bases, "ACGTACGT");
}

#[test]
fn invisible_characters_are_dropped_and_counted() {
    // An NBSP from a vendor spec sheet already cost this project once.
    let r = sanitise_paste("ACG\u{00A0}T\u{FEFF}ACGT");
    assert_eq!(r.bases, "ACGTACGT");
    assert!(r.rejected.is_empty());
    assert!(r.dropped.iter().any(|d| d.contains("2 invisible")));
}

#[test]
fn a_paste_of_rna_says_so_rather_than_rewriting_it() {
    let r = sanitise_paste("ACGUACGU");
    assert_eq!(r.bases, "ACGUACGU", "U is stored, not turned into T");
    assert_eq!(r.uracil, 2);
    assert!(r.summary().contains("RNA"));
}

#[test]
fn one_paste_is_one_operation() {
    let mut d = doc("AAAA", false);
    let mut e = SeqEdit::new();
    e.caret = 2;
    assert!(!e.paste(&mut d, ">x\nCCCC\nGGGG\n"));
    assert_eq!(seq_of(&d), "AACCCCGGGGAA");
    assert_eq!(d.log.path().len(), 1);
    d.undo().unwrap();
    assert_eq!(seq_of(&d), "AAAA");
}

#[test]
fn a_paste_needing_consent_changes_nothing_until_it_is_given() {
    let mut d = doc("AAAA", false);
    let mut e = SeqEdit::new();
    e.caret = 2;
    assert!(e.paste(&mut d, "CC-CC"), "the caller must ask");
    assert_eq!(seq_of(&d), "AAAA");
    assert!(!d.edited());

    let (report, target) = e.pending_paste.take().unwrap();
    e.insert_paste(&mut d, &report, target.unwrap());
    assert_eq!(seq_of(&d), "AACCCCAA");
}

#[test]
fn a_paste_that_sanitises_to_nothing_records_no_operation() {
    // `InsertAt { seq: "" }` is accepted by the engine and records a real
    // history entry for nothing at all.
    let mut d = doc("AAAA", false);
    let mut e = SeqEdit::new();
    e.paste(&mut d, "   \n\n  ");
    assert!(!d.edited());
}

// ---------------------------------------------------------------------------
// Where editing is refused
// ---------------------------------------------------------------------------

#[test]
fn an_annotation_track_refuses_editing_and_says_why() {
    let mut m = mol("", false);
    feature(&mut m, "orphan", 100, 400);
    assert!(m.is_annotation_track());

    let e = Editability::of(&m);
    assert!(!e.is_editable());
    let why = e.refusal().unwrap();
    assert!(why.contains("annotation track"), "{why}");
    assert!(why.contains("1 feature"), "{why}");
    // The engine would refuse too, but with "feature 0 'orphan' segment 0
    // start: 101 is past the 1 bp molecule" — the gate reporting a symptom of a
    // question that should never have been asked.
    assert!(!why.contains("past the"), "{why}");
}

#[test]
fn an_annotation_only_genbank_gets_its_own_sentence() {
    let mut m = mol("", false);
    m.declared_len = Some(2_944_528);
    feature(&mut m, "gene", 100, 400);
    let e = Editability::of(&m);
    assert!(!e.is_editable());
    let why = e.refusal().unwrap();
    assert!(why.contains("2,944,528"), "{why}");
    assert!(why.contains("carries none"), "{why}");
}

#[test]
fn a_genuinely_empty_document_is_editable() {
    // Refusing this would mean the editor cannot start a sequence from nothing.
    let m = mol("", false);
    assert!(Editability::of(&m).is_editable());

    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();
    e.type_text(&mut d, "ACGT", T0);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ACGT");
}

#[test]
fn a_document_that_arrived_with_bad_coordinates_stays_editable() {
    // The obvious wrong move. `OpLog::apply` compares problem counts per kind
    // and refuses only increases, precisely so a file from a bad importer stays
    // editable — gating on `is_valid()` would lock the user out of exactly the
    // files they most need to repair.
    let mut m = mol("ACGTACGT", false);
    feature(&mut m, "bad", 1, 900);
    assert!(!m.is_valid());
    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();
    e.caret = 4;
    e.type_text(&mut d, "T", T0);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ACGTTACGT");
}

// ---------------------------------------------------------------------------
// GenBank, end to end
// ---------------------------------------------------------------------------

#[test]
fn a_genbank_file_with_bases_accepts_an_insertion() {
    // Before `oplog::apply` learned to retire a stale LOCUS length, this was
    // refused — on every GenBank file, at every size, with a message about the
    // file declaring a different number of bases. The editor could do point
    // mutations and nothing else on the project's own default save format.
    let gb = "LOCUS       x                       12 bp    DNA     linear   SYN 01-JAN-2026\n\
              FEATURES             Location/Qualifiers\n\
              \x20    gene            5..8\n\
              ORIGIN\n\
              \x20       1 acgtacgtacgt\n//\n";
    let mut d = Document::from_bytes(gb.as_bytes(), "x.gb".into(), None).unwrap();
    assert_eq!(d.molecule().len(), 12);

    let mut e = SeqEdit::new();
    e.caret = 0;
    e.type_text(&mut d, "TTT", T0);
    e.commit(&mut d);

    assert_eq!(seq_of(&d), "TTTacgtacgtacgt");
    assert_eq!(coords(&d, "gene"), (8, 11));
    assert!(e.notice.is_none(), "no refusal to report: {:?}", e.notice);
    // ...and the round trip still declares the right length.
    let out = pl_fileio::genbank::write(d.molecule(), "x", (1, 1, 2026));
    assert!(out.contains("15 bp"), "{}", &out[..80.min(out.len())]);
}

// ---------------------------------------------------------------------------
// The readout
// ---------------------------------------------------------------------------

#[test]
fn the_readout_prints_the_number_the_operation_will_carry() {
    let m = mol(&"A".repeat(2686), false);
    let mut e = SeqEdit::new();
    e.caret = 2450;
    let r = e.readout(&m);
    assert!(r.starts_with("insert at 2,451"), "{r}");
    assert!(r.contains("between 2,450 and 2,451"), "{r}");

    e.caret = 2686;
    assert!(e.readout(&m).contains("after the last base (2,686)"));
    e.caret = 0;
    assert!(e.readout(&m).contains("before base 1"));
}

#[test]
fn the_readout_names_both_sides_of_a_circular_origin() {
    // Caret 0 and caret n are two positions in the text naming one gap on the
    // molecule, and they produce genuinely different files: inserting at 1
    // shifts every feature coordinate, inserting at n+1 moves nothing. The
    // consequence of choosing a side is printed before the user commits.
    let m = mol(&"A".repeat(2686), true);
    let mut e = SeqEdit::new();
    e.caret = 0;
    let a = e.readout(&m);
    e.caret = 2686;
    let b = e.readout(&m);
    assert!(a.contains("at the origin") && b.contains("at the origin"));
    assert!(a.contains("coordinates shift"), "{a}");
    assert!(b.contains("numbering unchanged"), "{b}");
}

#[test]
fn the_readout_never_prints_a_bare_position_while_a_selection_is_live() {
    let m = mol(&"A".repeat(5386), true);
    let mut e = SeqEdit::new();
    e.caret = 40;
    e.sel = Some(Selection {
        anchor: 4960,
        head: 40,
        through_origin: true,
    });
    let r = e.readout(&m);
    // Bases 4,961..=5,386 and 1..=40: 426 + 40 = 466, which is `n - (hi - lo)`
    // and not the 4,920 the caret difference would report.
    assert_eq!(r, "4,961..40 · 466 bp · crosses the origin");
    assert!(!r.starts_with("insert at"));

    e.sel = Some(Selection {
        anchor: 0,
        head: 5386,
        through_origin: false,
    });
    assert_eq!(e.readout(&m), "1..5,386 · 5,386 bp · whole molecule");
}

// ---------------------------------------------------------------------------
// Rendering the rows
// ---------------------------------------------------------------------------

#[test]
fn one_byte_renders_as_exactly_one_cell() {
    // The caret indexes `Molecule::seq`, which is a `Vec<u8>` documented "not
    // guaranteed to be valid IUPAC"; the GenBank reader filters only whitespace
    // and digits, so odd bytes reach it from real files.
    // `String::from_utf8_lossy` is not length-preserving in either direction —
    // b"AC\xF0\x90\x80GT" is 7 bytes and renders as 5 chars — so a column index
    // taken through a lossy render drifts from the base offset, silently, only
    // on the files that are already unusual.
    let m = Molecule {
        seq: b"AC\xF0\x90\x80GT".to_vec(),
        ..Default::default()
    };
    let e = SeqEdit::new();
    let mut out = String::new();
    e.row_text(&m, 0, 7, &mut out);
    assert_eq!(out.chars().count(), 7, "one cell per byte, always");
    assert_eq!(out, "AC???GT");
    assert_eq!(
        String::from_utf8_lossy(&m.seq).chars().count(),
        5,
        "which the lossy render, for contrast, does not do"
    );
}

#[test]
fn the_caret_space_is_built_from_the_bases_present_not_the_declared_length() {
    // On a `sequence_absent()` file `span()` returns the declared 2,944,528
    // while `seq` is empty; a caret space built from `span()` would let a click
    // at column 40 of row 900 produce an insertion at 54,001 on a molecule with
    // no bases at all.
    let mut m = mol("", false);
    m.declared_len = Some(2_944_528);
    let e = SeqEdit::new();
    assert_eq!(e.effective_len(&m), 0);
    assert_eq!(m.span(), 2_944_528);
}

#[test]
fn the_row_width_is_measured_not_assumed() {
    // Sixty cells at 11.5 pt monospace is about 414 px, and the panel this
    // view lives in offers roughly 380 minus a 62 px coordinate gutter. The
    // read-only view overflowed and egui clipped it, which cost nothing but a
    // truncated ruler; an editor cannot let a base sit outside the panel,
    // because a base you cannot see is a base you cannot click.
    let advance = 6.9;
    assert_eq!(fit_per_row(380.0 - 62.0 - 14.0, advance), 40);
    assert_eq!(fit_per_row(1000.0, advance), MAX_PER_ROW, "capped at 60");
    assert_eq!(fit_per_row(0.0, advance), 10, "and never zero");
    // Always a multiple of ten: a ruler that counts 47 to a row is useless.
    for w in [100.0, 220.0, 337.0, 413.9] {
        assert_eq!(fit_per_row(w, advance) % 10, 0, "width {w}");
    }
}

#[test]
fn arrowing_down_a_row_and_clicking_use_the_same_row_width() {
    // The renderer, the hit-test and Up/Down read one value. Two copies means
    // they disagree the day the panel is resized.
    let mut e = SeqEdit::new();
    assert_eq!(
        e.per_row(),
        MAX_PER_ROW,
        "before the first frame measures it"
    );
    e.set_per_row(40);
    assert_eq!(e.per_row(), 40);

    let mut d = doc(&"A".repeat(200), false);
    e.caret = 0;
    e.step(&mut d, e.per_row() as i64, false);
    assert_eq!(e.caret, 40, "one visual row down");
}

// ---------------------------------------------------------------------------
// Walking a selection across the origin
//
// These are the gestures the whole circular editing surface is reached by, and
// every one of them produced the COMPLEMENT of the intended arc: `canonical`
// (then named `normalised`) clears the wrap bit whenever the pair happens to be
// expressible without wrapping, and its output was stored back into `sel`, so
// the next keypress read a selection that had forgotten which way the user was
// travelling.
// ---------------------------------------------------------------------------

/// Which BASES a selection names, 1-based, in reading order.
fn selected(e: &SeqEdit, m: &Molecule) -> Vec<u64> {
    let n = m.len();
    let Some(s) = e.sel.map(|s| s.canonical(n, m.topology.is_circular())) else {
        return Vec::new();
    };
    if s.through_origin {
        (s.hi() + 1..=n).chain(1..=s.lo()).collect()
    } else {
        (s.lo() + 1..=s.hi()).collect()
    }
}

#[test]
fn shift_right_past_the_end_of_a_circle_adds_the_first_base() {
    // 12 bp circle, caret at gap 10. Four Shift+Rights must select 11, 12, 1, 2
    // and nothing else. Measured before: [11] -> [11,12] -> [11,12] (the third
    // press wasted, the caret jumping 12 -> 0 while the selection collapsed to
    // the non-wrapping form) -> [2,3,4,5,6,7,8,9,10] — nine bases on the far
    // side of the plasmid, and one Backspace there takes all nine.
    let m = mol("ABCDEFGHIJKL", true);
    let mut d = Document::of_molecule(m.clone());
    let mut e = SeqEdit::new();
    e.caret = 10;

    e.step(&mut d, 1, true);
    assert_eq!(selected(&e, &m), vec![11]);
    e.step(&mut d, 1, true);
    assert_eq!(selected(&e, &m), vec![11, 12]);
    e.step(&mut d, 1, true);
    assert_eq!(selected(&e, &m), vec![11, 12, 1], "across the origin");
    e.step(&mut d, 1, true);
    assert_eq!(selected(&e, &m), vec![11, 12, 1, 2]);
    assert!(
        e.sel.unwrap().through_origin,
        "and it knows that it wrapped"
    );

    // And the deletion that follows takes exactly those four.
    e.backspace(&mut d, T0);
    assert_eq!(seq_of(&d), "CDEFGHIJ");
}

#[test]
fn shift_right_at_the_end_with_nothing_selected_selects_base_one() {
    // The documented gesture. It selected NOTHING: the pair (12, 0) with the
    // wrap bit set canonicalises to the empty selection at 0.
    let m = mol("ABCDEFGHIJKL", true);
    let mut d = Document::of_molecule(m.clone());
    let mut e = SeqEdit::new();
    e.caret = 12;
    e.step(&mut d, 1, true);
    assert_eq!(selected(&e, &m), vec![1]);
}

#[test]
fn shift_left_at_the_origin_takes_the_last_base_and_gives_it_back() {
    // Leftward, and then walked back the other way. Measured before: the first
    // press selected nothing and the second selected bases 1..11 — eleven of
    // twelve bases, one Backspace from losing the plasmid.
    let m = mol("ABCDEFGHIJKL", true);
    let mut d = Document::of_molecule(m.clone());
    let mut e = SeqEdit::new();
    e.caret = 0;

    e.step(&mut d, -1, true);
    assert_eq!(selected(&e, &m), vec![12]);
    e.step(&mut d, -1, true);
    assert_eq!(selected(&e, &m), vec![11, 12]);
    // Shrinking from the moving end, back over the origin. The wrap bit has to
    // come off again here, and a rule that only ever sets it leaves the head
    // stuck at gap 0 forever.
    e.step(&mut d, 1, true);
    assert_eq!(selected(&e, &m), vec![12]);
    e.step(&mut d, 1, true);
    assert!(selected(&e, &m).is_empty());
    e.step(&mut d, 1, true);
    assert_eq!(selected(&e, &m), vec![1], "and on out the other side");
}

#[test]
fn arrow_keys_collapse_a_wrapping_selection_at_its_own_ends() {
    // For an arc that wraps, the 5-prime end is the gap `hi` and the 3-prime
    // end is the gap `lo` — the other way round from an ordinary selection.
    // Left put the caret at the 3-prime end and Right at the 5-prime one, so
    // select-across-the-origin, press Left, type, and the bases landed at the
    // far end of what had been highlighted.
    let mut d = doc("ABCDEFGHIJKL", true);
    let mut e = SeqEdit::new();
    let arc = Selection {
        anchor: 10,
        head: 2,
        through_origin: true,
    };

    e.sel = Some(arc);
    e.caret = 2;
    e.step(&mut d, -1, false);
    assert_eq!(e.caret, 10, "Left goes to the end that base 11 begins at");

    e.sel = Some(arc);
    e.caret = 2;
    e.step(&mut d, 1, false);
    assert_eq!(e.caret, 2, "Right goes to the end that base 2 finishes at");

    // The control: an ordinary selection is unchanged by all of this.
    let plain = Selection {
        anchor: 3,
        head: 7,
        through_origin: false,
    };
    e.sel = Some(plain);
    e.caret = 7;
    e.step(&mut d, -1, false);
    assert_eq!(e.caret, 3);
    e.sel = Some(plain);
    e.caret = 3;
    e.step(&mut d, 1, false);
    assert_eq!(e.caret, 7);
}

// ---------------------------------------------------------------------------
// A selection raised while a run is open
// ---------------------------------------------------------------------------

#[test]
fn typing_over_a_selection_made_during_a_run_replaces_it() {
    // `type_text` extended an open Insert run without consulting `sel`, and the
    // pointer can raise a selection while one is open. Measured: type "gg" at
    // caret 0, drag out bases 9..12, type "T" — "ggTAAAACCCCGGGGTTTT". The T
    // went in after the run's own "gg", ten bases from the highlight, and the
    // highlighted GGGG was still there. This is the only path in this surface
    // that put bases somewhere the user was not pointing.
    let mut d = doc("AAAACCCCGGGGTTTT", false);
    let mut e = SeqEdit::new();
    e.caret = 0;
    e.type_text(&mut d, "gg", T0);
    assert!(e.run().is_some(), "the premise: a run is open");

    // Assigned exactly as the drag handler used to, in the coordinates of the
    // view the user is looking at — "ggAAAACCCCGGGGTTTT", whose GGGG is bases
    // 11..14. The point of the test is that `type_text` must consult this
    // however it got there.
    e.sel = Some(Selection {
        anchor: 10,
        head: 14,
        through_origin: false,
    });
    e.caret = 14;
    e.type_text(&mut d, "N", T0 + 0.1);
    e.commit(&mut d);
    assert_eq!(seq_of(&d), "ggAAAACCCCNTTTT");
}

#[test]
fn a_pointer_selection_settles_the_run_it_interrupts() {
    // `set_selection` exists so that nothing outside this module assigns `sel`
    // and `caret` behind the run's back. Committing first is what makes the
    // typed bases a separate, undoable thing from whatever is done to the
    // selection next.
    let mut d = doc("AAAACCCCGGGGTTTT", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    e.type_text(&mut d, "nnn", T0);
    assert_eq!(d.log.path().len(), 0, "still open");

    e.set_selection(&mut d, Selection::point(2), 2);
    assert!(e.run().is_none());
    assert_eq!(d.log.path().len(), 1);
    assert_eq!(seq_of(&d), "AAAAnnnCCCCGGGGTTTT");
}

// ---------------------------------------------------------------------------
// What an edit took with it
// ---------------------------------------------------------------------------

#[test]
fn a_held_backspace_through_a_feature_says_the_feature_went() {
    // The report was made on the `apply_gesture` path only, so the two
    // gestures that go through `commit` instead — a held Backspace, and typing
    // over a selection — removed a feature and said nothing whatever. Both are
    // reachable from the keyboard with no pointer at all.
    let mut m = mol("AAAACCCCGGGGTTTT", false);
    feature(&mut m, "ori", 5, 8);
    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();
    e.caret = 8;
    for i in 0..4 {
        e.backspace(&mut d, T0 + 0.1 * i as f64);
    }
    assert_eq!(d.log.path().len(), 0, "the premise: one open run, not four");
    e.commit(&mut d);

    assert_eq!(seq_of(&d), "AAAAGGGGTTTT");
    assert!(names(&d).is_empty(), "the premise: it really is gone");
    let said = e.notice.clone().unwrap_or_default();
    assert!(said.contains("ori"), "said {said:?}");
}

#[test]
fn typing_over_a_feature_says_it_went() {
    let mut m = mol("AAAACCCCGGGGTTTT", false);
    feature(&mut m, "AmpR", 5, 8);
    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();
    e.sel = Some(Selection {
        anchor: 4,
        head: 8,
        through_origin: false,
    });
    e.type_text(&mut d, "n", T0);
    e.commit(&mut d);

    assert_eq!(seq_of(&d), "AAAAnGGGGTTTT");
    assert!(names(&d).is_empty());
    let said = e.notice.clone().unwrap_or_default();
    assert!(said.contains("AmpR"), "said {said:?}");
}

// ---------------------------------------------------------------------------
// Paste
// ---------------------------------------------------------------------------

#[test]
fn a_line_that_merely_starts_with_origin_is_not_a_genbank_block() {
    // `starts_with("ORIGIN")` matched "ORIGINAL CLONE, 2019" in a pasted note,
    // and everything ABOVE the match is discarded as header while digits below
    // it are silently stripped. Measured: this paste kept ten bases, dropped
    // the other ten, and raised no dialog because `rejected` was empty.
    let r = sanitise_paste("GAATTCACGT\nORIGINAL CLONE, 2019\nGGATCCACGT\n");
    assert!(
        r.bases.contains("GAATTC"),
        "the bases above the note survived: {:?}",
        r.bases
    );
    assert!(r.bases.contains("GGATCC"), "{:?}", r.bases);

    // The control: a real ORIGIN block, with the corroboration GenBank always
    // carries, is still read as one.
    let gb = "LOCUS       x   12 bp\nORIGIN\n        1 gaattcacgt aa\n//\n";
    let r = sanitise_paste(gb);
    assert_eq!(r.bases, "gaattcacgtaa");
    assert!(
        r.dropped.iter().any(|d| d.contains("position number")),
        "{:?}",
        r.dropped
    );

    // A bare `ORIGIN` line terminated by `//` is enough on its own: that is
    // what a partial record copied out of a viewer looks like.
    let r = sanitise_paste("ORIGIN\n        1 acgtacgt\n//\n");
    assert_eq!(r.bases, "acgtacgt");
}

#[test]
fn a_genbank_paste_counts_the_bases_it_dropped_above_the_origin_line() {
    // Whatever the structure test decides, the accounting line has to cover
    // bases and not only digits.
    let gb = "LOCUS       x   8 bp\nFEATURES\n    gene  1..4 /note=gaattc\n\
              ORIGIN\n        1 acgtacgt\n//\n";
    let r = sanitise_paste(gb);
    assert_eq!(r.bases, "acgtacgt");
    assert!(
        r.dropped
            .iter()
            .any(|d| d.contains("above the ORIGIN line")),
        "{:?}",
        r.dropped
    );
}

#[test]
fn prose_pasted_from_a_document_is_confirmed_rather_than_inserted() {
    // Sixteen of the twenty-six letters are IUPAC codes, so ordinary English
    // that happens to avoid E, F, I, J, L, O, P, Q, X and Z sanitises to a
    // clean-looking paste with nothing rejected and no dialog: "that was a bad
    // hack" went in as the 15 bases "thatwasabadhack", 47% of them ambiguity
    // codes, a composition no real DNA has.
    let mut d = doc(&"ACGT".repeat(750), false);
    let mut e = SeqEdit::new();
    assert!(
        e.paste(&mut d, "that was a bad hack"),
        "the caller must ask first"
    );
    assert!(!d.edited(), "and nothing has gone in yet");
    let (report, _) = e.pending_paste.clone().unwrap();
    assert_eq!(report.ambiguous, 7, "h, w, s, b, d, h, k");
    assert!(
        report.consent_question().contains("ambiguity codes"),
        "{}",
        report.consent_question()
    );

    // The controls. Real sequence, and sequence with a few Ns in it, are not
    // interrupted — and an RNA paste is 25% U and must not be either.
    for clean in [
        "GAATTCACGTACGTGGATCCAAAA",
        "GAATTCACGTNNNACGTGGATCCA",
        "ACGUACGUACGUACGUACGUACGU",
    ] {
        let r = sanitise_paste(clean);
        assert!(r.suspect.is_none(), "{clean}: {:?}", r.suspect);
    }
    // And a short degenerate oligo is below the floor: the user can see it.
    assert!(sanitise_paste("NNKNNK").suspect.is_none());
}

#[test]
fn an_ambiguity_heavy_paste_says_so_even_when_it_is_not_stopped() {
    // Below the floor there is no dialog, but the accounting line still may not
    // read like a clean paste.
    let r = sanitise_paste("acgtwn");
    assert!(r.suspect.is_none());
    assert!(
        r.summary().contains("ambiguity codes"),
        "summary was {:?}",
        r.summary()
    );
}

#[test]
fn many_distinct_rejected_characters_are_capped_but_still_counted() {
    // The tally was a `Vec` scanned linearly per character, which is
    // O(characters x kinds): 605 kB of CJK text off a web page has 20,992
    // distinct characters and took 635 ms; 4.24 MB took 4.55 s, on the UI
    // thread, inside the frame that handled Ctrl+V, with no way to cancel.
    let text: String = (0x4E00u32..0x4E00 + 500)
        .filter_map(char::from_u32)
        .collect();
    let r = sanitise_paste(&text);
    assert_eq!(r.rejected_kinds, 500, "all of them are counted");
    assert_eq!(r.rejected_total, 500);
    assert_eq!(r.rejected.len(), MAX_REJECTED_KINDS, "not all are kept");
    // Kept in the order the user would recognise, which a hash map does not
    // have: the dialog quotes each one's first position.
    assert!(r.rejected.windows(2).all(|w| w[0].first_at < w[1].first_at));
    assert!(r.consent_question().contains("more kinds"));
}

#[test]
fn a_confirmed_paste_lands_where_the_question_was_asked() {
    // The dialog is not modal and `Button` never takes keyboard focus, so the
    // caret can move between the question and the answer. The target was
    // captured with the report and then thrown away, and `insert_paste` read
    // `self.sel` at confirm time instead: measured, one caret move was enough
    // to insert eight bases at position 1 while leaving the CCCC the dialog had
    // been about exactly where it was.
    let mut d = doc("AAAACCCCGGGGTTTT", false);
    let mut e = SeqEdit::new();
    e.sel = Some(Selection {
        anchor: 4,
        head: 8,
        through_origin: false,
    });
    e.caret = 8;
    assert!(e.paste(&mut d, "ACGT?ACGT"), "the '?' needs consent");
    let (report, target) = e.pending_paste.take().unwrap();

    // The user clicks in the grid before answering.
    e.place(&mut d, 0, false);
    e.insert_paste(&mut d, &report, target.unwrap());
    assert_eq!(seq_of(&d), "AAAAACGTACGTGGGGTTTT");
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[test]
fn a_refusal_survives_the_keystrokes_that_follow_it() {
    // `egui` delivers ordinary typing as one `Event::Text` per character, so
    // the guard that kept a refusal alive within one event never fired in
    // practice. Typing "ACGZTACG" at human speed: the message naming 'Z'
    // appeared and the next 'T' erased it. Eight keys pressed, seven bases in,
    // and nothing on screen said which one did not make it.
    let mut d = doc("ACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    let mut t = T0;
    for c in "ACGZTACG".chars() {
        e.type_text(&mut d, &c.to_string(), t);
        t += 0.05;
    }
    e.commit(&mut d);
    assert_eq!(
        seq_of(&d),
        "ACGTACGTACG",
        "the premise: the Z did not go in"
    );
    let said = e.notice.clone().unwrap_or_default();
    assert!(said.contains('Z'), "said {said:?}");

    // Backspace does not wipe it either.
    e.backspace(&mut d, t);
    assert!(e.notice.clone().unwrap_or_default().contains('Z'));

    // But it is not permanent: a later burst of clean typing clears it.
    e.type_text(&mut d, "A", t + SeqEdit::REJECT_STICKY + 1.0);
    assert_eq!(e.notice, None);
}

#[test]
fn copying_bytes_that_are_not_nucleotide_codes_does_not_transliterate_them() {
    // `*b as char` is a Latin-1 reinterpretation. The grid paints these seven
    // bytes as "AC???GT", one cell per byte and by design; the clipboard got
    // "AC\u{f0}\u{90}\u{80}GT", which is neither that nor what the file holds,
    // and pasting it back needs consent for three characters it invented.
    let m = Molecule {
        seq: b"AC\xF0\x90\x80GT".to_vec(),
        ..Default::default()
    };
    let mut e = SeqEdit::new();
    e.sel = Some(Selection {
        anchor: 0,
        head: 7,
        through_origin: false,
    });
    let (text, skipped) = e.copy(&m).unwrap();
    assert_eq!(text, "ACGT");
    assert_eq!(skipped, 3, "and the count is offered rather than hidden");
}

// ---------------------------------------------------------------------------
// Caret transport
// ---------------------------------------------------------------------------

#[test]
fn a_caret_before_an_edit_does_not_move_with_it() {
    // These three arms answered the same thing whatever the caret was, so an
    // operation the editor did not issue teleported the caret to the edit site:
    // `transport(10, DeleteRange { start: 50, len: 10 }, 100)` was 49.
    let del = OpKind::DeleteRange { start: 50, len: 10 };
    assert_eq!(transport(10, &del, 100), 10, "before it: unmoved");
    assert_eq!(transport(70, &del, 100), 60, "after it: shifted by 10");
    assert_eq!(transport(55, &del, 100), 49, "inside it: the near edge");
    assert_eq!(transport(49, &del, 100), 49, "the near edge itself");

    let ins = OpKind::InsertAt {
        at: 50,
        seq: "ACGT".into(),
    };
    assert_eq!(transport(10, &ins, 100), 10);
    assert_eq!(transport(70, &ins, 100), 74);
    assert_eq!(transport(49, &ins, 100), 53, "at the insertion point");

    let rep = OpKind::ReplaceRange {
        start: 50,
        len: 10,
        seq: "AC".into(),
    };
    assert_eq!(transport(10, &rep, 100), 10);
    assert_eq!(transport(70, &rep, 100), 62);
    assert_eq!(transport(55, &rep, 100), 51);
}

#[test]
fn an_arrow_key_after_typing_at_the_end_does_not_walk_the_caret_back() {
    // `step` read the molecule's length before settling the open run, so with
    // a run open every length it worked with was the committed one while the
    // caret was in the coordinates of what is on screen. Typing three bases at
    // the end of "ACGT" and pressing Right clamped `to` to `min(8, 4)`: the
    // caret jumped back four places, to where the typing had started.
    let mut d = doc("ACGT", false);
    let mut e = SeqEdit::new();
    e.caret = 4;
    e.type_text(&mut d, "AAA", T0);
    assert_eq!(e.caret, 7, "the premise: the caret is past what was typed");

    e.step(&mut d, 1, false);
    assert_eq!(e.caret, 7, "Right at the end of the molecule stays put");
    assert_eq!(seq_of(&d), "ACGTAAA");

    // Down a row, on a molecule with more than one, is the same arithmetic.
    let mut d = doc(&"A".repeat(100), false);
    let mut e = SeqEdit::new();
    e.set_per_row(40);
    e.caret = 100;
    e.type_text(&mut d, "GGGG", T0);
    e.step(&mut d, -40, false);
    assert_eq!(e.caret, 64, "one row up from gap 104");
}

// ---------------------------------------------------------------------------
// A selection that covers no bases is not a selection
// ---------------------------------------------------------------------------

#[test]
fn a_paste_after_a_wrapping_selection_is_shrunk_away_lands_at_the_caret() {
    // Shift+Left from gap 0 and then Shift+Right back is a user changing their
    // mind. It leaves `{anchor: 0, head: n, through_origin: true}`, the one
    // empty shape `canonical` relocates: its `hi >= n` collapse sets
    // `head = lo = 0` while the caret is at gap n. `target` returned it because
    // `sel` was `Some`, so Ctrl+V inserted BEFORE base 1 where typing the same
    // characters inserted after base n — every coordinate in the file shifted
    // by the length of the paste and the origin moved, with no message, while
    // the readout had just promised "numbering unchanged".
    let mut m = mol("ACGTACGTACGT", true);
    feature(&mut m, "AmpR", 5, 8);
    let mut d = Document::of_molecule(m);
    let mut e = SeqEdit::new();
    e.caret = 0;
    e.step(&mut d, -1, true);
    e.step(&mut d, 1, true);

    // The premise, stated so a change in the gesture cannot silently pass this.
    assert_eq!(e.caret, 12);
    let s = e.sel.expect("the gesture leaves a selection behind");
    assert_eq!(
        s,
        Selection {
            anchor: 0,
            head: 12,
            through_origin: true
        }
    );
    assert!(s.is_empty(12), "and it covers no bases");
    assert!(
        e.readout(d.molecule()).contains("numbering unchanged"),
        "{}",
        e.readout(d.molecule())
    );

    assert_eq!(
        e.target(&d),
        Selection::point(12),
        "an empty selection is not a selection; the caret is the target"
    );

    assert!(!e.paste(&mut d, "TTT"), "no consent needed for three T's");
    assert_eq!(seq_of(&d), "ACGTACGTACGTTTT");
    assert_eq!(coords(&d, "AmpR"), (5, 8), "nothing was renumbered");

    // And typing from the identical state produces the identical document,
    // which is the invariant that was broken.
    let mut m2 = mol("ACGTACGTACGT", true);
    feature(&mut m2, "AmpR", 5, 8);
    let mut d2 = Document::of_molecule(m2);
    let mut e2 = SeqEdit::new();
    e2.caret = 0;
    e2.step(&mut d2, -1, true);
    e2.step(&mut d2, 1, true);
    e2.type_text(&mut d2, "TTT", T0);
    e2.commit(&mut d2);
    assert_eq!(seq_of(&d2), seq_of(&d));
    assert_eq!(coords(&d2, "AmpR"), coords(&d, "AmpR"));
}

#[test]
fn a_non_empty_wrapping_selection_is_still_the_target() {
    // The guard above must not disarm the origin-crossing paste itself.
    let mut d = doc("ACGTACGTACGT", true);
    let mut e = SeqEdit::new();
    e.caret = 0;
    e.step(&mut d, -1, true);
    assert_eq!(selected(&e, d.molecule()), vec![12]);
    let t = e.target(&d);
    assert!(!t.is_empty(12), "one base is still a selection: {t:?}");
    assert!(!e.paste(&mut d, "GG"));
    assert_eq!(
        seq_of(&d),
        "ACGTACGTACGGG",
        "base 12 was replaced by the pasted bases, not appended after it"
    );
}

#[test]
fn two_genbank_records_are_refused_rather_than_silently_truncated() {
    // `genbank_origin_line` found the FIRST `ORIGIN` and the `take_while` in
    // `sanitise_paste` stopped at the FIRST `//`, so record 2 vanished whole:
    // its bases were counted by nothing, `rejected` stayed empty and `refused`
    // stayed `None`, so `needs_consent` was false and the paste went in with no
    // dialog. The notice was byte-identical to a single-record paste and said
    // "this is a whole GenBank record", singular.
    let rec = |name: &str, base: char| {
        format!(
            "LOCUS       {name}   12 bp\nFEATURES\nORIGIN\n\
             \x20       1 {0}\n//\n",
            base.to_string().repeat(12)
        )
    };
    let one = sanitise_paste(&rec("recA", 'a'));
    assert_eq!(one.bases, "aaaaaaaaaaaa", "the control still pastes");
    assert!(one.refused.is_none());

    let two = sanitise_paste(&format!("{}{}", rec("recA", 'a'), rec("recB", 'c')));
    let why = two.refused.expect("two records are refused");
    assert!(why.contains("2 GenBank records"), "{why}");
    assert!(
        two.bases.is_empty(),
        "and nothing is pasted: {:?}",
        two.bases
    );

    // Three behave the same, and so does the shape whose first `//` is missing
    // — there the `take_while` used to run on and turn record 2's header
    // letters into bases.
    let three = sanitise_paste(&format!(
        "{}{}{}",
        rec("recA", 'a'),
        rec("recB", 'c'),
        rec("recC", 'g')
    ));
    assert!(three.refused.unwrap().contains("3 GenBank records"));

    let unterminated = format!(
        "LOCUS       recA   12 bp\nFEATURES\nORIGIN\n\x20       1 aaaaaaaaaaaa\n{}",
        rec("recB", 'c')
    );
    let r = sanitise_paste(&unterminated);
    assert!(
        r.refused.is_some(),
        "one ORIGIN but two terminators is still two records: {:?}",
        r.bases
    );
    assert!(
        !r.bases.contains('c') && !r.bases.contains('L'),
        "no header letters became bases: {:?}",
        r.bases
    );
}

#[test]
fn backspace_at_base_one_points_down_the_view_and_counts_rows_correctly() {
    // The message said base `n` was "{n / per_row} rows away, off the top of
    // this view". Base `n` is on the LAST row, which is the bottom-most row of
    // the grid, so the direction was inverted in every case; the count was one
    // row too many whenever `per_row` divided `n`; and for a circle short
    // enough to fit on one row it read "0 rows away, off the top" for a base
    // plainly visible beside the caret.
    let say = |n: u64, per_row: u64| {
        let mut d = doc(&"A".repeat(n as usize), true);
        let mut e = SeqEdit::new();
        e.set_per_row(per_row);
        e.caret = 0;
        e.backspace(&mut d, T0);
        assert!(!d.edited(), "still a no-op");
        e.notice.clone().unwrap()
    };

    let short = say(12, 40);
    assert!(short.contains("base 12"), "{short}");
    assert!(
        short.contains("on this same row"),
        "a 12 bp circle at 40 per row fits on one row: {short}"
    );
    assert!(!short.contains("off the top"), "{short}");

    // `per_row` divides `n`: base 40 is the last cell of row 0, the caret's own
    // row. `n / per_row` said one row away.
    let exact = say(40, 40);
    assert!(exact.contains("on this same row"), "{exact}");

    let plasmid = say(5_386, 40);
    assert!(plasmid.contains("base 5,386"), "{plasmid}");
    assert!(
        plasmid.contains("134 rows below"),
        "(5386 - 1) / 40 = 134, below the caret, not above it: {plasmid}"
    );
    assert!(!plasmid.contains("off the top"), "{plasmid}");

    // Plural agreement, on the range where the old code printed "1 rows".
    let one_row = say(100, 60);
    assert!(one_row.contains("1 row below"), "{one_row}");
    assert!(!one_row.contains("1 rows"), "{one_row}");
}
