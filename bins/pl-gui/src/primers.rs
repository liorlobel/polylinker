//! Where an oligo you already have anneals on the molecule that is open.
//!
//! # Why this module exists
//!
//! `pl-primer` was a finished, tested engine that `pl primers` used and that
//! `bins/pl-gui` had no dependency on at all: it reached the binary only
//! transitively, buried inside `pl-design`'s off-target prefilter
//! (`crates/pl-design/src/specificity.rs`). So the desktop app could *design* a
//! primer pair and could not answer the more ordinary question — here is an
//! oligo from the freezer box, where does it land on this plasmid? That is the
//! same defect, one crate over, that `pl-features` had until 2026-08-06, and it
//! is recorded here so the shape of it is recognisable the next time.
//!
//! # This module decides; `main.rs` paints
//!
//! Nothing here touches egui. The arithmetic that can be quietly wrong — which
//! caret a binding selects, whether it wraps the origin, which end is the 3'
//! end, what the Tm is a Tm *of* — is therefore exercised by ordinary unit
//! tests instead of by driving a window, exactly as `find.rs` and `annot.rs`
//! are.
//!
//! # One engine call, one set of defaults
//!
//! [`Primers::params`] is the ONLY place a [`pl_primer::Params`] is built for
//! the GUI. Every field the panel does not put a control on is taken from
//! `pl_primer::Params::default()` — the value `cmd_primers` starts from at
//! `bins/pl/src/main.rs:4602` — and the fields are LISTED rather than updated
//! over, so a setting added to the engine is a compile error here instead of a
//! silent default. The seed control's bounds are
//! `pl_primer::SEED_MIN..=SEED_MAX`, which is the range the CLI's `--seed`
//! validator now reads too, so the two cannot drift apart by editing one of
//! them.
//!
//! `the_primers_panel_and_the_cli_agree_about_the_same_primer_and_molecule` in
//! `main.rs` asserts the results, not just the settings: a fresh panel's
//! bindings must equal `pl_primer::find_bindings(primer, seq, circular,
//! &Params::default())`, which is the literal expression `cmd_primers` runs.
//!
//! # What is NOT shared, and why that is not a divergence
//!
//! The panel trims leading and trailing whitespace off the pasted oligo; the
//! CLI does not, because a shell has already done it. Everything else about the
//! query is passed through byte for byte — no uppercasing, no stripping of
//! internal spaces — because `pl_core::iupac::code_mask` uppercases on the way
//! in and `pl_index::scan::Motif::new` refuses anything else by naming the byte.
//! `5'-GAATTC-3'` off a supplier's order form is a refusal with a reason at both
//! surfaces, not a clean empty result at either.

use pl_primer::{Binding, Params, Strand};

use crate::seqedit::Selection;

/// The largest template this panel will scan without leaving the frame.
///
/// [`crate::design::GUI_TEMPLATE_LIMIT`], reused rather than reinvented, and
/// the reason is that a second number would need a second argument. That
/// constant buys exactly what is wanted here — neither window has a worker
/// thread, and a GUI that stops responding is worse than one that says no and
/// names the alternative — and it is already written down and already defended.
///
/// The two scans are different shapes. `find_bindings` walks the template once
/// per strand comparing a `seed_len` window; `pl-design` puts a whole candidate
/// set through `specificity::scan`, which carries its own seed index precisely
/// to avoid the O(candidates x template) its header names. WHICH IS CHEAPER HAS
/// NOT BEEN MEASURED, and this comment will not guess. What is claimed is only
/// that 200 kb covers every plasmid, cosmid and BAC anybody opens in a plasmid
/// editor, and that a bacterial genome belongs on the CLI either way.
///
/// So the refusal names `pl primers`, which has no such limit because it is not
/// holding a frame open.
pub const TEMPLATE_LIMIT: u64 = crate::design::GUI_TEMPLATE_LIMIT;

/// How many binding sites are kept, listed and drawn.
///
/// A cap is needed because the query is a user's paste and `find_bindings`
/// searches for an EXACT seed: `NNNNNNNNNNNNNN` matches at every position of
/// the template on both strands, which on a 200 kb molecule is 400,000
/// `Binding`s, each holding three heap allocations. Nothing about the panel is
/// useful past the first few dozen.
///
/// The cap is never silent — [`Primers::total`] keeps the true count and
/// [`Primers::tally`] prints "412,336 sites: this primer is not specific to one
/// place — the first 500 are listed". `docs/PLAN.md` item 33 is the
/// standing rule here: a hidden site cost a user a month of bench time, so a cap
/// that does not say it capped is worse than no cap.
pub const MAX_SITES: usize = 500;

/// Everything the Primers tab remembers, for ONE document.
///
/// Per tab, and parked in [`crate::bench::DocView`] with the find bar for the
/// same reason the find bar is: `sites` holds coordinates into one molecule,
/// and "3 sites" printed beside a plasmid that has none is a sentence about a
/// different file.
#[derive(Debug, Clone)]
pub struct Primers {
    /// The oligo, as pasted. Trimmed before use, never rewritten in place: a
    /// text box that edits itself under the caret is unusable.
    pub query: String,
    /// 3'-anchored seed length. `--seed`, bounded by [`pl_primer::SEED_MIN`]
    /// and [`pl_primer::SEED_MAX`], defaulted from `Params::default()`.
    pub seed_len: usize,
    /// One mismatch allowed inside the seed, never at the 3' base.
    /// `--seed-mismatch`, off by default, as the CLI's is.
    pub seed_mismatch: bool,
    /// `--exact`: the footprint stops at the first mismatch, which is what
    /// pydna and SnapGene do.
    ///
    /// Stored as the FLAG, not as `extend_mismatches`, so that the checkbox and
    /// the CLI switch mean the same thing when read side by side. The inversion
    /// happens once, in [`Primers::params`], as it does once in `cmd_primers`.
    pub exact: bool,
    /// Monovalent cation, MOLAR.
    ///
    /// Molar and not millimolar, which looks like the wrong unit for a control
    /// labelled "mM" and is the only unit that keeps the default exact.
    /// `Method::default().na_molar` is `50e-3`; `50e-3 * 1e3` is
    /// `50.000000000000007`, and taking that back to molar gives
    /// `0.05000000000000001`, which is not `0.05`. Storing the display unit
    /// would therefore make a panel nobody has touched compute a Tm a few
    /// femtokelvin away from `pl primers`' — enough for the agreement test to
    /// fail on a difference no thermocycler can express. The panel converts for
    /// the widget and writes back only when the widget actually changed.
    pub na_molar: f64,
    /// Total strand concentration, molar. See [`Primers::na_molar`] for the
    /// unit.
    pub oligo_molar: f64,
    /// The sites, at most [`MAX_SITES`] of them, in `find_bindings`' own order.
    pub sites: Vec<Binding>,
    /// How many there really were, which is `sites.len()` unless the cap bit.
    pub total: usize,
    /// Which site the user is looking at, as an index into `sites`.
    pub at: Option<usize>,
    /// Why there is nothing, when there is nothing.
    ///
    /// Every empty result carries one. An empty list with no sentence is
    /// indistinguishable from a search that never ran, and the four ways this
    /// can be empty — no query, a query that is not DNA, a template too big to
    /// scan, a primer shorter than its own seed — want four different actions
    /// from the user.
    pub note: Option<String>,
    /// The question the sites answer, so a redraw does not re-scan. See
    /// [`Primers::key`].
    pub done: Option<Key>,
}

/// What a result is an answer to: the query, every setting that reaches the
/// engine, and which molecule at which version.
///
/// The last two `u64`s are the document's `seq_version` and this window's
/// `doc_generation`; the two before them are the concentrations. The generation
/// is not optional: every document starts at
/// `seq_version` 0, so opening plasmid A and then plasmid B compares equal, the
/// scan is not redone, and A's binding sites are drawn on B — the identical
/// trap `annot::Version` documents, at the identical cost.
///
/// The two concentrations are compared as BITS, because `f64` is not `Eq` and
/// because bit equality is the right question for a memo: two values that
/// differ in the last place are two different questions, however close their
/// answers.
pub type Key = (String, usize, bool, bool, u64, u64, u64, u64);

impl Default for Primers {
    /// The CLI's starting state, derived rather than restated.
    ///
    /// Every field that reaches the engine is read off
    /// `pl_primer::Params::default()`, so a change to the engine's defaults
    /// moves this panel with it instead of leaving a literal behind. That is
    /// not hypothetical tidiness: `seed_len` is 14 here and `--seed`'s help
    /// text says 14, and those are two copies of one number that a third copy
    /// in this file would have made three.
    fn default() -> Self {
        let p = Params::default();
        Primers {
            query: String::new(),
            seed_len: p.seed_len,
            seed_mismatch: p.seed_mismatch,
            exact: !p.extend_mismatches,
            na_molar: p.tm_method.na_molar,
            oligo_molar: p.tm_method.oligo_molar,
            sites: Vec::new(),
            total: 0,
            at: None,
            note: None,
            done: None,
        }
    }
}

impl Primers {
    /// The settings, as the engine takes them.
    ///
    /// The ONE place a [`Params`] is built for this app. Everything the panel
    /// puts a control on comes from the panel; everything else comes from
    /// `Params::default()`, which is the value `cmd_primers` starts from — see
    /// the body for why the fields are listed rather than updated over.
    pub fn params(&self) -> Params {
        // EVERY FIELD OF `Params` IS NAMED, and no `..Default::default()` tail
        // follows them. That is not a style choice: with all four listed, a
        // fifth field added to the engine becomes a compile error right here,
        // and somebody has to decide what this panel does about it. A struct
        // update would have made it a silent default instead — a new engine
        // setting reachable from `pl primers` and from nothing in the app, which
        // is the shape of omission this whole module exists because of.
        Params {
            seed_len: self.seed_len,
            seed_mismatch: self.seed_mismatch,
            // The one inversion, in one place, as `cmd_primers` has it in one
            // place.
            extend_mismatches: !self.exact,
            tm_method: pl_thermo::Method {
                na_molar: self.na_molar,
                oligo_molar: self.oligo_molar,
                // The table and the salt correction come from the ENGINE's
                // default and are deliberately not exposed: `pl primers` has no
                // `--table` or `--salt` either, and a control here that the CLI
                // does not have is a way for the two to answer differently that
                // no agreement test could pin.
                ..Params::default().tm_method
            },
        }
    }

    /// The conditions behind every temperature this panel prints, in
    /// `pl_thermo`'s own words.
    ///
    /// [`pl_thermo::Method::describe`] VERBATIM and never a hand-written "50 mM
    /// Na+", for the reason `seqedit::tm_hover` gives: a number that differs
    /// from another tool's by a degree has to read as a documented modelling
    /// choice rather than as a bug.
    pub fn describe(&self) -> String {
        self.params().tm_method.describe()
    }

    /// The memo key. See [`Key`].
    pub fn key(&self, seq_version: u64, generation: u64) -> Key {
        (
            self.query.clone(),
            self.seed_len,
            self.seed_mismatch,
            self.exact,
            self.na_molar.to_bits(),
            self.oligo_molar.to_bits(),
            seq_version,
            generation,
        )
    }

    /// Find every site, or say why there are none to find.
    ///
    /// Pure: no `Ui`, no document, no clock.
    pub fn search(&mut self, seq: &[u8], circular: bool) {
        self.sites.clear();
        self.total = 0;
        self.at = None;
        self.note = None;
        let q = self.query.trim();
        if q.is_empty() {
            return;
        }
        // Validated through `pl-index`, whose refusals are already written and
        // already tested, and which the find bar already uses for the same
        // paste. "byte 3 is '\'', which is not an IUPAC nucleotide code and can
        // never match" is the sentence a pasted `5'-GAATTC-3'` needs; searching
        // for it and reporting nothing is the silent failure this whole project
        // organises against.
        let motif = match pl_index::scan::Motif::new(q) {
            Ok(m) => m,
            Err(e) => {
                self.note = Some(e.to_string());
                return;
            }
        };
        if seq.is_empty() {
            self.note = Some("this molecule has no bases to anneal to".into());
            return;
        }
        // Before the length check against the seed, because a user on a genome
        // needs to be told about the genome and not about their oligo.
        if seq.len() as u64 > TEMPLATE_LIMIT {
            self.note = Some(format!(
                "{} bp is more than this panel scans in a frame ({} bp). \
                 `pl primers <file> --primer {q}` has no such limit.",
                crate::doc::fmt_int(seq.len() as u64),
                crate::doc::fmt_int(TEMPLATE_LIMIT)
            ));
            return;
        }
        // `find_bindings` returns an EMPTY vector when the primer is shorter
        // than the seed — see `a_seed_longer_than_the_primer_finds_nothing_
        // silently` in pl-primer — and an empty vector here would print as "no
        // binding site", which is a claim about the molecule. It is a fact about
        // the settings, and it names the two ways out.
        if motif.len() < self.seed_len {
            self.note = Some(format!(
                "{} bases is shorter than the {}-base seed, so nothing can match. \
                 Lower the seed, or paste more of the oligo.",
                motif.len(),
                self.seed_len
            ));
            return;
        }
        let mut found = pl_primer::find_bindings(q.as_bytes(), seq, circular, &self.params());
        self.total = found.len();
        if self.total > MAX_SITES {
            found.truncate(MAX_SITES);
        }
        self.sites = found;
        if self.sites.is_empty() {
            // Named on BOTH strands, because a primer is searched on both and a
            // reader who assumes otherwise would go looking for the reverse
            // complement by hand.
            self.note = Some("no binding site on either strand".into());
        } else {
            self.at = Some(0);
        }
    }

    /// "1 site", "2 sites: this primer is not specific to one place", or the
    /// capped form.
    ///
    /// The words after the colon are `cmd_primers`' words, verbatim
    /// (`bins/pl/src/main.rs:4736`). A primer that binds twice is a failed PCR,
    /// and the count is the finding — which is why this is a sentence and not a
    /// number in the corner.
    pub fn tally(&self) -> String {
        match (self.total, self.sites.len()) {
            (0, _) => String::new(),
            (1, _) => "1 site".to_string(),
            (t, s) if t > s => format!(
                "{}: this primer is not specific to one place — the first {} are listed",
                plural_sites(t),
                crate::doc::fmt_int(s as u64)
            ),
            (t, _) => format!(
                "{}: this primer is not specific to one place",
                plural_sites(t)
            ),
        }
    }

    /// The site the user is looking at.
    pub fn current(&self) -> Option<&Binding> {
        self.at.and_then(|i| self.sites.get(i))
    }
}

fn plural_sites(n: usize) -> String {
    format!("{} sites", crate::doc::fmt_int(n as u64))
}

/// Where a binding sits, as a selection.
///
/// The twin of [`crate::find::selection`], and everything that note says
/// applies here for the same reasons. Two things are load-bearing:
///
/// - **A reverse binding is anchored at its 3' end**, which on the plus strand
///   is the LOW coordinate, so `head < anchor`. That is the bit the sequence
///   view's translation lane reads as "reverse", so the residues shown beside a
///   reverse primer are the ones it actually anneals to.
/// - **`through_origin` is read from the binding, never inferred.**
///   `Binding::end < Binding::start` is `pl-primer`'s own spelling for a
///   footprint that crosses the origin, and a pair of carets on a circle names
///   two arcs. Ordering them picks the wrong one: a 20 nt footprint at
///   8,110..12 of an 8,117 bp plasmid read as `[12, 8110]` is 8,098 bases, and
///   a map will draw that without complaint.
///
/// The TAIL is not in the selection, and must not be. A 5' tail does not pair
/// with the template, so it has no coordinates on this molecule at all;
/// selecting it would select template bases the oligo does not touch.
pub fn selection(b: &Binding, n: u64, circular: bool) -> Selection {
    // `end < start` can only be a wrap, and only on a circle. On a line
    // `find_bindings` never produces one, so the `circular` term is a belt on
    // top of braces rather than a behaviour: a linear molecule cannot be told
    // that a selection crosses an origin it does not have.
    let wraps = circular && b.end < b.start;
    let lo = b.start.saturating_sub(1);
    let hi = b.end.min(n);
    let (anchor, head) = match b.strand {
        Strand::Reverse => (hi, lo),
        Strand::Forward => (lo, hi),
    };
    Selection {
        anchor,
        head,
        through_origin: wraps,
    }
}

/// Why there is no Tm for this footprint, in the words that say what to do
/// about it.
///
/// `Binding::tm` is `None` in three cases and they are not interchangeable, so
/// the panel must not render all three as a blank cell. The mismatch case is
/// the one that matters: `pl-thermo` models a perfect duplex and has no
/// internal-mismatch parameters at all, so a number here would be a different
/// question's answer — and, on `pl-primer`'s own worked example, 10 °C HOT
/// (50.5 against 40.2). Ten degrees hot is the direction that runs the anneal
/// step too warm and amplifies nothing.
///
/// The method is passed because the ambiguity and length refusals come from
/// `pl_thermo::tm` itself, and asking it is the only way to name the offending
/// base rather than guess at it.
pub fn tm_refusal(b: &Binding, m: &pl_thermo::Method) -> String {
    if !b.mismatches.is_empty() {
        let n = b.mismatches.len();
        return format!(
            "no Tm: the footprint carries {n} mismatch{} and the model is a perfect duplex — \
             a number here would read about 10 °C hot",
            if n == 1 { "" } else { "es" }
        );
    }
    match pl_thermo::tm(&b.footprint, m) {
        // Not reachable while `find_bindings` and this agree, and stated rather
        // than `unreachable!()`: the two computations are in different crates
        // and a panic in a paint loop is not an acceptable way to find out they
        // have stopped agreeing.
        Ok(t) => format!("Tm {:.1} °C", t.tm),
        Err(pl_thermo::TmError::NotUnambiguous(i, base)) => format!(
            "no Tm: base {} of the footprint is {:?}, which stands for more than one base",
            i + 1,
            base as char
        ),
        Err(pl_thermo::TmError::TooShort) => {
            "no Tm: the footprint is too short for a nearest-neighbour stack".to_string()
        }
        Err(e) => format!("no Tm: {e}"),
    }
}

/// Suggested annealing temperature per polymerase, for ONE binding site.
///
/// Returns `(name, "58C" or "55-58C", vendor note)` per polymerase, or an empty
/// vector when this site has no Tm to advise from.
///
/// # Why per site and not per primer
///
/// `cmd_tm` advises from the lowest Tm across the oligos it was given, because
/// there every oligo is going into the same tube. Here the rows are not a set
/// of oligos, they are the several PLACES one oligo lands, and the temperature
/// a user types into a thermocycler is decided by the site they intend to
/// amplify from — not by the lowest of a set that includes off-target sites
/// they are trying to avoid. Taking the minimum over the rows would quietly
/// advise a Ta chosen by the worst mispriming site on the plasmid.
///
/// # Why `anneal_sized` and not `anneal`
///
/// The length is the FOOTPRINT's, which is what `anneal_sized` documents that it
/// wants: a 5' tail is not templated in the early cycles and does not anneal, so
/// counting it would push an 18-mer past Phusion's "over 20 nt" carve-out on the
/// strength of bases that are not in the duplex. The length-blind `anneal` has
/// that exact failure written into its own doc comment.
pub fn anneal_advice(b: &Binding) -> Vec<(&'static str, String, &'static str)> {
    let Some(tm) = b.tm else { return Vec::new() };
    pl_thermo::POLYMERASES
        .iter()
        .map(|p| {
            let (lo, hi) = pl_thermo::anneal_sized((tm, b.footprint.len()), None, p);
            // `cmd_tm`'s own formatting, so the two surfaces print one number
            // the same way: a range only when there is a range.
            let range = if (lo - hi).abs() < 0.01 {
                format!("{lo:.0} °C")
            } else {
                format!("{lo:.0}-{hi:.0} °C")
            };
            (p.name, range, p.note)
        })
        .collect()
}

/// The primers the open file itself records, as `(name, oligo)`.
///
/// SnapGene `.dna` block 5 is full of these and `pl-fileio`'s reader has always
/// populated `Molecule::primers` from it, so on a `.dna` the commonest oligo a
/// user wants to test is already in the document and retyping it is both work
/// and a chance to mistype.
///
/// # What is deliberately not offered
///
/// A `primer_bind` FEATURE is not a primer here. GenBank has no field for the
/// oligo — `genbank::write` puts the bases in a free-text `/note` as
/// `primer AGCT...; Tm: 62 C` and the reader keeps that note as prose — so
/// offering those would mean parsing a sentence to recover a sequence, and
/// getting it wrong would put an oligo in the box that is not the one in the
/// file. A binding site with no recorded sequence is already drawn on the map
/// as a feature; it does not need to be guessed at here.
///
/// Entries with an empty `seq` are dropped for the same reason: the picker
/// exists to save typing, and an entry that types nothing is a row that does
/// nothing when clicked.
pub fn document_primers(mol: &pl_core::Molecule) -> Vec<(&str, &str)> {
    mol.primers
        .iter()
        .filter(|p| !p.seq.trim().is_empty())
        .map(|p| (p.name.as_str(), p.seq.trim()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(query: &str) -> Primers {
        Primers {
            query: query.to_string(),
            ..Default::default()
        }
    }

    /// A fresh panel asks the engine exactly what `cmd_primers` asks it.
    ///
    /// Field by field against `Params::default()`, which is the value
    /// `bins/pl/src/main.rs:4602` starts from and then modifies only where a
    /// flag was passed. This is the settings half of the agreement; the results
    /// half is `the_panel_and_the_cli_agree_about_the_same_primer_and_molecule`
    /// in `main.rs`.
    ///
    /// PROVEN TO FAIL: setting `seed_mismatch: true` in `Primers::default`
    /// reports `the seed-mismatch default differs from the CLI's`.
    #[test]
    fn a_fresh_panel_holds_the_clis_defaults() {
        let a = Primers::default().params();
        let b = Params::default();
        assert_eq!(
            a.seed_len, b.seed_len,
            "the seed default differs from the CLI's"
        );
        assert_eq!(
            a.seed_mismatch, b.seed_mismatch,
            "the seed-mismatch default differs from the CLI's"
        );
        assert_eq!(
            a.extend_mismatches, b.extend_mismatches,
            "the --exact default differs from the CLI's, and it is the INVERTED flag"
        );
        // Bit equality, not approximate. The panel stores molar rather than the
        // millimolar it displays precisely so that this holds; see
        // `Primers::na_molar`.
        assert_eq!(
            a.tm_method.na_molar.to_bits(),
            b.tm_method.na_molar.to_bits(),
            "the salt default is not bit-identical, so an untouched panel computes a \
             different Tm from `pl primers`"
        );
        assert_eq!(
            a.tm_method.oligo_molar.to_bits(),
            b.tm_method.oligo_molar.to_bits()
        );
        assert_eq!(a.tm_method.table_name, b.tm_method.table_name);
        assert_eq!(a.tm_method.salt, b.tm_method.salt);
        // And the description the panel prints is the description of the
        // settings it will actually use, not of a default it has departed from.
        let hot = Primers {
            na_molar: 0.15,
            ..Default::default()
        };
        assert!(
            hot.describe().contains("150 mM"),
            "the conditions line does not follow the control: {}",
            hot.describe()
        );
    }

    /// The seed control cannot be set to something `pl primers` refuses.
    #[test]
    fn the_seed_control_uses_the_engines_bounds() {
        assert!(
            (pl_primer::SEED_MIN..=pl_primer::SEED_MAX).contains(&Primers::default().seed_len),
            "the panel opens on a seed its own control would refuse"
        );
    }

    /// Both strands, and both sites of a primer that binds twice.
    ///
    /// The whole point of the panel: a primer with two sites is a failed PCR,
    /// and a tool that shows the best one shows nothing useful.
    #[test]
    fn a_primer_that_binds_twice_reports_both_and_says_so() {
        // One perfect forward site, and the same 17 bases again 31 bp later.
        let mut seq = b"TGCACGAATGAGAACAGAACCACAAATGGTG".to_vec();
        seq.extend_from_slice(b"CCTTAGGTCTTAGG");
        seq.extend_from_slice(b"GAATGAGAACAGAACCA");
        let mut p = panel("GAATGAGAACAGAACCA");
        p.search(&seq, false);
        assert_eq!(p.total, 2, "{:?}", p.sites);
        assert_eq!(p.at, Some(0), "nothing was selected for the map to follow");
        let t = p.tally();
        assert!(
            t.contains("2 sites") && t.contains("not specific"),
            "the count is the finding and it is not stated: {t:?}"
        );
        // The control: one site says so without the warning.
        let mut one = panel("CCTTAGGTCTTAGG");
        one.search(&seq, false);
        assert_eq!(one.tally(), "1 site");
    }

    /// A reverse binding reads backwards, so the residues beside it are the
    /// strand the oligo anneals to.
    #[test]
    fn a_reverse_binding_is_anchored_at_its_three_prime_end() {
        let fwd = Binding {
            start: 5,
            end: 24,
            strand: Strand::Forward,
            footprint: vec![b'A'; 20],
            tail: Vec::new(),
            mismatches: Vec::new(),
            tm: None,
        };
        let rev = Binding {
            strand: Strand::Reverse,
            ..fwd.clone()
        };
        let (f, r) = (selection(&fwd, 400, false), selection(&rev, 400, false));
        assert!(f.anchor < f.head);
        assert!(
            r.head < r.anchor,
            "a reverse binding reads forwards, so the translation lane shows the wrong strand"
        );
        assert_eq!(
            (r.head.min(r.anchor), r.head.max(r.anchor)),
            (f.anchor, f.head),
            "the two strands' bindings cover different bases"
        );
        assert_eq!(f.base_count(400), 20, "the selection is not the footprint");
    }

    /// A footprint crossing the origin selects ITS OWN bases, not the other arc.
    ///
    /// PROVEN TO FAIL by hard-coding `through_origin: false` in `selection`:
    /// `base_count` is then `hi - lo` = 4,980, so a 20 nt footprint selects
    /// 4,980 bases of a 5,000 bp plasmid — the COMPLEMENT arc — and both the map
    /// and the sequence view draw that without complaint.
    #[test]
    fn an_origin_crossing_binding_selects_its_own_bases() {
        // 20 bases running 4,995..4,999 then 1..15 of a 5,000 bp circle.
        let b = Binding {
            start: 4_995,
            end: 14,
            strand: Strand::Forward,
            footprint: vec![b'A'; 20],
            tail: Vec::new(),
            mismatches: Vec::new(),
            tm: None,
        };
        let s = selection(&b, 5_000, true);
        assert!(
            s.through_origin,
            "the wrap flag is the only thing that says which of the two arcs is meant"
        );
        assert_eq!(
            s.base_count(5_000),
            20,
            "the selection covers the wrong number of bases"
        );
    }

    /// The engine really does find a site across the origin, and the selection
    /// really does land on it.
    ///
    /// End to end rather than on a hand-built `Binding`, because the coordinate
    /// convention is the thing being tested and a fixture can agree with a bug.
    #[test]
    fn an_origin_crossing_site_is_found_and_selected_whole() {
        // `pl-primer`'s own origin fixture: a perfect 19/19 match to 27..31 +
        // 1..14 of this 31 bp circle.
        let seq = b"CAAATGGTGTGCACGAATGAGAACAGAACCA";
        let mut p = panel("AACCACAAATGGTGTGCAC");
        p.search(seq, true);
        assert_eq!(p.sites.len(), 1, "{:?}", p.sites);
        let b = &p.sites[0];
        assert_eq!(b.start, 27, "the site did not start before the origin");
        assert!(b.end < b.start, "the engine did not report a wrap");
        assert_eq!(
            selection(b, 31, true).base_count(31),
            b.footprint.len() as u64
        );
        // The whole 19 bases anneal, so there is no tail. That is the assertion
        // the origin handling is really about: the five bases BEFORE the origin
        // pair with the template, and calling them a tail is the failure mode.
        assert!(b.tail.is_empty(), "{:?}", b);
        assert_eq!(b.footprint.len(), 19);

        // THE CONTROL, and it is the wrong answer rather than no answer.
        //
        // Read as a line the same molecule still reports a site — the 14 nt seed
        // matches 1..14 — but a DIFFERENT one: it begins at base 1 instead of
        // 27, and the five genuinely-annealing bases before the origin come back
        // as a 5' tail, which is exactly what `pl-primer`'s comment records
        // costing 11 °C of Tm. A control asserting "nothing on a line" would
        // have been false, and would have made this test pass for the wrong
        // reason on the day the origin handling regressed.
        let mut lin = panel("AACCACAAATGGTGTGCAC");
        lin.search(seq, false);
        assert_eq!(lin.sites.len(), 1, "{:?}", lin.sites);
        assert_eq!(lin.sites[0].start, 1);
        assert_eq!(
            lin.sites[0].tail_str(),
            "AACCA",
            "the linear reading should call the pre-origin bases a tail, and it is the \
             circular reading's job not to"
        );
        assert!(
            b.tm.expect("the circular site has a Tm")
                > lin.sites[0].tm.expect("so does the linear one") + 5.0,
            "the whole point: the 19-mer's Tm is well above the bare seed's"
        );
    }

    /// Every empty result says which of the four emptinesses it is.
    ///
    /// They ask for four different actions: type something, fix the paste, use
    /// the CLI, lower the seed. One blank list for all four is the failure this
    /// project is organised against.
    #[test]
    fn each_way_of_finding_nothing_says_which_one_it_was() {
        let seq = b"AAAAGAATTCTTTTTTTTTTTTTTGAATTCAAAA";

        // 1. Nothing typed: no note, because there is no question yet.
        let mut p = panel("   ");
        p.search(seq, false);
        assert!(p.note.is_none() && p.sites.is_empty());

        // 2. Not DNA: the byte is named.
        let mut p = panel("5'-GAATTC-3'");
        p.search(seq, false);
        let n = p.note.clone().expect("a reason");
        assert!(
            n.contains("IUPAC") || n.contains("never match"),
            "a paste off an order form read as an ordinary absence: {n}"
        );

        // 3. Shorter than the seed: `find_bindings` returns empty, and that is
        //    a fact about the settings, not about the molecule.
        let mut p = panel("GAATTC");
        p.search(seq, false);
        let n = p.note.clone().expect("a reason");
        assert!(
            n.contains("shorter than the 14-base seed"),
            "a primer shorter than its own seed read as 'no binding site': {n}"
        );

        // 4. Genuinely absent, and named on BOTH strands.
        let mut p = panel("GGGGGGGGGGGGGGGG");
        p.search(seq, false);
        assert_eq!(p.note.as_deref(), Some("no binding site on either strand"));

        // The control: a query that IS there produces no note at all.
        let mut p = panel("AAAAGAATTCTTTTTT");
        p.search(seq, false);
        assert!(p.note.is_none(), "{:?}", p.note);
        assert_eq!(p.sites.len(), 1);
    }

    /// A template too large to scan in a frame is refused by name, with the
    /// command that does not have the limit.
    #[test]
    fn a_genome_is_refused_with_the_cli_that_can_do_it() {
        let seq = vec![b'A'; TEMPLATE_LIMIT as usize + 1];
        let mut p = panel("GAATTCGAATTCGAATTC");
        p.search(&seq, false);
        let n = p.note.expect("a reason");
        assert!(
            n.contains("pl primers"),
            "the refusal does not name the thing that works: {n}"
        );
        // THE CONTROL: one base fewer and it really scans, and really finds the
        // site. Asserting only "no refusal" would pass against a panel that had
        // stopped searching altogether.
        //
        // A NON-PALINDROMIC primer, which is not fussiness: `GAATTCGAATTCGAATTC`
        // is its own reverse complement — EcoRI's site is, and so is any repeat
        // of it — so it anneals twice at one coordinate, once per strand, and
        // the count would have been 2 for a reason that has nothing to do with
        // the template size this test is about.
        let mut seq = b"GAATGAGAACAGAACCA".to_vec();
        seq.resize(TEMPLATE_LIMIT as usize, b'A');
        let mut p = panel("GAATGAGAACAGAACCA");
        p.search(&seq, false);
        assert!(p.note.is_none(), "{:?}", p.note);
        assert_eq!(
            p.sites.len(),
            1,
            "the largest template it accepts did not scan"
        );
        assert_eq!(p.sites[0].start, 1);
    }

    /// A mismatched footprint gets a refusal that says WHY, never a blank cell.
    ///
    /// The number `pl-thermo` would return for it is about 10 °C hot — the
    /// direction that runs the anneal step too warm and amplifies nothing — so
    /// the absence is a decision and has to read as one.
    #[test]
    fn a_mismatched_footprint_refuses_a_tm_out_loud() {
        // `pl-primer`'s own worked example: a mutagenic primer whose parent
        // reports 47.7 C and which the perfect-duplex model puts at 50.5.
        let seq = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let mut p = panel("GCATGAGAACAGAACCACAA");
        p.search(seq, false);
        let b = p.current().expect("the mutagenic primer still anneals");
        assert!(
            !b.mismatches.is_empty(),
            "the fixture has no mismatch in it"
        );
        assert!(b.tm.is_none(), "a perfect-duplex Tm was reported for it");
        let why = tm_refusal(b, &p.params().tm_method);
        assert!(
            why.contains("mismatch") && why.contains("hot"),
            "the refusal does not say why there is no number: {why}"
        );
        // No advice either: a Ta derived from a Tm that does not exist is the
        // number a user would type into a thermocycler.
        assert!(anneal_advice(b).is_empty());

        // THE CONTROL. Its perfectly-paired parent gets a number and advice,
        // so this test cannot pass by refusing everything.
        let mut ok = panel("GAATGAGAACAGAACCACAA");
        ok.search(seq, false);
        let b = ok.current().expect("the parent anneals");
        assert!(b.mismatches.is_empty() && b.tm.is_some());
        let advice = anneal_advice(b);
        assert_eq!(advice.len(), pl_thermo::POLYMERASES.len());
        assert!(advice.iter().any(|(n, _, _)| *n == "Phusion"));
    }

    /// The advice is measured over the FOOTPRINT, not over the ordered oligo.
    ///
    /// Phusion's rule adds 3 °C only above 20 nt, and a 5' tail is not in the
    /// duplex in the early cycles. Counting the tail would push a 20 nt
    /// footprint over that line on the strength of bases that are not annealed
    /// — the exact failure `pl_thermo::anneal`'s doc comment records for the
    /// length-blind form.
    #[test]
    fn the_annealing_rule_measures_the_footprint_and_not_the_tail() {
        let footprint = vec![b'A'; 20];
        let short = Binding {
            start: 1,
            end: 20,
            strand: Strand::Forward,
            footprint: footprint.clone(),
            // A 15 nt tail: the oligo is 35 nt, the duplex is 20.
            tail: vec![b'G'; 15],
            mismatches: Vec::new(),
            tm: Some(60.0),
        };
        let phusion = anneal_advice(&short)
            .into_iter()
            .find(|(n, _, _)| *n == "Phusion")
            .expect("Phusion is in the table");
        assert_eq!(
            phusion.1, "60 °C",
            "the tail was counted toward the 20 nt carve-out, so the advice is 3 °C hot"
        );
        // The control: one more annealed base and the +3 applies.
        let longer = Binding {
            end: 21,
            footprint: vec![b'A'; 21],
            ..short
        };
        assert_eq!(
            anneal_advice(&longer)
                .into_iter()
                .find(|(n, _, _)| *n == "Phusion")
                .expect("Phusion is in the table")
                .1,
            "63 °C"
        );
    }

    /// The picker offers what the file records and does not invent the rest.
    #[test]
    fn the_document_picker_offers_only_primers_with_a_sequence() {
        let mut mol = pl_core::Molecule {
            name: "p".into(),
            seq: b"AAAAGAATTCTTTT".to_vec(),
            ..Default::default()
        };
        mol.primers.push(pl_core::Primer {
            name: "M13F".into(),
            seq: "GTAAAACGACGGCCAGT".into(),
            ..Default::default()
        });
        // A `.dna` can carry a primer with no bases recorded; clicking it would
        // put nothing in the box.
        mol.primers.push(pl_core::Primer {
            name: "no bases".into(),
            seq: "  ".into(),
            ..Default::default()
        });
        assert_eq!(
            document_primers(&mol),
            vec![("M13F", "GTAAAACGACGGCCAGT")],
            "an unusable row was offered, or a usable one was dropped"
        );
    }

    /// A degenerate paste cannot fill the panel with half a million rows, and
    /// the cap says it capped.
    #[test]
    fn a_cap_that_bites_says_the_true_count() {
        // 14 Ns match at every position, on both strands.
        let seq = vec![b'A'; 2_000];
        let mut p = panel("NNNNNNNNNNNNNN");
        p.search(&seq, false);
        assert_eq!(p.sites.len(), MAX_SITES, "the cap did not bite");
        assert!(p.total > MAX_SITES, "the true count was lost: {}", p.total);
        let t = p.tally();
        assert!(
            t.contains(&crate::doc::fmt_int(p.total as u64)) && t.contains("first"),
            "a silent cap — docs/PLAN.md item 33: {t}"
        );
    }

    /// Two documents at the same version are two questions.
    ///
    /// Every document opens at `seq_version` 0, so the version alone compares
    /// equal across a tab switch and the memo would serve plasmid A's sites for
    /// plasmid B. That is `annot::Version`'s trap, and it costs the same here:
    /// every row would land somewhere plausible and nothing would error.
    #[test]
    fn the_memo_key_separates_two_documents_at_the_same_version() {
        let p = panel("GAATTCGAATTCGA");
        assert_ne!(
            p.key(0, 1),
            p.key(0, 2),
            "the generation is not in the key, so the second file shows the first's sites"
        );
        assert_ne!(
            p.key(0, 1),
            p.key(1, 1),
            "an edit does not invalidate the memo"
        );
        assert_eq!(p.key(3, 4), p.key(3, 4));
        // And every control is in it, or changing one would leave stale rows on
        // screen under new settings.
        let mut q = p.clone();
        q.seed_len += 1;
        assert_ne!(p.key(0, 0), q.key(0, 0), "the seed is not in the memo key");
        let mut q = p.clone();
        q.seed_mismatch = !q.seed_mismatch;
        assert_ne!(
            p.key(0, 0),
            q.key(0, 0),
            "seed-mismatch is not in the memo key"
        );
        let mut q = p.clone();
        q.exact = !q.exact;
        assert_ne!(p.key(0, 0), q.key(0, 0), "--exact is not in the memo key");
        let mut q = p.clone();
        q.na_molar = 0.15;
        assert_ne!(p.key(0, 0), q.key(0, 0), "the salt is not in the memo key");
        let mut q = p.clone();
        q.oligo_molar = 250e-9;
        assert_ne!(p.key(0, 0), q.key(0, 0), "the oligo is not in the memo key");
    }

    /// The panel's temperature is `pl tm`'s temperature — of the FOOTPRINT.
    ///
    /// The bindings half of the parity is
    /// `the_primers_panel_and_the_cli_agree_about_the_same_primer_and_molecule`
    /// in `main.rs`. This is the thermodynamic half, and it is a separate
    /// question: a panel can find exactly the right site and still print a
    /// number from a different model, a different salt or a different length of
    /// oligo, and nothing about the site list would look wrong.
    ///
    /// `cmd_tm` builds its `Method` from `--table 1998` and `--salt santalucia`,
    /// which both resolve to `pl_thermo::Method::default()`, and leaves `--na`
    /// and `--oligo` at the struct defaults. So the expression `pl tm <oligo>`
    /// evaluates is `pl_thermo::tm(oligo, &Method::default())`, and the advice
    /// block under it is `anneal_sized((tm, len), None, p)` per polymerase.
    /// Both are compared here.
    ///
    /// # The fixture is a primer with a TAIL, because that is where the two
    /// questions come apart
    ///
    /// `pl tm` of the ordered 26-mer `AACCGTTCGATGCAACTGGTAACCGT` is 61.1 °C; of
    /// the 20 nt footprint it actually anneals through, 53.9 °C. The panel must
    /// agree with the second. Seven degrees is not a rounding difference, it is
    /// an anneal step run hot enough to amplify nothing — the failure
    /// `pl-primer`'s header opens with.
    ///
    /// # What this CANNOT do
    ///
    /// `bins/pl` is a different crate and `cmd_tm` writes to stdout, so this
    /// pins the model, the conditions and the two engine calls — not the
    /// process. The one difference is deliberate and is asserted rather than
    /// ignored: `cmd_tm` prints `54C` where the panel prints `54 °C`, so the
    /// expected string is the CLI's own with the unit respaced. Comparing the
    /// raw strings would fail on a space and prove nothing about the physics.
    ///
    /// PROVEN TO FAIL by computing the Tm over `b.footprint` plus `b.tail` —
    /// the whole ordered oligo, which is what a tool that does not make the
    /// split reports: `the panel's Tm is not pl tm's of the footprint: 61.1
    /// against 53.9`.
    #[test]
    fn the_panel_and_pl_tm_agree_about_the_footprint_and_not_the_whole_oligo() {
        //                1234567890123456789012345678901234567890
        let seq = b"ACGGTTACCAGTTGCATCGA"; // 20 bp
        let oligo = "AACCGTTCGATGCAACTGGTAACCGT"; // 26 nt: 20 anneal, 6 are tail
        let mut p = panel(oligo);
        p.search(seq, false);
        let b = p.current().expect("the site is real, only over-long");
        assert!(
            b.has_tail(),
            "the fixture has no tail, so it cannot tell the two questions apart"
        );

        // The expression `pl tm <footprint>` evaluates.
        let cli = pl_thermo::tm(&b.footprint, &pl_thermo::Method::default())
            .expect("the footprint is plain ACGT")
            .tm;
        // Bit equality, not approximate: the panel stores molar precisely so
        // that an untouched panel and the CLI are the SAME f64, and an
        // approximate comparison here would hide the femtokelvin drift
        // `Primers::na_molar` exists to prevent.
        assert_eq!(
            b.tm.expect("a perfect footprint keeps its Tm").to_bits(),
            cli.to_bits(),
            "the panel's Tm is not pl tm's of the footprint: {:?} against {cli}",
            b.tm
        );

        // AND IT IS NOT THE WHOLE OLIGO'S, which is the number a tool without
        // the footprint/tail split prints for the same paste.
        let whole = pl_thermo::tm(oligo.as_bytes(), &pl_thermo::Method::default())
            .expect("the oligo is plain ACGT")
            .tm;
        assert!(
            whole > cli + 5.0,
            "the fixture must make the two unmistakable: whole {whole:.1} against footprint \
             {cli:.1}"
        );

        // The annealing advice is `cmd_tm`'s too, polymerase by polymerase.
        let advice = anneal_advice(b);
        assert_eq!(advice.len(), pl_thermo::POLYMERASES.len());
        for (poly, (name, range, _)) in pl_thermo::POLYMERASES.iter().zip(&advice) {
            assert_eq!(*name, poly.name);
            let (lo, hi) = pl_thermo::anneal_sized((cli, b.footprint.len()), None, poly);
            // `cmd_tm`'s own formatting, then its one cosmetic difference from
            // the panel's: the CLI writes `54C`, the panel `54 °C`.
            let cli_says = if (lo - hi).abs() < 0.01 {
                format!("{lo:.0}C")
            } else {
                format!("{lo:.0}-{hi:.0}C")
            };
            assert_eq!(
                *range,
                cli_says.replace('C', " °C"),
                "{name}: the panel advises a different temperature from `pl tm`"
            );
        }
    }

    /// A lowercase paste is the same oligo, and gets the same answer.
    ///
    /// Not a hypothetical paste. Every sequence view in this app renders lower
    /// case where the FILE stores lower case — `reverse_complement` preserves
    /// it deliberately — so copying a primer out of the sequence view and back
    /// into this box is the ordinary way to use the panel, and it arrives
    /// lowercase.
    ///
    /// Nothing uppercases on the way in, and nothing needs to:
    /// `pl_core::iupac::code_mask` calls `to_ascii_uppercase` on both operands,
    /// so `matches` is case-blind, and `pl_thermo::tm` uppercases before
    /// checking the alphabet. This test is what keeps that true, because the
    /// failure would be SILENT — a case-sensitive comparison anywhere in that
    /// chain reports "no binding site on either strand" for a primer that
    /// anneals perfectly, which reads as a fact about the plasmid.
    ///
    /// PROVEN TO FAIL by dropping `.to_ascii_uppercase()` from
    /// `pl_core::iupac::code_mask`, the single line the whole chain rests on: a
    /// lowercase byte then falls to that match's `_ => 0` arm, and a mask of
    /// zero matches nothing. `a lowercase paste found 0 sites where the same
    /// oligo in capitals found 1`.
    #[test]
    fn a_lowercase_oligo_finds_the_same_sites_at_the_same_temperature() {
        let seq = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let mut upper = panel("GAATGAGAACAGAACCA");
        upper.search(seq, false);
        let mut lower = panel("gaatgagaacagaacca");
        lower.search(seq, false);

        assert_eq!(
            upper.sites.len(),
            1,
            "the fixture must have a site, or this compares two empty lists"
        );
        assert_eq!(
            lower.sites.len(),
            upper.sites.len(),
            "a lowercase paste found {} sites where the same oligo in capitals found {}",
            lower.sites.len(),
            upper.sites.len()
        );
        let (a, b) = (&upper.sites[0], &lower.sites[0]);
        assert_eq!((a.start, a.end, a.strand), (b.start, b.end, b.strand));
        assert_eq!(a.tm, b.tm, "the same duplex melted at two temperatures");
        // The footprint keeps the case it was PASTED in, which is right: it is
        // a slice of the user's own oligo, and rewriting it would make the row
        // stop matching what is in the box above it.
        assert_eq!(b.footprint_str(), "gaatgagaacagaacca");
        assert_eq!(
            b.footprint_str().to_ascii_uppercase(),
            a.footprint_str(),
            "the two footprints are not the same bases"
        );

        // And a lowercase TEMPLATE, which is the other half of the same paste:
        // a `.dna` written by a tool that lower-cases its sequence.
        let mut soft = panel("GAATGAGAACAGAACCA");
        soft.search(&seq.to_ascii_lowercase(), false);
        assert_eq!(soft.sites.len(), 1, "{:?}", soft.note);
        assert_eq!(soft.sites[0].start, a.start);
    }

    /// An ambiguity code anneals, and then refuses a Tm by naming the base.
    ///
    /// A degenerate primer is ordinary — a library, a codon-randomised position
    /// — and `matches` is asymmetric precisely so that a pattern `R` pairs with
    /// a template `A`. So the SITE is real and must be reported. The
    /// temperature is not: `pl_thermo` has no stacking parameters for an
    /// ambiguity code and refuses with `NotUnambiguous`, and dropping the base
    /// instead would report the Tm of a different, shorter oligo — a number
    /// that looks entirely reasonable.
    ///
    /// This is the arm of [`tm_refusal`] that nothing else reaches. The
    /// mismatch arm is covered by
    /// `a_mismatched_footprint_refuses_a_tm_out_loud`, and the two produce
    /// different sentences on purpose: one says lower the stringency, the other
    /// says this base cannot have a temperature.
    ///
    /// # Why the explicit arm earns its place over the catch-all
    ///
    /// `TmError`'s own `Display` is not bad — it already names the position and
    /// says the base "has no stacking parameters" — so the argument for a
    /// separate arm has to be more than tone, and it is: **the index is into
    /// the FOOTPRINT**, and the catch-all does not say so. `pl_thermo::tm` is
    /// handed `b.footprint`, so on this panel's own fixture with a 6 nt tail,
    /// "base 3" is base 9 of the oligo in the box and base 3 of nothing the
    /// user can see. `seqedit::tm_clause` faces the identical problem from the
    /// other side and solves it the other way, translating the index into a
    /// molecule coordinate. Naming the coordinate system is the whole content
    /// of the arm.
    ///
    /// PROVEN TO FAIL with the `NotUnambiguous` arm deleted so the case falls
    /// through to `Err(e) => format!("no Tm: {e}")`: `the refusal does not say
    /// which coordinate "base 3" counts in: no Tm: base 3 is 'R', which has no
    /// stacking parameters; a Tm over the rest would be a different oligo's`.
    #[test]
    fn an_ambiguous_base_anneals_but_refuses_a_temperature_by_name() {
        let seq = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        // `R` is A-or-G at position 3; the template has an A there, so it pairs.
        // It sits OUTSIDE the 14-base 3' seed, so the seed is still exact.
        let mut p = panel("GARTGAGAACAGAACCA");
        p.search(seq, false);
        let b = p.current().expect("an ambiguity code still anneals");
        assert_eq!(b.start, 6, "{b:?}");
        assert_eq!(
            b.footprint.len(),
            17,
            "the ambiguous base was split off as tail instead of pairing"
        );
        assert!(
            b.mismatches.is_empty(),
            "an ambiguity code that matches is not a mismatch"
        );
        assert!(b.tm.is_none(), "a stacking parameter was invented for R");

        let why = tm_refusal(b, &p.params().tm_method);
        assert!(
            why.contains("base 3"),
            "the refusal does not locate the offending base: {why}"
        );
        // The coordinate system, not just the number. With a tail in the box
        // "base 3" is base 3 of nothing the user can see unless this says so.
        assert!(
            why.contains("of the footprint"),
            "the refusal does not say which coordinate \"base 3\" counts in: {why}"
        );
        assert!(
            why.contains("more than one base"),
            "the refusal does not say the base is degenerate: {why}"
        );
        assert!(
            !why.contains("mismatch"),
            "an ambiguity code was reported as a mismatch, which asks for the wrong fix: {why}"
        );
        // No advice either, for the same reason a mismatched footprint gets
        // none: a Ta derived from a Tm that does not exist is a number a user
        // would type into a thermocycler.
        assert!(anneal_advice(b).is_empty());

        // THE CONTROL. The unambiguous parent of the same oligo gets both, so
        // this cannot pass by refusing everything.
        let mut ok = panel("GAATGAGAACAGAACCA");
        ok.search(seq, false);
        let c = ok.current().expect("the parent anneals");
        assert!(c.tm.is_some() && !anneal_advice(c).is_empty());
    }

    /// An oligo longer than the molecule pairs once round and no further.
    ///
    /// The case is real on a small molecule — a 60 nt Gibson oligo against a
    /// 40 bp annealed cassette — and it is the one where a coordinate can leave
    /// the molecule entirely. That matters HERE rather than only in the engine
    /// because two painters consume these numbers: `selection` turns them into
    /// carets, and the map filters on `start >= 1 && start <= n`. A footprint
    /// reported longer than the template would put a caret past the end of the
    /// sequence and an arc more than once round the ring.
    ///
    /// `pl-primer`'s own `a_primer_longer_than_the_circle_does_not_pair_the_
    /// same_bases_twice` pins the engine; this pins what the panel then hands
    /// the painters.
    ///
    /// BOTH TOPOLOGIES, and the circular one is the half that can fail. On a
    /// line `find_bindings` walks the template itself, so a footprint cannot
    /// outrun it; on a circle it walks a DOUBLED buffer, and only an explicit
    /// one-turn clamp stops a 26-mer pairing with bases it has already
    /// consumed. Closing a molecule must not lengthen a footprint, so the two
    /// answers are also asserted equal to each other — a site that does not
    /// wrap cannot depend on the topology flag.
    ///
    /// PROVEN TO FAIL by dropping `.min(n)` from the reverse branch's `avail`
    /// in `find_bindings`, which is the defect that engine test was written
    /// for. The linear case is untouched by it — `.min(n)` is inert when `ext`
    /// IS the template — and the circular case reports a 26 nt footprint on a
    /// 20 bp molecule with the tail gone: `circular: a 26 nt footprint on a
    /// 20 bp molecule pairs bases twice`.
    #[test]
    fn an_oligo_longer_than_the_molecule_keeps_its_coordinates_on_it() {
        let seq = b"ACGGTTACCAGTTGCATCGA"; // 20 bp
        let n = seq.len() as u64;
        let oligo = "AACCGTTCGATGCAACTGGTAACCGT"; // 26 nt: 20 pair, 6 are a second turn
        let mut answers = Vec::new();
        for circular in [false, true] {
            let what = if circular { "circular" } else { "linear" };
            let mut p = panel(oligo);
            p.search(seq, circular);
            assert!(!p.sites.is_empty(), "{what}: {:?}", p.note);
            for b in &p.sites {
                assert!(
                    b.footprint.len() as u64 <= n,
                    "{what}: a {} nt footprint on a {n} bp molecule pairs bases twice",
                    b.footprint.len()
                );
                assert_eq!(
                    b.footprint.len() + b.tail.len(),
                    oligo.len(),
                    "{what}: the oligo is not accounted for — every base is footprint or tail"
                );
                // The coordinates the painters are handed are ON the molecule.
                assert!(
                    (1..=n).contains(&b.start) && (1..=n).contains(&b.end),
                    "{what}: {b:?} would draw off the end of a {n} bp molecule"
                );
                // And the selection covers the annealed bases, not the ordered
                // ones. `base_count` is where a span longer than the molecule
                // stops being arithmetic and starts being a caret past the end.
                assert_eq!(
                    selection(b, n, circular).base_count(n),
                    b.footprint.len() as u64,
                    "{what}: the selection is not the footprint"
                );
            }
            // The six bases of a second turn really are reported as tail, not
            // quietly dropped: an oligo whose length nothing accounts for is how
            // a user comes to order the wrong thing.
            let b = p.current().expect("a site");
            assert_eq!(b.tail_str(), "AACCGT", "{what}");
            assert_eq!(b.footprint_str(), "TCGATGCAACTGGTAACCGT", "{what}");
            answers.push(p.sites.clone());
        }
        assert_eq!(
            answers[0], answers[1],
            "closing the molecule changed a site that does not wrap"
        );
    }

    /// The salt control moves the number, in the direction and by the amount
    /// the CLI's own help promises.
    ///
    /// `pl design --tm`'s help says the default is "ON THIS MODEL'S 50 mM Na+
    /// SCALE, where an ordinary PCR buffer sits about 5 C higher". A control
    /// that did not reach the engine would leave the two Tms identical, which is
    /// exactly what a control wired to nothing looks like.
    #[test]
    fn the_salt_control_reaches_the_engine() {
        let seq = b"TGCACGAATGAGAACAGAACCACAAATGGTG";
        let mut low = panel("GAATGAGAACAGAACCACAA");
        low.search(seq, false);
        let mut high = panel("GAATGAGAACAGAACCACAA");
        high.na_molar = 0.15;
        high.search(seq, false);
        let (a, b) = (
            low.current().and_then(|b| b.tm).expect("a Tm at 50 mM"),
            high.current().and_then(|b| b.tm).expect("a Tm at 150 mM"),
        );
        assert!(
            b > a + 3.0 && b < a + 8.0,
            "150 mM Na+ should sit about 5 °C above 50 mM: {a:.1} then {b:.1}"
        );
    }
}
