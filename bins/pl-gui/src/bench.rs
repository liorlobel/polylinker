//! The bench: the documents that are open, and which one you are looking at.
//!
//! Stage 0 of the workspace. It holds AT MOST ONE document and behaves exactly
//! as `App::document: Option<Document>` did, on purpose — this stage is a
//! container swap and nothing else, so that the 88 call sites can be moved with
//! the test suite as the judge, before any of them have to think about a second
//! tab.
//!
//! # Why a type rather than a `Vec` on `App`
//!
//! Because the invariant is not expressible on a bare `Vec`. `active` must index
//! a tab that exists, or there must be no tabs; every code path that removes one
//! has to restore that, and doing it at eighty-eight call sites is how it stops
//! being true. Here there is one place to get it right and one place to test it.
//!
//! # What deliberately is NOT here yet
//!
//! Per-tab view state — caret, selection, the feature filter, the ORF settings.
//! Those live on `App` today and are reset wholesale by `App::adopt`. With one
//! tab that is correct and indistinguishable from holding them per tab; with two
//! it is a bug, because switching tabs would show you the other molecule's
//! caret. They move in the stage that introduces the second tab, not before,
//! since moving them now would be a change nothing could yet observe — and this
//! codebase has learned what unobservable changes cost.

use crate::doc::Document;

/// The open documents, and the index of the one on screen.
#[derive(Default)]
pub struct Bench {
    tabs: Vec<Document>,
    /// Always a valid index into `tabs`, or 0 when `tabs` is empty.
    active: usize,
}

impl Bench {
    /// The document on screen.
    pub fn get(&self) -> Option<&Document> {
        self.tabs.get(self.active)
    }

    pub fn get_mut(&mut self) -> Option<&mut Document> {
        self.tabs.get_mut(self.active)
    }

    /// Put `d` on the bench, replacing whatever was there.
    ///
    /// Stage 0 semantics, matching `self.document = Some(d)` exactly. The stage
    /// that adds tabs changes this call, not its callers: opening becomes
    /// "add a tab" and every caller keeps meaning "the user opened something".
    pub fn set(&mut self, d: Document) {
        self.tabs.clear();
        self.tabs.push(d);
        self.active = 0;
    }

    /// Nothing open.
    ///
    /// `#[cfg(test)]` because nothing in the app empties the bench today: the
    /// arm that used to assign `self.document = None` was the failed-load path,
    /// and cc36cf7 removed it precisely so that a load which fails leaves the
    /// open document alone. Kept because the tests exercise the empty state and
    /// because the stage that adds Close Tab needs it — and NOT left `pub` and
    /// unused, since a method with no caller is indistinguishable from one
    /// whose caller was deleted by mistake.
    #[cfg(test)]
    pub fn clear(&mut self) {
        self.tabs.clear();
        self.active = 0;
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Every open document.
    ///
    /// The questions that are about the WORKSPACE rather than the document on
    /// screen — "is there unsaved work anywhere" is the one the close guard
    /// will have to ask once there can be more than one tab. Test-only until
    /// then, for the same reason as `clear`.
    #[cfg(test)]
    pub fn all(&self) -> impl Iterator<Item = &Document> {
        self.tabs.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::Molecule;

    fn doc(name: &str) -> Document {
        Document::of_molecule(Molecule {
            name: name.into(),
            seq: b"ACGTACGTACGT".to_vec(),
            ..Default::default()
        })
    }

    #[test]
    fn an_empty_bench_has_nothing_on_it_and_says_so_rather_than_panicking() {
        let mut b = Bench::default();
        assert!(b.is_empty());
        assert!(b.get().is_none());
        assert!(b.get_mut().is_none());
        assert_eq!(b.all().count(), 0);
    }

    #[test]
    fn set_replaces_and_leaves_active_pointing_at_something_real() {
        let mut b = Bench::default();
        b.set(doc("first"));
        assert_eq!(
            b.get().map(|d| d.molecule().name.clone()),
            Some("first".into())
        );
        b.set(doc("second"));
        assert_eq!(
            b.get().map(|d| d.molecule().name.clone()),
            Some("second".into())
        );
        // Stage 0 holds one. When that changes, this assertion is the thing
        // that should be edited deliberately rather than quietly stop holding.
        assert_eq!(b.all().count(), 1);
        b.clear();
        assert!(b.is_empty() && b.get().is_none());
    }

    /// `active` must never index past the end. It cannot today — `set` and
    /// `clear` both reset it — and this is here so that the stage which adds
    /// tab removal has something to fail.
    #[test]
    fn active_always_indexes_a_tab_that_exists() {
        let mut b = Bench::default();
        for _ in 0..3 {
            b.set(doc("x"));
            assert!(b.active < b.tabs.len().max(1));
            assert!(b.get().is_some());
        }
        b.clear();
        assert_eq!(b.active, 0);
        assert!(b.get().is_none());
    }
}
