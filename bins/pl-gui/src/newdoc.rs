//! File ▸ New: a molecule out of bases somebody had in their hands, with
//! nothing on disk.
//!
//! Every other door into this program is a file. A gBlock arrives in an email as
//! bare bases, a synthesis vendor sends a plain sequence, a reviewer pastes
//! 300 bp into a message — and to look at any of that in Polylinker you had to
//! open a text editor, write a FASTA by hand and save it first.
//!
//! # It goes in through the file path anyway
//!
//! [`Draft::record`] renders GenBank and `main.rs` parses it straight back with
//! `Document::from_bytes`, exactly as the cut-and-religate panel adopts its
//! product. That is a deliberate detour, and its argument is written out at
//! `App::clone_panel`: the document then behaves like one that was opened from
//! disk, gets the same load report, the same digest worker and the same
//! unsaved-changes protection, and there is no second way to build a `Molecule`
//! to drift from the first. `Document::of_molecule` exists for the same job and
//! is `#[cfg(test)]`, which is where it belongs.
//!
//! GenBank and not FASTA, because **FASTA cannot say whether a molecule is
//! circular**. The topology is chosen here at creation, on purpose: it changes
//! every downstream answer this program gives — the digest, the origin-crossing
//! features, the gel — and a plasmid pasted as linear is wrong everywhere at
//! once and annoying to correct afterwards.
//!
//! # And the text is cleaned by the paste pipeline
//!
//! [`crate::seqedit::sanitise_paste`] is what decides which characters are
//! bases, and it is called here rather than reimplemented: it already folds
//! case, already knows U, already reads a FASTA header, an `ORIGIN` block and a
//! numbered listing as structure, and already reports every character it
//! removed. A second alphabet in this crate is a second alphabet to get wrong.
//!
//! What differs is the ANSWER to a character that is not a base. A paste into an
//! open document offers to insert the rest and discard it, because the user is
//! looking at a molecule and one Ctrl+Z puts it back. Here nothing exists yet
//! and the text is still in the box, so it is refused with the character named
//! and its position quoted — see [`crate::seqedit::PasteReport::rejected_question`].

use crate::seqedit::{sanitise_paste, PasteReport};

/// The New dialog: what is typed in it, and whether it is up.
///
/// It is one field on `App` rather than an `Option`, so that Cancel and Escape
/// KEEP the text. A 20 kb gBlock pasted into the box and lost to a stray Escape
/// is the kind of small cruelty nobody reports and everybody remembers. Create
/// clears it, because after a document exists the previous molecule's bases are
/// no longer an offer to resume.
#[derive(Default)]
pub struct New {
    /// Whether the dialog is on screen.
    pub open: bool,
    pub name: String,
    pub text: String,
    pub circular: bool,
    /// The last BASES [`New::draft`] was asked about, and its answer.
    ///
    /// Memoised because this is read inside the paint pass of a modal that
    /// stays up while somebody types into it, and `sanitise_paste` allocates a
    /// `HashMap`, a copy of the body and a `String` of bases every time it runs.
    /// A `String` comparison is a length test and a `memcmp`; a re-sanitise of a
    /// 20 kb insert is neither.
    ///
    /// **THE KEY IS THE TEXT AND NOT THE NAME**, which it was until this was
    /// measured. The name cannot change one base — [`Draft::of`] uses it for
    /// `title` and `locus` and for nothing else — so keying on it meant every
    /// character typed into the Name box re-sanitised the whole paste. On the
    /// 10 Mb case in `zzz`-style timing that is 43 ms per keystroke against a
    /// 12 ms frame, so naming a large gBlock ran at about 18 fps and the memo
    /// was defeated by the one box it was not thinking about. [`New::draft`]
    /// refreshes the two name-derived fields in place instead, which is
    /// `locus_name` over a name somebody typed by hand.
    cached: Option<(String, Draft)>,
    /// How many times [`New::draft`] has missed and re-read the bases.
    ///
    /// A COUNTER AND NOT A TIMING ASSERTION. What went wrong was a cache miss,
    /// and a cache miss is a fact — "this ran twice" — while "this was fast" is
    /// a property of whichever machine the suite is on and passes on a quick one
    /// whatever the code does. On `New` rather than in a `static`, so two tests
    /// running at once cannot read each other's arithmetic.
    #[cfg(test)]
    reads: usize,
}

/// What the text in the box would become, and everything it would not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// The document's title: what the tab strip and the window title say.
    ///
    /// Never empty — see [`Draft::of`].
    pub title: String,
    /// As much of `title` as GenBank's LOCUS field can hold.
    ///
    /// Shown when it differs, because it is what the molecule will be called
    /// everywhere the molecule names itself: the map's caption and every figure
    /// exported from it. Silently renaming somebody's construct is the class of
    /// thing this program refuses to do.
    pub locus: String,
    /// The bases, and an account of every character that is not one.
    pub report: PasteReport,
    /// Why nothing can be created from this yet.
    ///
    /// `None` and no bases means the box is simply empty — there is nothing
    /// wrong to say, and a dialog that opens already complaining is a dialog
    /// people learn to ignore.
    pub refusal: Option<String>,
}

/// What a document with no name is called.
///
/// Something, rather than nothing: the title is the tab's caption and the
/// window's title, and a tab captioned with the empty string is a tab you
/// cannot tell from its neighbour. `genbank::locus_name` has its own fallback
/// for an unusable name and it is `sequence`, which describes a file; this
/// describes a document somebody has not named yet.
pub const UNTITLED: &str = "untitled";

impl Draft {
    /// What the Name box makes of itself: the document's title, and the LOCUS
    /// name GenBank will really carry.
    ///
    /// Split out of [`Draft::of`] so [`New::draft`] can refresh these two
    /// without touching the bases — see the memo's own doc for the keystroke
    /// that made that matter.
    fn names(name: &str) -> (String, String) {
        let title = match name.trim() {
            "" => UNTITLED.to_string(),
            n => n.to_string(),
        };
        let locus = pl_fileio::genbank::locus_name(&title);
        (title, locus)
    }

    /// Read the box.
    pub fn of(name: &str, text: &str) -> Draft {
        let (title, locus) = Draft::names(name);
        // An empty box is not an error. `sanitise_paste` is still not called on
        // it — it would return an empty report and a "joined 0 lines" that says
        // nothing — and the dialog disables Create on `report.bases`.
        if text.trim().is_empty() {
            return Draft {
                title,
                locus,
                report: PasteReport::default(),
                refusal: None,
            };
        }
        let report = sanitise_paste(text);
        let refusal = if let Some(r) = &report.refused {
            // Two records concatenated. `sanitise_paste` refuses this because
            // joining them fabricates a chimera, and that reason does not soften
            // because the chimera would be a new document rather than an edit.
            Some(r.clone())
        } else if let Some(q) = report.rejected_question() {
            Some(q)
        } else if report.bases.is_empty() {
            // Reachable, and not the same as an empty box: a FASTA header with
            // nothing under it, or a line of punctuation. Say what WAS found, so
            // "there are no bases in this" is checkable rather than flat
            // contradiction of a box the user can see has text in it.
            let mut why = "There are no bases in this.".to_string();
            if !report.dropped.is_empty() {
                why.push_str(&format!("\nAll of it was {}.", report.dropped.join(", ")));
            }
            Some(why)
        } else {
            None
        };
        Draft {
            title,
            locus,
            report,
            refusal,
        }
    }

    /// May a document be made from this?
    pub fn creatable(&self) -> bool {
        self.refusal.is_none() && !self.report.bases.is_empty()
    }

    /// The molecule, as a GenBank record for the loader to read back.
    ///
    /// `date` is passed in rather than read, so the bytes are a pure function of
    /// the dialog and a test can assert on them.
    pub fn record(&self, circular: bool, date: (u32, usize, i32)) -> String {
        let mol = pl_core::Molecule {
            // Set as well as passed below because a `Molecule` that names itself
            // one thing while its own LOCUS line names another is a trap for
            // whoever writes the next path out of here. `write_reporting` takes
            // the title and sanitises it, so the file is what decides — this is
            // belt and braces, not a second answer.
            name: self.title.clone(),
            seq: self.report.bases.as_bytes().to_vec(),
            topology: if circular {
                pl_core::Topology::Circular
            } else {
                pl_core::Topology::Linear
            },
            // `double_stranded` is deliberately left `None`. A synthesised
            // fragment is double-stranded far more often than not, and writing
            // `ds-` here would be this program asserting something nobody told
            // it — `Molecule::double_stranded`'s own doc calls unknown "a real
            // third state and callers should say so".
            ..Default::default()
        };
        pl_fileio::genbank::write_reporting(&mol, &self.title, date).0
    }
}

impl New {
    /// Put the dialog up, with the last text still in it.
    pub fn show(&mut self) {
        self.open = true;
    }

    /// Take the dialog down, keeping what is typed.
    pub fn hide(&mut self) {
        self.open = false;
    }

    /// Take it down and empty it, which is what a successful Create does.
    pub fn done(&mut self) {
        self.open = false;
        self.name.clear();
        self.text.clear();
        self.circular = false;
        self.cached = None;
    }

    /// What the box currently says, with the bases read at most once per change
    /// to them.
    pub fn draft(&mut self) -> &Draft {
        let stale = match &self.cached {
            Some((t, _)) => t != &self.text,
            None => true,
        };
        if stale {
            #[cfg(test)]
            {
                self.reads += 1;
            }
            let d = Draft::of(&self.name, &self.text);
            self.cached = Some((self.text.clone(), d));
        } else {
            // The bases stand; only the two fields the Name box owns are
            // rebuilt. Unconditionally rather than behind a comparison, because
            // `names` is `locus_name` over something typed by hand and the
            // comparison would cost about what it saves.
            let d = &mut self.cached.as_mut().expect("not stale, so present").1;
            (d.title, d.locus) = Draft::names(&self.name);
        }
        // `expect` rather than a restructure: both branches leave it assigned.
        &self.cached.as_ref().expect("just computed").1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything the loader would make of a draft, without going near `App`.
    fn made(name: &str, text: &str, circular: bool) -> (pl_core::Molecule, String) {
        let d = Draft::of(name, text);
        assert!(d.creatable(), "refused: {:?}", d.refusal);
        let gb = d.record(circular, (1, 0, 2026));
        let (mol, fmt, _) =
            pl_fileio::load_with_report(gb.as_bytes()).expect("the record we just wrote");
        assert_eq!(fmt, pl_fileio::Format::GenBank);
        (mol, d.title)
    }

    #[test]
    fn bare_bases_become_a_molecule() {
        let (mol, title) = made("pTest", "GAATTCACGTGGATCC", false);
        assert_eq!(mol.seq, b"GAATTCACGTGGATCC");
        assert_eq!(title, "pTest");
        assert_eq!(mol.name, "pTest");
        assert!(!mol.topology.is_circular());
    }

    /// The choice that cannot be deferred: FASTA has no field for it, and a
    /// plasmid pasted as linear gives a wrong answer to every question after.
    #[test]
    fn circular_is_chosen_at_creation_and_survives_the_round_trip() {
        for circular in [false, true] {
            let (mol, _) = made("p", "ACGTACGTACGT", circular);
            assert_eq!(
                mol.topology.is_circular(),
                circular,
                "topology did not survive the GenBank round trip"
            );
        }
    }

    /// The four shapes a real paste arrives in, and one refusal, through the
    /// pipeline `newdoc` shares with Ctrl+V.
    #[test]
    fn the_shapes_people_actually_paste() {
        // Wrapped, with line breaks and indentation.
        let (mol, _) = made("a", "  GAATTC ACGT\n\tGGATCC\r\n\nAAAA\n", false);
        assert_eq!(mol.seq, b"GAATTCACGTGGATCCAAAA");

        // A whole FASTA record, header and all.
        let (mol, _) = made(
            "b",
            ">pUC19 cloning vector\nACGTACGTAC\nGTACGTACGT\n",
            false,
        );
        assert_eq!(mol.seq, b"ACGTACGTACGTACGTACGT");
        assert!(Draft::of("b", ">pUC19 cloning vector\nACGT\n")
            .report
            .dropped
            .iter()
            .any(|d| d.contains("pUC19")));

        // A numbered listing, the shape a record viewer's copy gives.
        let (mol, _) = made(
            "c",
            "        1 gaattcacgt ggatccacgt\n       21 aaaacccc\n",
            false,
        );
        assert_eq!(mol.seq, b"gaattcacgtggatccacgtaaaacccc");

        // Lower case is kept, because case is the only channel the sequence view
        // has for "this is the bit I added".
        let (mol, _) = made("d", "acgtACGT", false);
        assert_eq!(mol.seq, b"acgtACGT");

        // U is kept and is not silently rewritten as T.
        let d = Draft::of("e", "ACGUACGU");
        assert!(d.creatable());
        assert_eq!(d.report.uracil, 2);
        let (mol, _) = made("e", "ACGUACGU", false);
        assert_eq!(mol.seq, b"ACGUACGU");
    }

    /// The whole point of not silently dropping: whatever was removed is
    /// available to say out loud.
    #[test]
    fn what_was_ignored_is_reported_rather_than_dropped_in_silence() {
        let d = Draft::of("x", ">seq1\nACGT\nACGT\n");
        assert!(d.creatable());
        assert!(
            d.report.dropped.iter().any(|s| s.contains("header")),
            "the FASTA header went unmentioned: {:?}",
            d.report.dropped
        );

        let d = Draft::of("x", "        1 acgtacgt\n        9 acgtacgt\n");
        assert!(
            d.report
                .dropped
                .iter()
                .any(|s| s.contains("position number")),
            "the coordinates went unmentioned: {:?}",
            d.report.dropped
        );
    }

    /// Refused, with the character named and located — not accepted with a hole
    /// in it, and not refused with a shrug.
    #[test]
    fn a_character_that_is_not_a_base_is_refused_by_name_and_position() {
        let d = Draft::of("x", "ACGT-ACGT");
        assert!(!d.creatable());
        let why = d.refusal.expect("a refusal");
        assert!(why.contains("U+002D"), "{why}");
        assert!(why.contains('-'), "{why}");
        assert!(
            why.contains("position 5"),
            "the position the user has to go and look at is missing: {why}"
        );

        // A whole line of prose, refused for its punctuation rather than
        // quietly turned into 20-odd ambiguity codes.
        let d = Draft::of("x", "please synthesise this for me, thanks!");
        assert!(!d.creatable(), "{:?}", d.report.bases);
    }

    #[test]
    fn two_records_are_refused_rather_than_fused_into_a_chimera() {
        let d = Draft::of("x", ">a\nACGT\n>b\nTTTT\n");
        assert!(!d.creatable());
        assert!(d.refusal.expect("a refusal").contains("2 FASTA records"));
    }

    /// An empty box says nothing; a box with text but no bases in it says so.
    #[test]
    fn nothing_to_create_is_two_different_states() {
        let d = Draft::of("x", "   \n\n");
        assert!(!d.creatable());
        assert!(
            d.refusal.is_none(),
            "an untouched box must not open already complaining"
        );

        let d = Draft::of("x", ">just a header and nothing else\n");
        assert!(!d.creatable());
        assert!(d.refusal.expect("a refusal").contains("no bases"));
    }

    /// The name is never empty, and what GenBank will actually hold of it is
    /// computed rather than hoped for.
    #[test]
    fn an_unnamed_document_still_has_a_title() {
        let d = Draft::of("   ", "ACGT");
        assert_eq!(d.title, UNTITLED);
        assert_eq!(d.locus, UNTITLED);

        // Sixteen columns of [A-Za-z0-9_.-]; the dialog shows this when it
        // differs so the rename is disclosed rather than discovered.
        let d = Draft::of("my gBlock #3 for the Nissle work", "ACGT");
        assert_ne!(d.locus, d.title);
        assert!(d.locus.len() <= 16, "{}", d.locus);
        let (mol, title) = made("my gBlock #3 for the Nissle work", "ACGT", false);
        assert_eq!(title, "my gBlock #3 for the Nissle work");
        assert_eq!(
            mol.name, d.locus,
            "the molecule is called what the dialog said it would be called"
        );
    }

    /// A name is a NAME. Nothing in this dialog turns one into a path.
    ///
    /// The Name box takes free text and the document it makes has no file
    /// behind it, so the first time that title meets the filesystem is the
    /// `set_file_name` seed on a save dialog the user has not opened yet.
    /// `App::export`, `App::export_protein` and `App::save_dna` all seed it with
    /// `genbank::locus_name(&d.title)`, which is `[A-Za-z0-9_.-]` and sixteen
    /// characters — so this asserts the property at the function all three go
    /// through, on the shapes that would matter if they did not.
    ///
    /// The crash-recovery slot is the other place a title could have become a
    /// path and does not: `recover::slot_path` is `{pid}-{slot}.recover` and the
    /// title is escaped into the file's CONTENTS.
    ///
    /// A newline is included for completeness and is not reachable through the
    /// interface — egui 0.35's single-line `TextEdit` replaces `\r` and `\n`
    /// with a space on paste (`text_edit/builder.rs`, the `Event::Paste` arm)
    /// and Enter inserts nothing. Checked because "unreachable" is a claim
    /// about a dependency's behaviour, and this is the file that finds out if
    /// it changes.
    #[test]
    fn a_name_with_a_path_separator_in_it_never_becomes_a_path() {
        for name in [
            "../../etc/passwd",
            "C:\\Users\\x\\evil.gb",
            "pUC\n19",
            "con.gb",
            "..",
        ] {
            let d = Draft::of(name, "ACGT");
            assert!(d.creatable(), "{name:?} is a name, not a refusal");
            // The title is kept verbatim: it is what the user typed and what
            // the tab is captioned, and quietly editing it would be the same
            // silent rename the LOCUS disclosure exists to avoid.
            assert_eq!(d.title, name);
            // And the LOCUS — which is what every save dialog is seeded from —
            // carries none of it.
            assert!(
                !d.locus.contains(['/', '\\', '\n', ':']),
                "{name:?} produced a LOCUS a file name could be built from: {:?}",
                d.locus
            );
            assert!(d.locus.chars().any(|c| c.is_ascii_alphanumeric()));
            // The record still parses, which is the other half: a title that
            // reached the GenBank header raw would end the LOCUS line.
            let gb = d.record(false, (1, 0, 2026));
            let (mol, _, _) = pl_fileio::load_with_report(gb.as_bytes())
                .unwrap_or_else(|e| panic!("{name:?}: {e}"));
            assert_eq!(mol.seq, b"ACGT");
            assert_eq!(mol.name, d.locus);
        }
    }

    /// One base is a molecule.
    ///
    /// The smallest thing anyone can paste, and the size at which an off-by-one
    /// in a writer shows up: `write_reporting` omits the `source` feature
    /// entirely below one base, and `locus_line` prints the length in a fixed
    /// column. Round-tripped rather than asserted on the string, because what
    /// matters is that the loader gets a 1 bp molecule back.
    #[test]
    fn a_single_base_is_a_molecule_like_any_other() {
        for circular in [false, true] {
            let (mol, _) = made("one", "A", circular);
            assert_eq!(mol.seq, b"A");
            assert_eq!(mol.len(), 1);
            assert_eq!(mol.topology.is_circular(), circular);
        }
    }

    /// The memo must not answer a question it was not asked.
    #[test]
    fn the_draft_is_recomputed_when_the_box_changes() {
        let mut n = New {
            text: "ACGT".into(),
            ..Default::default()
        };
        assert_eq!(n.draft().report.bases, "ACGT");
        n.text = "ACGTACGT".into();
        assert_eq!(n.draft().report.bases, "ACGTACGT");
        n.name = "pX".into();
        assert_eq!(n.draft().title, "pX");
        // ...and the bases it was not asked about are still the bases.
        assert_eq!(n.draft().report.bases, "ACGTACGT");
        assert_eq!(n.draft().locus, "pX");
    }

    /// Typing a NAME must not re-read the bases.
    ///
    /// The memo was keyed on `(name, text)`, so it missed on every character
    /// typed into the Name box and re-ran `sanitise_paste` over the whole
    /// paste. Measured at 43 ms per keystroke on a 10 Mb one, against a frame
    /// that otherwise costs 12 ms — so naming a large gBlock ran at about
    /// 18 fps, in the one dialog whose entire purpose is to receive a large
    /// paste.
    ///
    /// PROVEN TO FAIL against the `(name, text)` key. The whole module was
    /// re-run, not this test by name:
    ///
    /// ```text
    /// ---- newdoc::tests::naming_a_document_does_not_re_read_its_bases stdout ----
    /// assertion `left == right` failed: eight characters of name cost eight
    ///   re-reads of the paste
    ///   left: 9
    ///  right: 1
    /// ```
    ///
    /// Counted rather than timed: see [`New::reads`].
    #[test]
    fn naming_a_document_does_not_re_read_its_bases() {
        let mut n = New {
            text: "GAATTCACGTGGATCC".into(),
            ..Default::default()
        };
        assert_eq!(n.draft().report.bases, "GAATTCACGTGGATCC");
        assert_eq!(n.reads, 1, "the first read");

        for c in "pNissle1".chars() {
            n.name.push(c);
            // What the dialog actually reads per frame, so this is the real
            // call pattern and not a reduced one.
            let d = n.draft();
            assert_eq!(d.report.bases, "GAATTCACGTGGATCC");
            assert!(!d.title.is_empty());
        }
        assert_eq!(
            n.reads, 1,
            "eight characters of name cost eight re-reads of the paste"
        );
        // And the name-derived fields did keep up, which is the half a memo
        // that simply never invalidated would also pass.
        assert_eq!(n.draft().title, "pNissle1");
        assert_eq!(n.draft().locus, "pNissle1");

        // One character into the BASES box does read them again, because that
        // is the question the memo exists to answer.
        n.text.push('A');
        assert_eq!(n.draft().report.bases, "GAATTCACGTGGATCCA");
        assert_eq!(n.reads, 2);
    }

    /// Cancel keeps the work; Create does not offer it back.
    #[test]
    fn cancel_keeps_the_text_and_create_clears_it() {
        let mut n = New::default();
        n.show();
        n.text = "ACGT".into();
        n.name = "pX".into();
        n.hide();
        assert!(!n.open);
        assert_eq!(n.text, "ACGT", "a stray Escape must not cost the paste");
        n.show();
        n.done();
        assert!(!n.open);
        assert!(n.text.is_empty() && n.name.is_empty());
    }
}
