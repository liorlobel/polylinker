//! Searching the molecule that is open.
//!
//! The Library tab searches an *indexed folder*; until now nothing searched the
//! document on screen. That is the most-used key in a plasmid editor — paste
//! twenty bases of a primer, find where it lands — and every competing tool has
//! had it for thirty years.
//!
//! # Why this is not `pl_index::scan`
//!
//! `pl-index` is already a dependency and its `scan` module already does
//! origin-aware, both-strand, IUPAC motif search. It cannot be used here. Its
//! entry points take the library's nibble-packed store plus a `Row` describing a
//! file on disk — `path`, `size`, `mtime_ns`, `content`, `seq_off` — and a
//! molecule that came from no index has none of those. Using it would mean
//! packing the whole sequence into that representation on every keystroke, to
//! forge a row for a file that may not exist.
//!
//! So the search is `pl_core::iupac::find_all` directly, and `pl-index` is still
//! used for the one thing it is uniquely good for: `Motif::new` VALIDATES the
//! query and `Motif::describe` says what was actually searched for. An empty
//! result only reads as "searched and absent" if the user can see the question.
//!
//! # The asymmetry is deliberate and is the right way round
//!
//! `iupac::matches` lets a pattern `N` match a subject `A`, and does **not** let
//! a pattern `A` match a subject `N`. So a degenerate primer finds its sites,
//! and a plain query does not silently match the `N`s in a draft assembly and
//! report a landing site that is not known to exist.

use pl_index::scan::Strand;

/// One place the query matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// 1-based start on the PLUS strand, as `iupac::find_all` reports it.
    pub start: u64,
    /// Which strand carries the match reading 5'->3'.
    ///
    /// `Both` for a palindrome, which is one site and not two — reporting
    /// `GAATTC` twice at the same coordinate would make every EcoRI site look
    /// like a repeat.
    pub strand: Strand,
}

/// The find bar's state, for one document.
///
/// PER TAB. It holds coordinates into one molecule, so carrying it across a tab
/// switch would put "3 of 7" beside a plasmid that has neither.
#[derive(Debug, Default, Clone)]
pub struct Find {
    pub open: bool,
    pub query: String,
    pub hits: Vec<Hit>,
    /// Which hit is selected, as an index into `hits`.
    pub at: usize,
    /// Why there is nothing, when there is nothing.
    pub note: Option<String>,
    /// What was searched for, in the motif's own words.
    pub what: Option<String>,
    /// The query and molecule the hits were computed for, so a redraw does not
    /// re-scan. `u64` is the document's `seq_version`.
    pub done: Option<(String, u64)>,
}

impl Find {
    /// Search `seq` for `query`, filling `hits`, `note` and `what`.
    ///
    /// Both strands, origin-aware, and deduplicated for a palindrome. Returns
    /// the hit that should be selected, if any.
    ///
    /// Pure: no `Ui`, no document, no clock. The whole reason it is here rather
    /// than inline in the panel is that the arithmetic below — which strand,
    /// which end is the anchor, does it wrap — is the part that can be wrong in
    /// a way nobody sees, and it is testable without standing up a frame.
    pub fn search(&mut self, query: &str, seq: &[u8], circular: bool) {
        self.hits.clear();
        self.note = None;
        self.what = None;
        let q = query.trim();
        if q.is_empty() {
            return;
        }
        // Validated through `pl-index`, whose refusals are already written and
        // already tested: "byte 6 is 'X', which is not an IUPAC nucleotide code
        // and can never match" is exactly the sentence a pasted `5'-GAATTC-3'`
        // needs, and it beats searching for it and reporting nothing.
        let motif = match pl_index::scan::Motif::new(q) {
            Ok(m) => m,
            Err(e) => {
                self.note = Some(e.to_string());
                return;
            }
        };
        self.what = Some(motif.describe());
        if q.len() > seq.len() {
            self.note = Some(format!(
                "{} bases is longer than this molecule ({} bp)",
                q.len(),
                seq.len()
            ));
            return;
        }
        let pat = q.as_bytes().to_ascii_uppercase();
        if pl_core::iupac::is_palindrome_masks(&pat) {
            // One site, not two. `Both` records that the two readings are the
            // same site rather than collapsing the information away.
            for start in pl_core::iupac::find_all(&pat, seq, circular) {
                self.hits.push(Hit {
                    start,
                    strand: Strand::Both,
                });
            }
        } else {
            for start in pl_core::iupac::find_all(&pat, seq, circular) {
                self.hits.push(Hit {
                    start,
                    strand: Strand::Forward,
                });
            }
            let rc = pl_core::iupac::reverse_complement(&pat);
            for start in pl_core::iupac::find_all(&rc, seq, circular) {
                self.hits.push(Hit {
                    start,
                    strand: Strand::Reverse,
                });
            }
        }
        // Sorted so "next" means "further along the molecule" rather than
        // "found by the second pass". A user stepping through hits is reading
        // the map left to right.
        self.hits.sort_by_key(|h| (h.start, h.strand as u8));
        if self.hits.is_empty() {
            self.note = Some("no match on either strand".into());
        }
        if self.at >= self.hits.len() {
            self.at = 0;
        }
    }

    /// Step forward, wrapping. Wrapping is right for a circle and right for a
    /// line too: the alternative is a button that stops working at the end with
    /// nothing saying why.
    pub fn next(&mut self) {
        if !self.hits.is_empty() {
            self.at = (self.at + 1) % self.hits.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.hits.is_empty() {
            self.at = (self.at + self.hits.len() - 1) % self.hits.len();
        }
    }

    /// "3 of 7", or what went wrong.
    pub fn tally(&self) -> String {
        match (&self.note, self.hits.len()) {
            (Some(n), _) => n.clone(),
            (None, 0) => String::new(),
            (None, n) => format!("{} of {n}", self.at + 1),
        }
    }
}

/// Where a hit sits, as a selection.
///
/// Carets are GAPS, `0..=n`. A hit starting at 1-based `start` of length `k`
/// therefore spans carets `start - 1` to `start - 1 + k` on a line.
///
/// Two things are not obvious and are both load-bearing:
///
/// - **A minus-strand hit is anchored at its 3' end on the plus strand**, so
///   `head < anchor`. That is the same bit the sequence view's translation lane
///   reads as "reverse", so the amino acids shown beside a found reverse primer
///   are the ones it actually anneals to.
/// - **`through_origin` is not derivable from the carets.** A pair of carets on
///   a circle names two arcs, and only this flag says which. A wrapping hit at
///   8..3 of a 10 bp circle has `base_count == 6`; read as `[3, 8]` it is 4, and
///   both are plausible numbers for a map to draw.
pub fn selection(h: Hit, k: u64, n: u64, circular: bool) -> crate::seqedit::Selection {
    let wraps = circular && h.start - 1 + k > n;
    let lo = h.start - 1;
    let hi = if wraps {
        (h.start - 1 + k) % n
    } else {
        h.start - 1 + k
    };
    let reverse = h.strand == Strand::Reverse;
    let (anchor, head) = if reverse { (hi, lo) } else { (lo, hi) };
    crate::seqedit::Selection {
        anchor,
        head,
        through_origin: wraps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(q: &str, seq: &str, circular: bool) -> Find {
        let mut f = Find::default();
        f.search(q, seq.as_bytes(), circular);
        f
    }

    /// Both strands, and a palindrome is one site rather than two.
    #[test]
    fn a_query_is_found_on_either_strand_and_a_palindrome_only_once() {
        // GGATCC is its own reverse complement; GAATTG is not.
        let f = find("GGATCC", "AAAAGGATCCTTTT", false);
        assert_eq!(
            f.hits,
            vec![Hit {
                start: 5,
                strand: Strand::Both
            }],
            "a palindrome was reported twice, so every BamHI site reads as a repeat"
        );

        // A non-palindrome present only as the reverse complement.
        let f = find("CAATTC", "AAAAGAATTGTTTT", false);
        assert_eq!(f.hits.len(), 1, "the minus strand was not searched");
        assert_eq!(f.hits[0].strand, Strand::Reverse);
        assert_eq!(f.hits[0].start, 5);
    }

    /// A match spanning the origin is ONE match, not none and not two.
    #[test]
    fn a_match_across_the_origin_of_a_circle_is_found_whole() {
        // TTC | TTTTGAA -> GAATTC spans the origin starting at base 8.
        let f = find("GAATTC", "TTCTTTTGAA", true);
        assert_eq!(
            f.hits,
            vec![Hit {
                start: 8,
                strand: Strand::Both
            }],
            "an origin-spanning site was missed — the commonest place for one to hide"
        );
        // The same molecule read as a line has no such site.
        assert!(
            find("GAATTC", "TTCTTTTGAA", false).hits.is_empty(),
            "a linear molecule matched across its own ends"
        );
    }

    /// The wrapping selection covers the bases the hit covers.
    ///
    /// The naive answer — order the two carets and take the span between them —
    /// gives 4 bases for a 6-base hit, and it is a plausible enough number that
    /// a map would draw it without complaint.
    #[test]
    fn an_origin_spanning_hit_selects_all_of_its_own_bases() {
        let h = Hit {
            start: 8,
            strand: Strand::Both,
        };
        let s = selection(h, 6, 10, true);
        assert!(
            s.through_origin,
            "the wrap flag is the only thing that says which arc"
        );
        assert_eq!(
            s.base_count(10),
            6,
            "the selection covers the wrong number of bases"
        );
    }

    /// A reverse hit is anchored at its 3' end, which is what makes the
    /// translation lane show the strand the oligo anneals to.
    #[test]
    fn a_reverse_hit_runs_backwards() {
        let fwd = selection(
            Hit {
                start: 5,
                strand: Strand::Forward,
            },
            6,
            40,
            false,
        );
        assert!(fwd.anchor < fwd.head);
        let rev = selection(
            Hit {
                start: 5,
                strand: Strand::Reverse,
            },
            6,
            40,
            false,
        );
        assert!(
            rev.head < rev.anchor,
            "a reverse hit reads forwards, so the residues beside it are the wrong strand"
        );
        assert_eq!(
            (rev.head.min(rev.anchor), rev.head.max(rev.anchor)),
            (fwd.anchor, fwd.head),
            "the two strands' hits cover different bases"
        );
    }

    /// A query that can never match is refused with the reason, not searched.
    #[test]
    fn something_that_is_not_dna_is_named_rather_than_reported_as_absent() {
        let f = find("5'-GAATTC-3'", "AAAAGGATCCTTTT", false);
        assert!(f.hits.is_empty());
        let note = f.note.expect("a reason");
        assert!(
            note.contains("IUPAC") || note.contains("never match"),
            "the refusal does not say what is wrong with the query: {note}"
        );
        // And a real query that is simply absent says something different, or
        // the two cases are indistinguishable to a reader.
        let f = find("GGGGGG", "AAAAGGATCCTTTT", false);
        assert_eq!(f.note.as_deref(), Some("no match on either strand"));
    }

    /// Stepping wraps, and the tally counts from one.
    #[test]
    fn stepping_wraps_in_both_directions() {
        let mut f = find("AA", "AATTAATTAA", false);
        let n = f.hits.len();
        assert!(n >= 3, "the fixture needs several hits, got {n}");
        assert_eq!(f.tally(), format!("1 of {n}"));
        f.prev();
        assert_eq!(f.at, n - 1, "stepping back from the first did not wrap");
        f.next();
        assert_eq!(f.at, 0, "stepping on from the last did not wrap");
    }

    /// A query longer than the molecule is not a match failure.
    #[test]
    fn a_query_longer_than_the_molecule_says_so() {
        let f = find("GAATTCGAATTC", "AAAACCCC", false);
        assert!(f.hits.is_empty());
        assert!(
            f.note
                .unwrap_or_default()
                .contains("longer than this molecule"),
            "a query that cannot fit read as an ordinary absence"
        );
    }

    /// Degeneracy runs from the query to the subject and not back.
    ///
    /// A degenerate primer finds its sites; a plain query does not silently
    /// match the `N`s in a draft assembly and claim a landing site nobody has
    /// established.
    #[test]
    fn degeneracy_is_asymmetric_the_way_a_primer_needs() {
        assert_eq!(find("GGWCC", "AAGGACCAA", false).hits.len(), 1);
        assert_eq!(find("GGTCC", "AAGGNCCAA", false).hits.len(), 0);
    }
}
