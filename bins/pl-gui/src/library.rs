//! The Library tab: a folder of files, searchable.
//!
//! Scanning a shared drive takes seconds, and seconds on the UI thread are
//! dropped frames, so it runs on a worker — modelled on `DigestState` in
//! `doc.rs`, down to the "send failing means the caller moved on; that is fine"
//! contract. Zero new dependencies: `std::thread` and `mpsc`.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use pl_index::codec::Library;
use pl_index::query::{Filters, Query, Results};
use pl_index::scan::Motif;
use pl_scan::{ScanOptions, ScanReport};

/// Which field the search box is searching.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Mode {
    Name,
    Text,
    Motif,
    Enzyme,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Name => "Name",
            Mode::Text => "Features",
            Mode::Motif => "Sequence",
            Mode::Enzyme => "Enzyme",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Mode::Name => "part of a file or plasmid name",
            Mode::Text => "a feature, primer or note",
            Mode::Motif => "IUPAC bases, e.g. GGWCC",
            Mode::Enzyme => "an enzyme name, e.g. EcoRI",
        }
    }
}

/// A scan in flight, or its result.
pub enum ScanState {
    Running {
        root: PathBuf,
        rx: Receiver<Result<(Library, ScanReport), String>>,
    },
    Done {
        root: PathBuf,
        lib: Box<Library>,
        report: ScanReport,
    },
    Failed(String),
}

impl ScanState {
    pub fn is_running(&self) -> bool {
        matches!(self, ScanState::Running { .. })
    }

    /// Collect the worker's result if it has finished. Returns true if the
    /// state changed, so the caller knows to repaint.
    pub fn poll(&mut self) -> bool {
        let done = match self {
            ScanState::Running { rx, root } => rx.try_recv().ok().map(|r| (r, root.clone())),
            _ => None,
        };
        let Some((result, root)) = done else {
            return false;
        };
        *self = match result {
            Ok((lib, report)) => ScanState::Done {
                root,
                lib: Box::new(lib),
                report,
            },
            Err(e) => ScanState::Failed(e),
        };
        true
    }

    pub fn library(&self) -> Option<&Library> {
        match self {
            ScanState::Done { lib, .. } => Some(lib),
            _ => None,
        }
    }
}

/// Start a scan on a worker thread.
///
/// The existing index is loaded first, so a rescan of an unchanged folder costs
/// a `stat` per file rather than a re-parse. A damaged or stale index is
/// reported by `pl-scan` and rebuilt; only a *newer* one stops us, and that
/// surfaces as `Failed` rather than being overwritten.
pub fn start(root: PathBuf) -> ScanState {
    let (tx, rx) = channel();
    let worker_root = root.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let dir = pl_scan::cache_dir()?;
            let path = pl_scan::index_path(&dir, &worker_root);
            let previous = match pl_scan::load(&path) {
                Ok(v) => v,
                Err(e) if e.rebuildable() => None,
                Err(e) => return Err(format!("{}: {e}", path.display())),
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let (lib, report) = pl_scan::scan(
                &worker_root,
                now,
                &ScanOptions {
                    previous,
                    ..Default::default()
                },
            );
            // Saving is best-effort: a read-only cache directory should cost
            // the next scan time, not this answer.
            let _ = pl_scan::save(&path, &lib);
            Ok((lib, report))
        })();
        // Send failing means the user moved on. That is fine.
        let _ = tx.send(result);
    });
    ScanState::Running { root, rx }
}

/// What the search box will actually do, given what is typed.
///
/// Returned rather than rendered so the caller can show it *before* searching.
/// An empty result is only legible as "searched and absent" if the user can see
/// what was asked — and a motif that can never match is an error here, not a
/// clean empty list.
pub enum Parsed {
    /// Nothing typed yet.
    Idle,
    Ready(Box<Query>, Option<String>),
    Rejected(String),
}

pub fn parse_query(mode: Mode, needle: &str, absent: bool) -> Parsed {
    let needle = needle.trim();
    if needle.is_empty() {
        return Parsed::Idle;
    }
    let filters = Filters::default();
    match mode {
        Mode::Name => Parsed::Ready(
            Box::new(Query {
                name: Some(needle.to_string()),
                filters,
                ..Default::default()
            }),
            None,
        ),
        Mode::Text => Parsed::Ready(
            Box::new(Query {
                text: Some(needle.to_string()),
                filters,
                ..Default::default()
            }),
            None,
        ),
        Mode::Motif => match Motif::new(needle) {
            Ok(m) => {
                let note = m.describe();
                Parsed::Ready(
                    Box::new(Query {
                        motif: Some(m),
                        filters,
                        absent,
                        ..Default::default()
                    }),
                    Some(note),
                )
            }
            Err(e) => Parsed::Rejected(e.to_string()),
        },
        Mode::Enzyme => match pl_enzymes::by_name(needle) {
            Some(e) => {
                let m = Motif::new(e.site).expect("a shipped site is always valid");
                let note = format!("{} — {}", e.name, m.describe());
                Parsed::Ready(
                    Box::new(Query {
                        motif: Some(m),
                        filters,
                        absent,
                        ..Default::default()
                    }),
                    Some(note),
                )
            }
            // Never fall through to searching for the literal letters of the
            // name: `BsaI` would become a search for B-s-a-I, which matches
            // nothing and looks like an answer.
            // The count is computed, and so is the shortest site: a sentence
            // that tells a user what is missing has to be right about what is
            // present. This read "There is no BsaI, BsmBI, BbsI or SapI yet"
            // until 2026-09-03, naming four enzymes that all ship — twelve
            // lines above a test that asserts `by_name("BsaI").is_some()`,
            // which is how long a false sentence can sit next to the thing
            // that disproves it. Corrected rather than deleted, because the
            // shape of the message was right and only its example was wrong.
            None => Parsed::Rejected(format!(
                "{needle:?} is not one of the {} enzymes shipped, whose shortest \
                 recognition site is {} bp — REBASE is what real work wants and \
                 it is not redistributed here. Switch to Sequence and type the \
                 site to search for it anyway.",
                pl_enzymes::ENZYMES.len(),
                pl_enzymes::ENZYMES
                    .iter()
                    .map(|e| e.site.len())
                    .min()
                    .unwrap_or(0)
            )),
        },
    }
}

/// Run a query against a scanned library.
pub fn run<'a>(lib: &'a Library, q: &Query) -> Results<'a> {
    pl_index::query::run(&lib.rows, &lib.packed, q)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unsatisfiable_motif_is_rejected_rather_than_searched() {
        // Typing `5'-GAATTC-3'` must say why, not return nothing.
        let p = parse_query(Mode::Motif, "5'-GAATTC-3'", false);
        match p {
            Parsed::Rejected(e) => assert!(e.contains("byte 1"), "{e}"),
            _ => panic!("a pattern with a non-code byte must be rejected"),
        }
        assert!(matches!(
            parse_query(Mode::Motif, "  ", false),
            Parsed::Idle
        ));
        assert!(matches!(
            parse_query(Mode::Motif, "GGWCC", false),
            Parsed::Ready(..)
        ));
    }

    #[test]
    fn an_unknown_enzyme_names_itself_instead_of_becoming_a_motif() {
        // `BsaI` used to be the example here, because it was not shipped. It
        // is now, so the test needs a name that really is absent — otherwise
        // it would pass while asserting nothing.
        assert!(pl_enzymes::by_name("BsaI").is_some(), "BsaI ships now");
        match parse_query(Mode::Enzyme, "NotAnEnzyme", false) {
            Parsed::Rejected(e) => {
                assert!(e.contains("NotAnEnzyme"), "{e}");
                // AND THE REFUSAL MAY NOT NAME A SHIPPED ENZYME AS ABSENT.
                //
                // PROVEN TO FAIL on 2026-09-03 against the shipped sentence
                // "There is no BsaI, BsmBI, BbsI or SapI yet — switch to
                // Sequence and type the site", every name in which is in the
                // table. The assertion above could not see it: it only checks
                // that the refusal names what the user typed. This one reads
                // the message back against `by_name`, so a message that
                // declares any enzyme missing fails unless it really is.
                for word in e.split(|c: char| !c.is_ascii_alphanumeric()) {
                    assert!(
                        pl_enzymes::by_name(word).is_none(),
                        "the refusal names {word:?} as missing, and it ships: {e}"
                    );
                }
            }
            _ => panic!("an unknown name must not silently become a motif"),
        }
        for known in ["EcoRI", "BsaI", "BsmBI", "SapI"] {
            assert!(
                matches!(parse_query(Mode::Enzyme, known, false), Parsed::Ready(..)),
                "{known} should be searchable"
            );
        }
    }

    #[test]
    fn the_note_says_what_will_be_searched_before_it_is_searched() {
        let Parsed::Ready(_, Some(note)) = parse_query(Mode::Motif, "GGWCC", false) else {
            panic!("a motif should carry a description");
        };
        assert!(note.contains("W = A|T"), "{note}");
        assert!(note.contains("both strands"), "{note}");

        let Parsed::Ready(_, Some(note)) = parse_query(Mode::Enzyme, "EcoRI", false) else {
            panic!("an enzyme should carry one too");
        };
        assert!(note.contains("EcoRI"), "{note}");
        assert!(note.contains("GAATTC"), "{note}");
    }

    #[test]
    fn name_and_text_search_different_fields() {
        let Parsed::Ready(q, _) = parse_query(Mode::Name, "pUC", false) else {
            panic!()
        };
        assert_eq!(q.name.as_deref(), Some("pUC"));
        assert!(q.text.is_none());

        let Parsed::Ready(q, _) = parse_query(Mode::Text, "AmpR", false) else {
            panic!()
        };
        assert_eq!(q.text.as_deref(), Some("AmpR"));
        assert!(q.name.is_none());
    }
}
