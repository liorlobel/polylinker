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
use crate::{aa, gel, seqedit, CentralView};

/// Everything on `App` that belongs to ONE document rather than to the session.
///
/// THE LIST IS `adopt`'s LIST, and that is the point. `App::adopt` already reset
/// exactly these fields when a document was replaced, because each of them is
/// meaningless — or worse, quietly wrong — against a different molecule. With
/// one document, "reset on replace" and "keep one per tab" are indistinguishable.
/// With two tabs they are not: the first shows you a fresh caret, the second
/// shows you the caret you left.
///
/// So `Default` is what `adopt` used to assign, field for field, and switching
/// tabs swaps this whole struct in and out. A field that belongs here and is
/// left on `App` does not fail loudly — it silently leaks one molecule's state
/// onto another, which is why `no_view_state_leaks_between_tabs` enumerates
/// them rather than trusting this comment.
pub struct DocView {
    /// The line under the toolbar describing this file.
    pub status: String,
    pub notice: Option<String>,
    pub edit: seqedit::SeqEdit,
    /// An INDEX into this molecule's features, meaningless against another.
    pub selected: Option<usize>,
    pub hot: Option<usize>,
    pub hot_shown: Option<usize>,
    /// The Features tab's search box. `adopt` never reset it, so it survived a
    /// document swap lighting nothing — a bug with one document, and a leak
    /// with two.
    pub filter: String,
    pub enz_strip: bool,
    pub orf_strip: bool,
    pub tr: aa::Translations,
    pub doc_code: pl_core::translate::Code,
    pub gel: gel::View,
    pub central_view: CentralView,
}

impl Default for DocView {
    /// A view nobody has looked through yet.
    ///
    /// Two fields cannot be derived and neither is a free choice. `central_view`
    /// is `Map` because that is what `adopt` shows for a newly opened file.
    /// `doc_code` has no honest default at all — `adopt` computes it from the
    /// molecule, as the modal `/transl_table` across its CDS features or the
    /// user's own preference — so table 1 stands in, and the value is NEVER
    /// observed: a tab's stored view is written by `store` before it can be read
    /// by `activate`, and the tab created by `set` is active immediately, with
    /// `App`'s own fields set by `adopt` from the real molecule.
    ///
    /// Written out rather than derived so that a future field with a meaningful
    /// default has to be considered here instead of silently getting a zero.
    fn default() -> Self {
        DocView {
            status: String::new(),
            notice: None,
            edit: seqedit::SeqEdit::new(),
            selected: None,
            hot: None,
            hot_shown: None,
            filter: String::new(),
            enz_strip: false,
            orf_strip: false,
            tr: aa::Translations::default(),
            doc_code: pl_core::translate::table(1).expect("the standard code is compiled in"),
            gel: gel::View::default(),
            central_view: CentralView::Map,
        }
    }
}

/// One tab: a document and how you were looking at it.
pub struct Tab {
    pub doc: Document,
    /// The view state for this tab while it is NOT active.
    ///
    /// The active tab's view lives on `App`, because several hundred call sites
    /// read `self.edit` and `self.selected` directly and rewriting them all
    /// would be a far larger change than this one with no more safety. Switching
    /// swaps the two. The stored copy of the active tab is stale by design and
    /// is never read while it is active.
    pub view: DocView,
}

/// The open documents, and the index of the one on screen.
#[derive(Default)]
pub struct Bench {
    tabs: Vec<Tab>,
    /// Always a valid index into `tabs`, or 0 when `tabs` is empty.
    active: usize,
}

impl Bench {
    /// The document on screen.
    pub fn get(&self) -> Option<&Document> {
        self.tabs.get(self.active).map(|t| &t.doc)
    }

    pub fn get_mut(&mut self) -> Option<&mut Document> {
        self.tabs.get_mut(self.active).map(|t| &mut t.doc)
    }

    /// Open `d` in a NEW tab and make it active.
    ///
    /// This is where Stage 1 changes what every caller means without changing
    /// what any caller says. `set` used to replace the one document; it now
    /// adds. Opening a file therefore cannot destroy work any more — there is
    /// nothing to destroy, because nothing is replaced — which is why the
    /// unsaved-changes question moved off the open paths entirely and onto the
    /// one place work can still be lost: closing.
    pub fn set(&mut self, d: Document) {
        self.tabs.push(Tab {
            doc: d,
            view: DocView::default(),
        });
        self.active = self.tabs.len() - 1;
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// Titles, for the tab strip.
    pub fn titles(&self) -> Vec<(String, bool)> {
        self.tabs
            .iter()
            .map(|t| (t.doc.title.clone(), t.doc.unsaved()))
            .collect()
    }

    /// Is there unsaved work in ANY tab?
    ///
    /// The question the close guard has to ask now, and the one thing Stage 1
    /// could get wrong in a way that costs somebody their work: with one
    /// document "the open document is unsaved" and "the workspace has unsaved
    /// work" were the same sentence, and they are not any more.
    pub fn any_unsaved(&self) -> bool {
        self.tabs.iter().any(|t| t.doc.unsaved())
    }

    /// How many tabs hold unsaved work, for a dialog that has to be specific.
    pub fn unsaved_count(&self) -> usize {
        self.tabs.iter().filter(|t| t.doc.unsaved()).count()
    }

    /// Store the active tab's view, so it can be restored when you come back.
    pub fn store(&mut self, v: DocView) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.view = v;
        }
    }

    /// Make `i` active and hand back its stored view.
    ///
    /// `None` when `i` is not a tab or is already active — the caller must not
    /// then scatter a default view over the one on screen, which would clear the
    /// user's caret for clicking the tab they were already on.
    pub fn activate(&mut self, i: usize) -> Option<DocView> {
        if i >= self.tabs.len() || i == self.active {
            return None;
        }
        self.active = i;
        Some(std::mem::take(&mut self.tabs[i].view))
    }

    /// Close tab `i`, returning it so the caller can offer it back.
    ///
    /// `active` is repaired here rather than at the call site: closing a tab to
    /// the left of the active one shifts every later index down by one, and a
    /// caller that forgets shows the wrong molecule under the right title.
    pub fn close(&mut self, i: usize) -> Option<Tab> {
        if i >= self.tabs.len() {
            return None;
        }
        let t = self.tabs.remove(i);
        if self.tabs.is_empty() {
            self.active = 0;
        } else if i < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
        }
        Some(t)
    }

    /// Re-open a closed tab at the end, active.
    pub fn reopen(&mut self, t: Tab) {
        self.tabs.push(t);
        self.active = self.tabs.len() - 1;
    }

    /// The view stored for whatever is active now.
    ///
    /// Used after a close, when `active` has moved to a tab whose view is in
    /// its slot rather than on `App`. `None` when the bench is empty, which the
    /// caller must not treat as "scatter a default": there is nothing to show,
    /// and `App`'s fields were already blanked by `take_view`.
    pub fn take_active_view(&mut self) -> Option<DocView> {
        self.tabs
            .get_mut(self.active)
            .map(|t| std::mem::take(&mut t.view))
    }

    /// Nothing open.
    ///
    /// Still `#[cfg(test)]`: nothing in the app empties the bench even now.
    /// The arm that used to assign `self.document = None` was the failed-load
    /// path, and cc36cf7 removed it precisely so a load which fails leaves the
    /// open document alone; `close_tab` removes ONE tab and never the lot. It
    /// stays because the tests exercise the empty state, and it stays gated
    /// because a `pub` method with no caller is indistinguishable from one
    /// whose caller was deleted by mistake.
    #[cfg(test)]
    pub fn clear(&mut self) {
        self.tabs.clear();
        self.active = 0;
    }

    /// Nothing open — asked by Ctrl+W before it closes anything.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Every open document.
    ///
    /// Test-only, and the reason is worth stating: the two production questions
    /// about the WHOLE workspace — "is there unsaved work anywhere" and "how
    /// many tabs have it" — are answered by `any_unsaved` and `unsaved_count`
    /// rather than by handing the caller an iterator and trusting it to fold
    /// correctly. A guard that a caller has to assemble is a guard with a place
    /// to be assembled wrongly.
    #[cfg(test)]
    pub fn all(&self) -> impl Iterator<Item = &Document> {
        self.tabs.iter().map(|t| &t.doc)
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

    /// A document that came FROM A FILE, and so starts clean.
    ///
    /// `of_molecule` cannot: it passes no path, and `Document::from_bytes` reads
    /// `saved = path.is_some().then_some(None)` — a document with nowhere to
    /// have been written is unsaved work by construction, which is correct and
    /// is why the recovery banner's restored draft counts as unsaved. A test
    /// about which tabs are DIRTY therefore has to start from ones that are not.
    fn saved_doc(name: &str) -> Document {
        let mut d = doc(name);
        // `mark_saved` rather than a path, because `unsaved()` is
        // `saved != Some(log.cursor())` — it is about the CURSOR, not about
        // whether a path exists, which is cc36cf7's whole redefinition. Saying
        // "this state is on disk" directly is the only way to start clean that
        // does not depend on how `from_bytes` happens to seed the field.
        d.mark_saved();
        assert!(!d.unsaved(), "the helper must produce a clean document");
        d
    }

    #[test]
    fn an_empty_bench_has_nothing_on_it_and_says_so_rather_than_panicking() {
        let mut b = Bench::default();
        assert!(b.is_empty());
        assert!(b.get().is_none());
        assert!(b.get_mut().is_none());
        assert_eq!(b.all().count(), 0);
    }

    /// Stage 0 asserted that `set` REPLACED, and said in as many words that the
    /// assertion should be edited deliberately when that changed. This is that
    /// edit: opening now adds a tab and makes it active, which is the whole of
    /// why opening a file can no longer cost anybody their work.
    #[test]
    fn opening_adds_a_tab_and_makes_it_active() {
        let mut b = Bench::default();
        b.set(doc("first"));
        assert_eq!(b.len(), 1);
        b.set(doc("second"));
        assert_eq!(b.len(), 2, "opening replaced instead of adding");
        assert_eq!(b.active(), 1);
        assert_eq!(
            b.get().map(|d| d.molecule().name.clone()),
            Some("second".into())
        );
        // And the first is still there, untouched.
        assert_eq!(
            b.all()
                .map(|d| d.molecule().name.clone())
                .collect::<Vec<_>>(),
            vec!["first".to_string(), "second".to_string()]
        );
        b.clear();
        assert!(b.is_empty() && b.get().is_none());
    }

    /// Closing repairs `active` here rather than at the call site, because a
    /// caller that forgets shows the wrong molecule under the right title.
    #[test]
    fn closing_a_tab_leaves_active_on_something_real() {
        let mut b = Bench::default();
        for n in ["a", "b", "c"] {
            b.set(doc(n));
        }
        assert_eq!((b.len(), b.active()), (3, 2));

        // Closing one to the LEFT of the active tab shifts it down.
        b.close(0);
        assert_eq!(b.len(), 2);
        assert_eq!(b.get().map(|d| d.molecule().name.clone()), Some("c".into()));

        // Closing the active one lands on a tab that exists.
        b.close(b.active());
        assert_eq!(b.len(), 1);
        assert!(b.get().is_some());
        assert!(b.active() < b.len());

        b.close(0);
        assert!(b.is_empty());
        assert_eq!(b.active(), 0);
        assert!(b.close(0).is_none(), "closing nothing must not panic");
    }

    #[test]
    fn unsaved_is_asked_of_every_tab_not_just_the_visible_one() {
        let mut b = Bench::default();
        b.set(saved_doc("clean"));
        b.set(saved_doc("also clean"));
        assert!(!b.any_unsaved());
        assert_eq!(b.unsaved_count(), 0);

        // Edit the one that is NOT active.
        b.activate(0);
        b.get_mut()
            .expect("a tab")
            .apply(pl_core::OpKind::InsertAt {
                at: 1,
                seq: "AAAA".into(),
            })
            .expect("an ordinary insert");
        b.activate(1);
        assert!(
            b.any_unsaved(),
            "an edit in a background tab is still unsaved work"
        );
        assert_eq!(b.unsaved_count(), 1);
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
