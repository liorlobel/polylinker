# Polylinker
### A planning document for a free, open, offline plasmid editor

**Status:** proposal / architecture decision record · **Date:** 2026-07-26 · **Audience:** the person who is going to build this, and whoever funds them

> Throughout this document, claims that could not be verified against a primary source — or that the review pass contradicted — are tagged **[VERIFY]** with the specific experiment or fetch needed. Do not quote a **[VERIFY]** claim in a grant application, a paper, or a README until it is closed.

---

## Table of contents

1. [Executive summary](#1-executive-summary)
2. [Landscape](#2-landscape)
3. [Product definition](#3-product-definition)
4. [Feature roadmap](#4-feature-roadmap)
5. [Architecture](#5-architecture)
6. [File-format interop](#6-file-format-interop)
7. [The science](#7-the-science)
8. [Data and licensing](#8-data-and-licensing)
9. [Legal guardrails](#9-legal-guardrails)
10. [Risks](#10-risks)
11. [Sustainability](#11-sustainability)
12. [The first two weeks](#12-the-first-two-weeks)
13. [Appendix A — naming](#appendix-a--naming)
14. [Appendix B — decisions log](#appendix-b--decisions-log)

---

## 1. Executive summary

### Should this be built?

**Yes — but not the thing the research asked for.**

The research set out to plan a SnapGene clone. Three months of desk research and two review passes converge on a conclusion the original framing did not anticipate: *a SnapGene clone is the least valuable and most expensive thing in the opportunity space.* SnapGene is genuinely good, costs $350/yr academic (and as little as ~$135–205/seat at institutions that negotiate **[VERIFY: Columbia/WashU figures came from search snippets, not primary sources]**), and gives away a free Viewer forever plus free course licences. Competing on "same thing but free" is how ApE, Serial Cloner, UGENE, GENtle and ove-electron all lost. ove-electron shipped complete three-platform packaging and accumulated roughly 1,600 installs in three and a half years.

Meanwhile, three artifacts that *do not exist anywhere* are each individually more valuable than a clone, each is publishable, each is adoptable by the incumbents, and each makes the app cheaper to build:

1. **An openly-licensed, provenance-tracked common-features annotation database.** This does not exist. Every open annotator in the field — pLannotate, and therefore anything downstream of it — depends on a ~1,367-row CSV **[VERIFY: the dossier states both 1,367 and ~1,600; count the file]** scraped from SnapGene's proprietary Common Features list in 2021, whose Description column is SnapGene's curated prose verbatim, redistributed with no licence from GSL Biotech. It is the single clearest IP exposure in the ecosystem and simultaneously the single most-used dataset. Replacing it is the highest-leverage contribution available to anyone in this field.
2. **A rotation- and strand-invariant correctness benchmark for cloning operations.** Every proposed QA strategy in the research compares *representations* — byte diffs, GenBank diffs, parser cross-checks — when the thing that matters is whether two tools produced the same *molecule*. For a circular double-stranded plasmid, rotation, strand choice, annotation order and feature-name spelling are all free. `cdseguid` (from the `seguid` package, MIT, by the pydna author) gives a rotation- and complement-invariant checksum. A public truth set of `(inputs, operation, expected cdseguid)` triples — digests, ligations, Gibson junctions, Golden Gate overhang sets, PCR products, origin-spanning edits, frame arithmetic — is citable, is runnable by SnapGene and Benchling and pydna and UGENE alike, and is the only way to earn the sentence "we are correct" instead of "we are free."
3. **A permissively-licensed, framework-agnostic circular plasmid map renderer.** The visible quality signal in this entire product category. No open component does it at SnapGene's standard. The research called this "comparatively cheap"; the review correctly called that wrong — automatic non-overlapping label placement with leader lines is the hardest layout problem in the product. It has to be built either way, so build it as a standalone package that seqviz, plascad, OpenCloning and pLannotate can all adopt.

So: build the app, but build it *last*, on top of three things that stand alone. If the app dies of maintainer attrition — which is how every project in this space has died — the three artifacts survive and the field is permanently better off. That is the only version of this project with a non-catastrophic failure mode.

### Build vs. fork verdict

**Compose. Do not greenfield, do not hard-fork.**

| Candidate | Verdict | Why |
|---|---|---|
| Greenfield everything | **No** | 3–5 years. There is no version of this where hand-writing a sequence-editor virtualization layer, an overhang-aware assembly kernel and an ABIF parser is a good use of the first year. |
| Hard-fork `@teselagen/ove` | **No** | Inherits React 18 as a *hard* (not peer) dependency, `@blueprintjs/core` pinned to 3.54.0 (March 2022), untranspiled ES source as `main` (698 files, 22.8 MB), and a published `package.json` with no `license` field that automated scanners will flag `UNLICENSED`. Forking means owning a Blueprint-6 migration nobody asked for. |
| Fork/join **OpenCloning** | **No, but collaborate** | MIT, active, and its maintainer co-maintains pydna and commits upstream to tg-oss — but it is a 4-star, single-maintainer, web-only academic app with 67 open issues, no desktop story, no `.dna` support, no project layer, and the same primer3-py GPL-2.0 contamination. Take pydna. Study the wiring. Offer to co-maintain. Do not model the schedule on "it's already built." |
| Fork **UGENE** | **No** | GPL-2.0, C++/Qt, general bioinformatics workbench. Wrong licence, wrong shape, wrong decade of UI. |
| Fork **plascad** | **No** | 138 stars, one person, 269 commits, cannot write `.dna`, no macOS binary. Excellent Rust reference for the block enum and the in-tree AB1 module; not a base. |
| **Compose** | **Yes** | Consume `@teselagen/ove` for the row/sequence view. Vendor + port `sgffp`/`autosnapgene` for the `.dna` codec. Port pydna's `Dseq` + assembly graph to Rust with pydna as the CI oracle. Build our own circular renderer and our own annotation database. Contribute the fixes upstream. |

The one hard fork worth making is of the **`.dna` codec**, and even there the move is *vendor and fix*, not *depend*.

### The pitch, one paragraph

**Polylinker is a free, offline plasmid editor that opens your lab's actual files — including SnapGene `.dna` — annotates them from a database whose every entry cites its source, and never sends a sequence anywhere.** It ships the first openly-licensed common-features library in the field, so its annotations are auditable rather than magic; it publishes a public correctness benchmark that any tool can run, so its answers are checkable rather than trusted; and it renders publication-grade circular maps to vector SVG and PDF identically on every platform. It is Apache-2.0, has a real Python and command-line API, runs with the network cable unplugged, and requires no account, no administrator rights, and no subscription. Where it cannot be perfectly faithful — writing SnapGene's history trees, matching an unpublished melting-temperature parameter set — it says so, loudly, in the UI, instead of guessing.

---

## 2. Landscape

### 2.1 The honest table

| Tool | Licence | Alive? | Desktop | Offline | Edits | Cloning sim | Reads .dna | Writes .dna | Auto-annot | Pub. export | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **SnapGene 8.2.2** | Proprietary ($350–1,845/yr) | Yes, ~quarterly | Yes | Yes | Yes | 8 modules | Native | Native | Yes (curated, closed) | SVG/PDF/EMF | The incumbent. Good. |
| **SnapGene Viewer mode** | Freeware | Yes | Yes | Yes | **No** | **No** | Yes | No | **No** | Yes | Universal reader. This is the funnel. |
| **Benchling free academic** | Proprietary SaaS | Yes | No | **No** | Yes | Yes | Import | No | Yes | Limited | *The real competitor for the free tier.* Cloud-only; export portability is the documented weak point. |
| **ApE 3.1.9** | Proprietary freeware, no redistribution of modified source | Yes (2026-02) | Yes | Yes | Yes | Partial | **No** | No | Basic | Weak | Most-used free desktop editor. Cannot be forked. Public Tcl mirror is ~4.5 yr stale. |
| **UGENE 53.1** | GPL-2.0 | Yes (2026-03) | Yes | Yes | Yes | Partial | No | No | No | Weak | Real OSS, real maintenance, wrong shape. Copyleft blocks reuse in a permissive project. |
| **@teselagen/ove 0.8.42** | MIT | Maintained, decelerating (last commit 2026-05-22) | Component only | n/a | Yes | **No** | Yes | No | Hook only | SVG-ish | Best OSS editor *engine*. Not a product. React 18 hard dep. |
| **seqviz 3.10.24** | MIT | Very active | Component | n/a | **No** | No | Yes | No | No | SVG | Clean read-only viewer. Wrong category. |
| **OpenCloning** | MIT | Active, 1 maintainer | **No** | **No** | Yes | **16 methods** | No | No | No | No | Strongest cloning-workflow entrant. Complementary, not competitive. |
| **plascad** | MIT (Rust) | 2025-12 | Yes | Yes | Yes | Partial | Yes | **Documented failure** | Basic | Basic | One person. Good reference. |
| **SpliceCraft 1.2.39** | MIT (but bundles GPL-2 primer3-py) | Very active (2026-07) | TUI | Yes | Yes | MoClo/GB | No | No | HMM/BLAST | No | Terminal-only. Great idea source. Same licence conflict. |
| **pLannotate 2.0.0** | GPL-3.0 | Tagged 2024-07 | No (web/CLI) | Sort of | No | No | No | No | **Yes, the reference pipeline** | Bokeh | Algorithm is the value. DB is 2020–21 vintage *and* legally tainted. |
| **sgffp 0.19.1** | MIT | Active (2026-06) | Library | n/a | n/a | n/a | Yes | **Claims yes** | n/a | n/a | See §2.3 — the headline claim is contradicted by its own issue tracker. |
| **PlasmidStudio** | Proprietary freemium SaaS | Active | No | No | Yes | Yes | Yes | **Yes** | AI | PDF | The funded closed competitor already occupying the "modern alternative" slot. |
| **Serial Cloner / Vector NTI / GENtle** | — | **Dead** | — | — | — | — | — | — | — | — | Vector NTI's 2019 death created SnapGene's opening. No OSS tool captured it. |

### 2.2 Why none of it suffices

Sort the table by two axes — *is it a product a bench scientist installs?* and *is it open?* — and the cells are almost disjoint. The open things (OVE, seqviz, DnaFeaturesViewer, pydna, pLannotate) are components used by bioinformaticians. The product-shaped things (SnapGene, ApE, Benchling, PlasmidStudio) are closed. UGENE is the sole open product, and it is a Qt bioinformatics workbench, not a cloning-first editor.

Tellingly, the highest-starred project in the whole survey is **DnaFeaturesViewer** — a matplotlib plotting library — at 694 stars against OVE's ~100 and tg-oss's 100. Open-source attention in this field flows to programmable components used by people who write code, not to GUI applications used by people who hold pipettes. That is exactly why no product emerges, and it is a structural fact this plan has to design around rather than wish away: **the library tier is where contributors come from, and the app tier is where users come from, and they are different populations.** A plan that only serves the second gets no contributors; a plan that only serves the first gets no users. Polylinker's answer is to make the library tier a genuinely first-class, separately-published, separately-citable deliverable — not an internal implementation detail of a GUI.

### 2.3 The wedge everyone chose is already claimed — and broken

Three of the seven research areas nominate "write `.dna`" as the flagship differentiator, on the strength of the finding that no open-source tool can do it. A fourth area reports that **sgffp** (MIT, v0.19.1, 2026-06-07) shipped a full reader *and* writer for `.dna`/`.rna`/`.prot` covering all 17 block types, "round-trip compatible with SnapGene 8.2.2."

Both are wrong, in opposite directions, and the correction changes the strategy:

- **sgffp exists**, so "nobody can write `.dna`" is false. The wedge is claimed.
- **sgffp does not work**, so "it's solved" is also false. Its own issue tracker, ten days before the research was compiled, carries **issue #19: "History block present but not parsed and lost upon saving (enzymes also disappear)"** and **issue #18: a primer coordinate off-by-one on round-trip**. Those are precisely the two failure modes the research itself designates as non-negotiable: silent destruction of provenance, and coordinate drift. Its README's actual claim — "SnapGene normalizes sgffp-written files without rewriting bytes" — is weaker and vaguer than the paraphrase, and it describes no test corpus. Issue #1 is a standing crowdsourced request for users to send in dumps of unknown block types.

**Conclusion: `.dna` write is table stakes, not a moat.** It must be built, it must be built better than sgffp, and it will take 6–10 weeks to a common-case writer plus 4–8 months before history and traces round-trip losslessly — an estimate anchored by the fact that a motivated solo developer spent roughly six months and 163 commits on nothing but this format and still loses data. But it is not the thing that makes anyone switch. The moat is the annotation database, the benchmark, and the map.

**[VERIFY — highest-priority experiment in this entire document]** Write a `.dna` file with sgffp. Open it in the free SnapGene Viewer. Re-export it. Diff. This is an afternoon, it has never been run by anyone, and it determines whether two-way SnapGene compatibility is achievable at all. PlasCAD's README documents that emitting structurally valid blocks is *not* sufficient — SnapGene rejects its output. Nobody knows whether sgffp's is accepted.

### 2.4 Which one to join instead

Not one, but three, in parallel with building:

- **pydna** (BSD-3, active, `pydna-group/pydna`) — take as the correctness oracle and port target. Do not reimplement overhang-aware assembly from scratch; that is a documented multi-year trap.
- **tg-oss** — contribute the `jsonToSnapgene` writer that genuinely does not exist in JavaScript anywhere, and (funding permitting) the React-19/Blueprint-6 migration. That single contribution is, per the research's own assessment, "the highest-leverage thing an outside team could do."
- **sgffp** — offer co-maintainership rather than competing. Its author already filed the port-to-JS proposal upstream at tg-oss (issue #278, unanswered). He is the natural collaborator on the codec.

---

## 3. Product definition

### 3.1 Who it is for

**Primary persona — "the third-year grad student on a locked-down PC."** Runs 5–20 cloning projects a year, mostly restriction and Gibson/HiFi. Receives `.dna` files by email from the postdoc down the hall and from the core facility. Sends constructs for Plasmidsaurus-style whole-plasmid sequencing and needs to see whether the read matches. Cannot install software requiring administrator rights. Puts plasmid maps in a thesis. Has no budget line and will not ask the PI for one. This person is on SnapGene Viewer mode, meaning **they cannot edit a sequence, run an alignment, simulate a digest, or customise an enzyme set.** The bar to beat for them is very low; the bar to beat *paid* SnapGene is the real target and comes later.

**Secondary persona — "the computational PI."** Wants a library API, wants to script 200 constructs, wants annotations with provenance they can put in a methods section, wants the tool in a Nextflow step. SnapGene's entire programmatic surface is two CLI commands (format conversion, map rendering) with no API, no plugin system, no scripting. "Limited API" is a named community complaint. This person is a *contributor*, not just a user, and serving them is how the project gets a bus factor above one.

**Tertiary — "the course instructor."** SnapGene gives course licences away free, precisely because teaching is the acquisition funnel. It is also the one segment a free tool can win outright and permanently: a class of 40 students who learn Polylinker will carry it into 40 labs. Zero-friction install, offline operation, and a good tutorial matter more here than any feature.

**Explicitly not for (v1):** pharma enterprise (Benchling won that seat; the blocker there is procurement, validation and 21 CFR Part 11, not features), genome-scale/BAC work, and anyone who needs real-time multi-user collaboration.

### 3.2 The v1 scope that earns adoption

The scope is set by the research's own ranked workflow frequencies, which put "open a map someone sent you," "micro-lookups," and "verify a clone" far above "simulate cloning" — even though the last is what everyone demos. **[VERIFY: this ranking is explicitly labelled inference in the source research, triangulated from what SnapGene made free rather than from user data. Validate with 20–30 structured interviews before locking the roadmap — see §12, Day 1.]**

v1.0 ships exactly this:

1. Open and render `.dna`, `.prot`, GenBank, FASTA, GFF3/BED, `.ab1`, FASTQ — fast, correctly, offline.
2. A circular and linear map at or above SnapGene's visual quality, exported as clean layered SVG and PDF, byte-identically on every platform.

   > **Both shapes are real, as of 2026-08-07.** This line was half true for months and read as though it were whole: `pl-draw` had one figure, and a linear molecule — every FASTA, every assembly, every PCR product and every gBlock — exported as a C-shaped ring with a gap in it. The gap was an honest statement about topology and the wrong picture. `crates/pl-draw/src/linear.rs` is the horizontal track, built as the same `Scene` out of the same three primitives, so no writer changed; `Options::shape` defaults to asking the molecule, and every circular figure this project had already produced is byte-identical across the change (measured over 126 SVG/PDF/EPS/PNG/scene combinations, not asserted). Byte-identity across runs is checked in two processes as well as in one, by the gate step named *the same molecule twice, from two processes*.
   >
   > **What this item still does not cover, and should not be read as covering.** This item is about `pl-draw`, and `pl-draw` is one of **four** things in this tree that draw a plasmid map. `packages/circular-map` (TypeScript, standalone) is circular only — `LINEAR_GAP` at `src/render.ts:83` maps a linear molecule onto the ring and leaves a gap where its two free ends are — so that is the renderer with no track, and it is the one that still draws a line as a gapped ring. The desktop app's on-screen map (`bins/pl-gui/src/map.rs`) paints straight to egui: since the export learned the track, it and `pl-draw` agree about the *shape* of a linear molecule, and they still disagree about lanes, about where a feature's name goes, about how many ruler ticks there are, and about which end of the last base a feature box stops at. The fourth is the browser prototype's, and it is **its own**: `prototype/dna-reader.template.html` carries a ~230-line renderer inline, `renderMap` at :605 branching to `renderCircular`:615 and `renderLinear`:769, and it has drawn a linear molecule as a horizontal track since `d20056b` (2026-07-26) — driven in jsdom on a 420 bp linear record it produces no `<circle>` at all, one horizontal axis at `y=96`, and the caption `linfrag.gb · 420 bp linear`. It loads nothing out of `packages/`: the template has no `<script src>`, no `import` and no fetch, and `tools/build-web.ps1` substitutes `{{WASM_BASE64}}`, `{{DEMO_GENBANK}}` and `{{BUILD_STAMP}}` and nothing else. *This paragraph said the prototype's map **was** the TypeScript renderer, from `14250f2` (2026-08-07) until 2026-08-14, and README's component table carried the same claim from the same commit.* It was false when it was written — `d20056b` is an ancestor of `14250f2`, so the prototype had stopped drawing linear molecules as rings twelve days earlier — and understating a capability was the harmless half of it: the harmful half is that naming the wrong file sends whoever is fixing a prototype map bug to code the page never loads, and hides the fourth renderer behind the third, which is exactly the count this paragraph exists to state. **And the four are barely held to one another.** Of the six pairs, two are checked. `crates/pl-draw/tests/agreement.rs` replays a fixture that `tools/gen-agreement.mjs` generates from the TypeScript, and two gate steps regenerate it and re-run it so it cannot go stale — but the check is helper-level: it never builds a `Molecule`, never calls `scene`, and never compares an arc, a radius, an arrowhead or a label column, as `crates/pl-draw/src/lib.rs` says of itself. The app's on-screen map is held to the exported figure only at the quantities that have already gone wrong once — the label anchor (`mid_base`) and the arrowhead count. The prototype's renderer is checked against **nothing**: `prototype/check_page.js` asserts only that the map has at least eight paths and no `NaN` in its geometry, never which of the two branches ran, and no step of `tools/ci.ps1` invokes it. "Layered" is also a claim about grouping that the SVG does not yet make: it emits ordered elements with `<title>`, not named layer groups an illustrator can toggle.
3. Sequence editing with inline features, ORF finding, six-frame translation, non-standard genetic codes and non-ATG starts.
4. Restriction site display with **loud, unmissable** enzyme-set filtering, unique-cutter emphasis, methylation blocking, and an always-visible "*N additional cut sites are hidden by the current enzyme set*" badge.
5. Auto-annotation from the Polylinker Features Database, on open/paste, offline, in under 200 ms for a 10 kb plasmid — with every call showing its source, accession, licence and percent identity. **Measured, and it holds: 11 ms and 103 ms** for the two shapes of 10 kb circular plasmid, release build, on the development machine.

   > **The number, measured.** `crates/pl-features/tests/budget.rs` builds two 10 kb circular plasmids out of real records from the shipped table and times `Annotator::annotate` on each: a *dense* one carrying about 37 short parts, and a *large* one carrying four multi-kilobase CDSs. Release build, median of five: **11 ms dense, 103 ms large.** Debug: 106 ms and 1,075 ms. The k-mer index build, excluded because both callers pay it once per process, is a further 6 ms release / 30 ms debug.
   >
   > Which of the two is the expensive one was not guessable, and the first guess was wrong: the *large* plasmid, with a tenth as many hits, costs nine times as much, because the cost is the aligner and `pl-features::align` is a plain dynamic program whose cost is the product of the two lengths. A budget checked only against the dense plasmid would have reported a tenth of the real figure and passed with room to spare. Both are timed on every test run now. A debug build is deliberately not held to the budget: this document describes what a user's machine does, and nobody ships an unoptimised build.
   >
   > This line said "under 200 ms" with nothing measuring it. That is exactly how `rust-version` sat at a wrong `1.82` for months.
6. Primer binding-site detection with SantaLucia NN Tm, plus a real primer-design engine.
7. Simulate PCR and restriction digest; render on a calibrated virtual gel.
8. Restriction cloning + homology-overlap assembly (Gibson/HiFi class) + Type IIS assembly, all non-destructively (a new document, source untouched).
9. Sanger `.ab1` align-to-reference with discrepancy calling, **and** circular-aware whole-plasmid consensus alignment with arbitrary origin offset.
10. Write GenBank losslessly enough that SnapGene reimports with features, types and colours intact; write `.dna` for the common case with byte-preserving passthrough and an explicit, visible report of anything at risk.
11. A local project library: folders, full-text and motif search across thousands of files, recent files, OS file associations, bulk folder import.
12. A Rust library, a CLI, a PyO3 Python wheel, and an MCP server — all over the same engine.

### 3.3 Explicit non-goals

Written down so they can be pointed at when someone asks.

| Non-goal | Rationale |
|---|---|
| **Cloud, accounts, sync, real-time collaboration** | The one durable axis against Benchling is local-first data sovereignty. Users say this explicitly and repeatedly. A cloud tier forfeits the disaffected-SnapGene market to win a market Benchling already owns. Also: contradicts "unfunded." |
| **Telemetry of any kind** | Opt-in, symbolicated, self-hosted crash reporting only (GlitchTip). Nothing else, ever. This is a marketing claim and it must be literally true. |
| **CRISPR guide design in v1** | Genuine SnapGene gap and loudest Benchling delta, but: absent from the top-10 workflows; genome-wide off-target enumeration needs gigabytes of per-organism, versioned reference genome, which breaks the zero-admin offline install; and the permissive scoring stack is weak (Azimuth BSD-3 but archived 2024-06-17 and Python 2; rs3 Apache-2.0 but pinned to scikit-learn ≤1.0.2 and numpy ≤1.26.4; CRISPOR excluded outright). Shipping half-working guide scoring in a tool whose only asset is trust is a bad first bet. → v2, as an opt-in plugin with a downloaded index. |
| **Gateway, TOPO, In-Fusion, TA/GC** | Trademarked chemistries, long-tail usage. Restriction + overlap + Type IIS covers the overwhelming majority of real assembly. |
| **The 23 legacy vendor import formats** | Marketing surface area. Xdna and GCK are cheap wins because Biopython already reads them and *writes* Xdna; the rest are not. |
| **Multiple sequence alignment with four interchangeable engines** | Bundle MAFFT (BSD-3) only, in v2. MUSCLE 5 and Clustal Omega are copyleft; "conda-installable" is irrelevant to a notarized `.app`. |
| **A paid tier, hosted SaaS, or enterprise edition** | See §9. Commercial substitution, not cloning, is what turned *SAS v. WPL* into a 13-year war. Revenue comes from grants, sponsorship and support contracts — not from a closed tier. |
| **AI features at runtime** | The product's only asset is deterministic, reproducible, checkable answers. A plausible hallucinated annotation is exactly the silent-wrong-answer failure mode that kills the project, delivered at scale. AI is used **offline in the database build pipeline** (triage, draft descriptions, cross-checks) under the rule *AI may propose, never assert; nothing AI-derived ships without provenance and a human sign-off*, and is exposed **as an MCP tool surface** so agents can drive the deterministic engine. |
| **BAC / genome scale** | 200 kb is a stress case for CI budgets, not a target market. A BAC editor and a plasmid editor are different products; committing to both doubles the rendering architecture. |

---

## 4. Feature roadmap

Sizing assumes **one competent full-time developer**, and includes design, tests, docs and review — not just typing. Where the source research gave an estimate, the estimate here is deliberately larger; the review pass found the research roughly 4× optimistic overall, and a 4× optimistic schedule is the documented mechanism by which every project in this space died.

### Pre-v0: the three standalone artifacts (weeks 1–14, overlapping)

These ship before the app and are separately published, separately licensed, separately citable.

| Item | Size | Justification |
|---|---|---|
| **`polylinker-bench` v0.1** — CC0 truth set: 200 `(input, operation, expected cdseguid)` triples covering digest, ligation, PCR, Gibson junction, Type IIS overhang sets, origin-spanning edits, frame arithmetic | **3 weeks** | Nothing else in this plan can be called correct without it. Runnable by any tool. Citable. Costs nothing to build alongside the engine because you need the cases anyway. |
| **`polylinker-features` v0.1** — 800–1,200 curated features from UniProt/Swiss-Prot, Rfam 15.1, FPbase, NCBI UniVec 10.0, Barrick Table S1, with per-entry `source_db / accession / licence / curator / date / doi` | **8 weeks initial, then permanent** | The one thing in the field that does not exist. The one place where copying creates real legal exposure. The flagship. See §8.3. |
| **`@polylinker/circular-map` v0.1** — framework-agnostic TS, SVG out, arrowed multi-segment features, leader-line label placement, collision avoidance, zoom tiers | **6 weeks** (of an eventual 12–16) | The visible quality signal. Has to be built regardless; building it standalone makes it a gift to seqviz/plascad/OpenCloning and forces a clean API. |

### v0.1 — "it opens my file and draws it" (target: week 14)

| Feature | Size | Justification |
|---|---|---|
| Tauri v2 shell, React 18, project scaffold, CI on 3 platforms | 2 wk | Foundation. React 18 pin is forced by OVE; see ADR-3. |
| `.dna`/`.prot` **reader** in Rust (17 block types, passthrough for unknown) | 2 wk | Reading is well-trodden; four independent implementations agree on the layout. |
| GenBank + FASTA read/write | 1.5 wk | GenBank is the canonical internal interchange format. Multiline qualifiers and `join()` are where the bugs are. |
| Sequence model + op log + undo/redo | 2 wk | Cannot be retrofitted. See §5.4. |
| Circular + linear map render (v0.1 of `@polylinker/circular-map`) | (above) | — |
| Restriction site search + enzyme sets + **hidden-sites badge** | 2 wk | Workflow #3. The badge is the direct fix for the single documented case of SnapGene costing a user a month of bench time. |
| SVG export via serialized DOM → `resvg`/`svg2pdf` in Rust | 1 wk | Engine-independent, CI-diffable, strictly better than Chromium `printToPDF`. Never use `html2canvas`. |
| Bulk folder import + **rebuildable** index + search | 2 wk | The actual switching cost is a shared drive with 3,000 `.dna` files. **SQLite rejected — see ADR-11.** |
| **Demo:** open a real Addgene `.dna`, draw the map, list cut sites, export SVG, save GenBank | — | — |

### v1.0 — "I can stop paying" (target: months 12–15)

| Feature | Size | Justification |
|---|---|---|
| Sequence editing, ORFs, 6-frame translation, codon tables 1–33, non-ATG starts | 4 wk | Everything the free Viewer cannot do. Bacterial/mitochondrial work needs the tables. |
| Auto-annotation engine (k-mer index + `edlib`/WFA2 verify, ≥96% identity, indel-tolerant, 1–2 terminal-codon slack) | 4 wk | **Critical scoping call:** SnapGene's "magic moment" is *approximate string matching against a curated library*, not homology search. That is single-digit milliseconds in WASM. Do **not** build the pLannotate BLAST+/DIAMOND/Infernal stack for this — see below. |
| "Deep annotation" opt-in tier (Swiss-Prot DIAMOND + Rfam Infernal), optional sidecar or remote | 6 wk, **deferrable to v2** | Genuine long-tail value (catches codon-optimised ORFs, ncRNA). Off the critical path. Requires 3 native binaries × 4 platform targets, each separately signed for notarization, plus a multi-hundred-MB DB — a 4–6 month platform-engineering workstream wearing a checkbox. Not on file-open. |
| `.dna` **writer**, common case, byte-preserving passthrough, coordinate-taint warnings | 8 wk | See §6. Do not believe 3–6 weeks. |
| Primer binding-site detection + SantaLucia NN Tm + per-polymerase Ta advisor | 3 wk | Report Tm, never bake buffer corrections into it. See §7.2. |
| Primer design engine (own implementation on MIT Tm code) | 6 wk | Bundling primer3-py is **not** "closing the gap cheaply" — it relicenses the whole distribution GPL-2.0 and may make it undistributable alongside Apache-2.0 components. See §8.4. |
| Restriction cloning + linear ligation | 3 wk | — |
| `pl-clone`: Rust port of pydna `Dseq` + overlap-graph assembler (Gibson/HiFi + Type IIS) | 8 wk | Resolves the Python-in-Tauri packaging problem. pydna becomes the CI oracle, not the runtime. See ADR-5. |
| Type IIS / Golden Gate with overhang-set fidelity checking | 3 wk | Palindromic, duplicate and single-mismatch-neighbour overhangs must be flagged (Potapov/Pryor, PLOS ONE 2020). |
| Agarose gel simulation (calibrated monotone spline) | 3 wk | Refuses to predict supercoiled/uncut migration — even MacVector documents that nothing does this. |
| `.ab1` reader + chromatogram render (Canvas) + align-to-reference + discrepancy calling | 6 wk | Workflow #3. Canvas, not SVG, for the trace. |
| Circular-aware whole-plasmid ONT/Plasmidsaurus alignment | 3 wk | Whole-plasmid sequencing has largely displaced Sanger for verification, and rotation normalization is genuinely hard to retrofit. |
| Non-destructive simulation + graphical history (own DAG, not SnapGene's) | 4 wk | An HN commenter flagged destructive PCR simulation as "unexpected" within days of a competing tool's launch. |
| Publication export: SVG, PDF, **EPS**, PNG/TIFF at specified physical width and dpi, journal presets | 2 wk | EPS is a free win — SnapGene does not offer it and JCB and Cell Press both accept it. |
| Accessibility pass: Okabe–Ito palette, redundant non-colour encoding, WCAG 2.2 AA, full keyboard operation, navigate-by-feature screen-reader model | 4 wk | ~1 in 12 men cannot separate a red AmpR arrow from a green CDS, and "unique cutters are bold" is currently the only non-colour channel in the entire category's design language. Also a soft procurement blocker (VPAT / EN 301 549). |
| Autosave, crash recovery, file-association *conflict* handling, cloud-drive concurrent-edit detection | 3 wk | Losing an afternoon of annotation is a one-strike uninstall. Silently stealing the `.dna` association from an installed SnapGene will enrage exactly the user you are courting — **ask, don't take**. |
| Docs (tutorial / how-to / reference / explanation), shipped **offline in-app**, plus a methods page per computation | 4 wk | A local-first tool whose help requires internet contradicts itself. The methods pages double as the answer to "why is your Tm different from SnapGene's." |
| ~~Signing, notarization~~, updater, ~~Flathub, winget, Homebrew Cask~~ | ~~8 wk + permanent~~ | See §10. **Withdrawn 2026-08-06.** Signing and notarisation are struck because they are not planned work — not deferred, not blocked on money, not waiting on an entity that could hold a certificate. The builds are unsigned and stay unsigned. That is a settled decision rather than a gap, and it has a price the user pays: SmartScreen shows *"Windows protected your PC"* on first run, Gatekeeper refuses a downloaded macOS binary outright, and the SHA-256 manifest that stands in for a signature proves the bytes arrived intact and nothing about who produced them. All of that, plus the `xattr -d com.apple.quarantine` remedy, is stated in full in `docs/RELEASING.md`, in the three shipped readmes and in the release-notes template; `tools/ci.ps1` holds them to it; **none of that text is affected by this row and none of it should be removed.** The three channels go with the certificate: Flathub, winget and Homebrew Cask each want a signed, packaged artifact, none was ever built, and none is planned. What ships instead is done — on Windows a zip, a readable per-user PowerShell installer (`tools/installer/`) and an MSI; on macOS and Linux a tarball whose readme names the quarantine flag and the glibc floor. `tools/release.ps1` still accepts `-WindowsCert` and calls `signtool`, so the hook outlives the plan if anyone ever reverses this. **The updater is not withdrawn.** The *auto*-updater is cancelled, and that has not changed — but an opt-in update check exists and is shipped: `crates/pl-update`, reached from `pl update` and from a box in the editor's Help menu that ships off. It verifies an Ed25519 signature made by a key compiled into the running binary, and it installs nothing; the four conditions it had to meet are in `docs/RELEASING.md`. (This row read "~~updater~~ … cancelled, not pending" from 2026-08-05 to 2026-08-06, which was true for a day.) |
| `pl-cli`, `pl-py` (PyO3 wheel), `pl-mcp` | 3 wk | Nearly free once the library is factored. Reaches the population that produces contributors. SnapGene's programmatic surface is two commands; the MCP space is empty. |

**Honest v1.0 total: 12–15 months for one developer, 8–10 for two.** Not 4–6.

### v2.0 — "it does things SnapGene can't"

| Feature | Size | Justification |
|---|---|---|
| CRISPR: PAM scan (origin-wrapping), CFD + MIT/Hsu off-target, Rule Set 3 on-target, downloaded genome index | 8 wk | Largest capability gap vs. Benchling. Plugin, not core. |
| Deep annotation tier (if deferred from v1) | 6 wk | — |
| MSA via bundled MAFFT (BSD-3) | 4 wk | — |
| Codon optimization via DNA Chisel (MIT), three strategies + CAI + %MinMax | 3 wk | Naive CAI maximisation is a known way to make expression *worse*; users need harmonisation. |
| Secondary structure (`seqfold`, MIT — **not** ViennaRNA, whose licence forbids redistribution for a fee) | 3 wk | — |
| Silent restriction-site add/remove in a CDS | 2 wk | Genuinely useful, non-obvious, absent from most free tools. |
| **Optional biosecurity screen before synthesis export** (IBBIS Common Mechanism, `commec`, MIT) | 4 wk | See §10, risk 8. First plasmid editor with one. Opens a funding channel. |
| Protein documents, disulfide/interchain bonds, custom numbering | 4 wk | — |
| i18n rollout (strings externalised from commit 1 regardless) | 3 wk | Largest under-served populations are not anglophone; teaching market is global. |

---

## 5. Architecture

### 5.1 The stack decision

> **ADR-1 — Tauri v2 shell + Vite + React 18 + TypeScript frontend + Rust compute core.**

**Rationale.**

> **SUPERSEDED, 2026-08-05 — the app is not Tauri.** `bins/pl-gui` is eframe/egui
> with no webview (`README.md`: "one static binary, no webview"), so everything
> below about a Tauri shell, its plugins, its `signCommand` and its
> "free, signature-mandatory auto-updater" describes a stack that was evaluated
> and not built. `docs/RELEASING.md` is the authority on releasing, signing and
> updating, and it records that there is deliberately **no auto-updater at all**.
> This section is kept because it is the record of how the decision was reached,
> not a description of what exists.
>
> **Amended 2026-08-06.** "No auto-updater at all" is still what `RELEASING.md`
> records, and is still exact: nothing runs on a timer and nothing installs
> itself. What has changed is that the *signature* half of the sentence above
> was eventually built, by hand and on this project's own terms —
> `crates/pl-update` verifies an Ed25519 signature over the release manifest
> against a key compiled into the running binary, and hands back a path for a
> person to run. It is reached from `pl update` and from a Help-menu box that
> ships off. So the row above is wrong about Tauri being the only way to get a
> mandatory update signature, and right that this project did not take it.

1. **The UI decision is forced, not chosen.** The only reusable open-source plasmid-editor UI in existence — OVE and seqviz — is React + SVG. Rewriting a virtualized sequence editor, circular map layout and chromatogram rendering in Qt or Swift is multi-person-year work producing nothing a user can see. Qt/PySide6 is licensing-fine and strategically wrong; BeeWare/Toga was still building primitives as of January 2026. Web frontend it is, which reduces the decision to *shell only*.
2. **Tauri v2 is mature.** `tauri` 2.11.5 (2026-07-01), plugins shipping weekly, MIT/Apache-2.0, Commons Conservancy governance. It gives real filesystem access, OS file associations for `.dna`/`.gb`, native menus, and a **free, signature-mandatory auto-updater** that Electron matches only via third-party tooling — and which works even while Windows builds are still unsigned.
3. **The Rust burden is opt-in.** The `fs`/`dialog`/`store`/`updater` plugins are prebuilt; the default template needs essentially no Rust. Contributors can start with `npm run dev`.
4. **Rust pays a multiplier nothing else does.** The same crate compiles to a native library for the desktop app, to `wasm32` for a browser viewer, to a CLI binary, and to a PyO3 wheel for the bioinformatics audience. One correctness implementation, four distribution surfaces.
5. **Electron's 8-week Chromium cadence with a 3-release (~6-month) support window** is a permanent, recurring maintenance tax on an unfunded project: bump every 8 weeks or ship a browser with known CVEs.

**The strongest counter-argument, stated honestly.**

> Choose Electron 43 instead if cross-platform rendering *identity* is a correctness requirement — and it arguably is, because the entire user-visible value of this product is a plasmid map that goes into a figure.
>
> Tauri means four engines: WebView2 (Chromium) on Windows, WKWebView on macOS (frozen at the user's OS version), Android WebView, and **WebKitGTK 2.36+ on Linux**. Tauri ships a dedicated *Linux Graphics Issues* troubleshooting page covering blank/white windows, flicker on resize, and crashes on resize (most often NVIDIA); WebKitGTK renders fonts noticeably bolder than other engines and masks the WebGL renderer string. Electron gives you exactly one Chromium on all three desktops, making "the map renders differently on my Ubuntu box" structurally impossible.
>
> The size argument that normally rebuts this is weak precisely where it is advertised: Tauri's own docs state the Linux AppImage grows "from the 2–6 MB range to 70+ MB" once WebKitGTK is bundled — within 30% of Electron.

**The rebuttal, and why Tauri still wins.** Neutralise the rendering-identity argument by making the *deliverable* engine-independent: never use the browser's print/PDF pipeline. Serialize the live SVG DOM and rasterize/convert it in Rust with `resvg` (PNG) and `svg2pdf` (PDF). Output is byte-identical on Windows, macOS and Linux regardless of webview, and is testable as a golden-file artifact in CI. Then pin Linux to **Flathub** (GNOME 46 runtime carries every Tauri dependency, so no bundled WebKitGTK *and* a pinned engine version) and add CI screenshot-diffs on Ubuntu 22.04 and Fedora. What remains is on-screen cosmetic divergence, not figure divergence.

**Pivot condition, written down in advance:** if the first three field bug reports are Linux rendering, switch to Electron. The frontend code ports unchanged. This is not embarrassing and should not be treated as a defeat.

### 5.2 Module breakdown

```
polylinker/
├─ crates/
│  ├─ pl-core        Sequence model, coordinate arithmetic, op log,
│  │                 seguid checksums, IUPAC, genetic codes.  NO I/O.
│  ├─ pl-enzymes     REBASE ingest, cut arithmetic, methylation blocking,
│  │                 enzyme sets.  Depends on the separate data package.
│  ├─ pl-fileio      genbank | fasta | fastq | gff3 | bed | xdna
│  │                 snapgene (.dna/.rna/.prot) | abif (.ab1)
│  ├─ pl-align       edlib + WFA2 bindings; seed-and-extend anchoring;
│  │                 circular/rotation-normalized; Sanger discrepancy calling.
│  ├─ pl-annotate    k-mer seed index, approximate verification,
│  │                 DB loader + provenance propagation.
│  ├─ pl-clone       Dseq (watson/crick/ovhg), overlap graph, path & cycle
│  │                 enumeration.  Port of pydna, validated against pydna.
│  ├─ pl-thermo      SantaLucia NN Tm, salt corrections, end stability,
│  │                 hairpin/dimer (seqfold port).
│  ├─ pl-gel         Calibrated mobility spline, band profile.
│  ├─ pl-export      resvg / svg2pdf rasterization + vector conversion.
│  └─ pl-ffi         PyO3 wheel + wasm-bindgen + C ABI.
├─ packages/
│  ├─ @polylinker/circular-map   Framework-agnostic TS. SVG out. Standalone-published.
│  ├─ @polylinker/linear-view    Thin wrapper over @teselagen/ove RowView.
│  └─ @polylinker/app            React 18 + Tauri. The product.
├─ bins/
│  ├─ pl              CLI
│  └─ pl-mcp          MCP server over the library
└─ data/              (separate repos, separate licences)
   ├─ polylinker-features   CC BY 4.0
   ├─ polylinker-bench      CC0
   └─ polylinker-rebase     REBASE bespoke grant + NOTICE
```

**Hard rule:** `pl-core` has no I/O and no dependencies on anything above it. Everything that could ever be wrong about a molecule lives in crates that the CLI can exercise without a GUI. If the app is abandoned, the crates remain useful.

**Hard rule:** the app must build and run with `npm run dev` against a `wasm32` build of the core, with **no Rust toolchain installed**. Contributors must be able to fix a UI bug without learning Rust. Otherwise the contributor pipeline dies, which is the documented cause of death in this field.

### 5.3 Data model for a sequence document

```rust
struct SequenceDocument {
    id:            Ulid,
    kind:          Kind,                 // Dna | Rna | Protein
    topology:      Topology,             // Linear | Circular
    strandedness:  Strandedness,         // Single | Double
    seq:           Vec<u8>,              // ASCII, IUPAC, uppercase, no gaps
    methylation:   Methylation,          // { dam, dcm, ecoki }: bool
    genetic_code:  u8,                   // NCBI transl_table, default 1
    features:      Vec<Feature>,
    primers:       Vec<Primer>,
    colors:        Vec<ColorOverride>,
    notes:         Vec<Note>,            // key + text + attrs, in file order
    provenance:    Provenance,           // { op_log_head, parents: Vec<Ulid> }
    foreign:       ForeignBlocks,        // see §6.3
    checksums:     Checksums,            // cdseguid | ldseguid + sha256(seq)
}

struct Feature {
    id:          Ulid,
    name:        String,
    kind:        FeatureKind,            // SO term where one exists + free string
    segments:    Vec<Segment>,           // >1 == join(): introns, fusions
    strand:      Directionality,         // None | Fwd | Rev | Bidirectional
    qualifiers:  Vec<(String, Vec<QVal>)>,
    origin:      FeatureOrigin,          // see below — this is the differentiator
}

struct Segment { start: u32, length: u32, kind: SegKind, color: Option<Rgb> }

struct FeatureOrigin {
    source:      Source,      // Manual | Imported{format} | AutoAnnotated{..}
    db_entry:    Option<String>,   // polylinker-features stable ID
    db_version:  Option<String>,   // MUST be stamped — see §5.3.3
    accession:   Option<String>,   // UniProt / Rfam / FPbase / GenBank acc
    licence:     Option<String>,   // SPDX or short name
    identity:    Option<f32>,      // 0.0–1.0, as matched
    coverage:    Option<f32>,      // fraction of the DB feature covered
    curator:     Option<String>,
    date:        Option<Date>,
    doi:         Option<String>,
}
```

**5.3.1 Coordinates — the single most important decision in the model.**

Segments are `{ start: u32, length: u32 }`, **0-based, with position `i` of the segment resolving to `(start + i) mod L`.**

This is deliberately *not* GenBank's 1-based inclusive `join()`, *not* SnapGene's `"start-end"` range strings, and *not* the half-open `[start, end)` that most code reaches for. The reason: on a circular molecule, `[start, end)` cannot represent an origin-spanning interval without either a sentinel, a sign convention, or a two-segment split — and every one of those is a place where an off-by-one hides. With `{start, length}`, wrapping is free, rotation is `start = (start + k) mod L`, and **you cannot construct an invalid interval.** Rotation invariance of the restriction-site set becomes a one-line property test rather than a special case.

Conversion happens only at the format boundary, in exactly four functions, each with exhaustive round-trip property tests:

- GenBank ↔ internal (1-based inclusive; `join()`; `complement()`; origin-spanning `join(4500..5000,1..200)`)
- SnapGene XML ↔ internal (1-based inclusive `"start-end"` strings; **plus** the documented Biopython primer quirk of an extra ±1 beyond the normal conversion; **plus** the ×3 multiplier for protein feature coordinates)
- GFF3/BED ↔ internal (GFF3 1-based inclusive, BED 0-based half-open — two different conventions in adjacent code paths, a classic bug site)
- ABIF ↔ internal

> **AMENDMENT (2026-07-26) — the implementation uses 1-based inclusive
> `{start, end}`, not `{start, length}`. Decided, not drifted.**
>
> The argument above still stands on its merits and is not being disputed. It
> was overruled for one reason the original text does not weigh: **both formats
> this tool actually reads are already 1-based inclusive.** SnapGene's
> `<Segment range="a-b">` and GenBank's `4500..5000` are the same convention,
> so choosing it internally makes two of the four boundary conversions the
> *identity function*. §5.3.1 asks for the conversions to be confined to four
> places; this reduces two of them to nothing at all, and an off-by-one cannot
> hide in a function that does not exist.
>
> Origin-spanning intervals, the case that motivates `{start, length}`, are
> represented the way both source formats represent them: as a feature with two
> segments. That machinery is needed regardless — GenBank `join()` exists for
> introns and fusions, not only for the origin — so the wrapping case reuses it
> rather than requiring a second mechanism.
>
> **What this costs, stated plainly.** `{start, length}` makes an invalid
> interval *unrepresentable*. `{start, end}` does not: `end < start`, `start == 0`
> and `end > len` are all constructible. That safety is not free and is not
> obtained by choosing the other convention here — so it is bought explicitly
> with [`Molecule::validate`], which every reader and every operation runs
> against. Rotation is likewise a remap rather than one line of modular
> arithmetic, and is covered by the rotation-identity property test on real
> plasmids.
>
> Reopen this only with evidence: a coordinate bug that the other
> representation would have made impossible.

**5.3.2 Sequence storage.** Plain `Vec<u8>` ASCII. Not bit-packed. A 200 kb BAC is 200 kB; bit-packing saves nothing that matters and breaks IUPAC ambiguity codes, which a plasmid editor needs constantly. Reconsider only if profiling says so.

**Note on PDF export.** The plan says "SVG export via serialized DOM →
`resvg`/`svg2pdf`". What shipped renders one device-independent `Scene` twice —
`pl_draw::scene` → SVG or PDF — rather than converting one output into the
other. Those crates convert *someone else's* SVG; we generate the drawing, so
going through SVG would mean serialising a picture, parsing it back, and taking
a font-shaping stack to do it. Rendering the scene directly is a few hundred
lines and no dependencies, and it is the only arrangement in which the two
outputs cannot drift, because there is nothing above the level of ink for them
to disagree about. Text uses Helvetica, one of the fourteen fonts every PDF
viewer must provide, so nothing is embedded and no font licence is involved;
the cost is WinAnsi, and any name that loses a character is reported rather
than silently written with `?`.

**5.3.3 Annotation provenance is not optional metadata — it is the product.** Every auto-annotated feature carries the database version that produced it. Two collaborators with different database versions will otherwise get different annotations for the same file and will blame the tool. Stamped `db_version` also makes the annotation reproducible and therefore citable in a methods section, which is the differentiator over SnapGene's closed, undocumented, complained-about-as-duplicated library.

### 5.4 Undo/redo, and why it is the same mechanism as history

> **ADR-2 — History is an append-only, content-addressed op log with lazy materialization. Undo/redo is a cursor into it. There is no separate "history feature."**

SnapGene's graphical history is its signature capability *and* its documented performance liability — users are advised in community threads to delete history trees to stop memory bloat, and the r/labrats report of unbounded memory growth on macOS points at nested embedded sequence blobs.

```
Op = { id: Ulid, parent: Option<Ulid>, kind: OpKind, payload: …, ts, actor }

OpKind = InsertAt | DeleteRange | ReplaceRange | SetTopology | Rotate
       | ReverseComplement | SetFeature | RemoveFeature | SetMethylation
       | Digest{enzymes} | Ligate{parts} | Pcr{template,primers}
       | Assemble{method,parts} | Mutagenize{…} | Annotate{db_version}
```

Properties:

- **Append-only.** Nothing is ever mutated. Undo moves a cursor; redo moves it back; a new edit after undo forks the DAG (it does not truncate — SnapGene users lose work to truncating undo stacks in every other editor).
- **Content-addressed.** Each op and each materialized document version has a hash. Two labs that perform the same digest on the same plasmid get the same hash, which makes the history *comparable* across machines, which makes it a provenance record rather than a UI affordance.
- **Lazily materialized.** Only the current document and a snapshot every N=50 ops live in memory. Seeking to an arbitrary history node replays from the nearest snapshot. A 200 kb document with 2,000 ops costs ~40 snapshots ≈ 8 MB, not 400 MB.
- **Serialized as JSON Patch deltas**, never state snapshots. Snapshot-per-keystroke on a 200 kb sequence is untenable.
- **Forward-compatible with collaboration without being a CRDT now.** If real-time editing ever becomes a requirement, the op log replays into Loro (Rust core, matching our Rust side, movable-tree and rich-text types that handle coordinate-anchored marks correctly) or Yjs. **Do not adopt a CRDT now.** A naive JSON CRDT merging two edits to a sequence will happily produce a construct whose feature coordinates no longer match its bases — a silent corruption, not a conflict.

### 5.5 File I/O flow

```
bytes ──► format sniffer ──► format-specific parser ──► IR (foreign-faithful)
                                                          │
                                       validation + coordinate normalization
                                                          │
                                                          ▼
                                                  SequenceDocument
                                                          │
              ┌───────────────────────────────────────────┼──────────────┐
              ▼                                           ▼              ▼
        GenBank writer (canonical)              .dna writer        SVG/PDF export
```

**GenBank is the canonical internal interchange format and the default save format.** It is the only format with bidirectional support across Biopython, `@teselagen/bio-parsers`, PlasCAD, GenomicAnnotations.jl and SnapGene itself. `.dna` is an import/export *adapter*, which means a SnapGene format change degrades one feature instead of breaking the product.

Parsing is **defensive by policy**, because a `.dna` file is untrusted binary that arrives by email:

- Attacker-controlled 4-byte big-endian length fields never drive an allocation directly. Cap per-block allocation; reject implausible lengths with an error rather than a resize.
- Hard caps on decompressed size for LZMA (blocks 0x07, 0x1D, 0x1E), zlib (0x17) and BGZF (0x1B). Decompression bombs are the obvious attack.
- XML parsed with entity expansion and external entities **disabled**. Blocks 0x05, 0x06, 0x08, 0x0A, 0x0E, 0x11, 0x14 are all XML.
- `cargo-fuzz` on the parser as a memory-safety *and* resource-exhaustion target, in CI, from week 3.

### 5.6 Rendering

| Layer | Technology | Why |
|---|---|---|
| Circular map | **SVG**, our own renderer | Cost tracks visible glyph count, not bp. Interactive, hit-testable, and *is* the exported figure. |
| Linear/row view | **SVG**, virtualized via `@teselagen/ove` RowView + `@teselagen/react-list` | Decade-refined, low differentiation, high effort. Consume it. Re-enable the row cache their authors wrote and commented out. |
| Base letters when zoomed out | **Canvas overlay** | Never emit one `<text>` node per base. This is the SVG cliff. |
| Chromatograms, coverage, quality tracks | **Canvas overlay** | Dense per-pixel data. |
| WebGL | **Never** | Buys nothing at plasmid scale, degrades text, breaks SVG export, and WebKitGTK masks the renderer string with measurably worse latency. |

**Two non-negotiable invariants:**

1. Never one `<text>` per base. Batch into a single `<text>` with `letter-spacing`, or draw to Canvas.
2. When annotations are pared down for performance, **show a visible "N features hidden" badge.** OVE's `pareDownAnnotations()` caps at roughly 50 items per layer and silently drops the rest. On a dense construct that is not a performance strategy, it is a *correctness* failure — the user sees an incomplete map and has no idea.

**Performance budgets, asserted in CI:** open a 200 kb GenBank with 500 features in <1.5 s; circular map first paint <300 ms; linear scroll at 60 fps with <2,000 live SVG nodes. Test against a real BAC checked into the repo as a fixture, not a synthetic random sequence.

**[VERIFY]** Nobody has ever measured OVE on a real 200 kb BAC with ~500 features. All performance claims in the source research are architectural inference from reading source. This is a one-afternoon spike (load a BAC into the tg-oss demo) and it gates the largest technical assumption in the stack. Run it in week 1.

---

## 6. File-format interop

### 6.1 The format, as settled

Four independent implementations (Biopython, autosnapgene, PlasCAD, `@teselagen/bio-parsers`) agree, so this is not in doubt:

- Flat stream of TLV blocks: **1-byte type + 4-byte big-endian `u32` length + payload**. No file header, no index, no file-level compression.
- Every file opens with block `0x09` (cookie), 14-byte payload = `"SnapGene"` + three big-endian `u16` (`seq_type`, `export_version`, `import_version`). Literally: `09 00 00 00 0E 53 6E 61 70 47 65 6E 65`.
- Block `0x00` = DNA: one flag byte (`0x01` circular, `0x02` double-stranded, `0x04` Dam, `0x08` Dcm, `0x10` EcoKI) then raw ASCII. Block `0x15` (21) = protein; block `0x20` (32) = RNA.
- Features `0x0A`, primers `0x05`, notes `0x06`, alignable sequences `0x11` are UTF-8 XML with 1-based inclusive `"start-end"` range strings and a `directionality` attribute (0/1/2/3).
- **Note the discrepancy:** the Incenp prose says "the second bit is the Topology flag"; all four implementations mask `0x01` (bit 0). **Trust the code.** Confirm against a hex dump of a known-circular plasmid in week 1 and record the answer.

### 6.2 Read plan

Full parse for: `0x09` cookie, `0x00` DNA, `0x15` protein, `0x20` RNA, `0x0A` features, `0x05` primers, `0x06` notes, `0x11`/`0x10`/`0x12` alignments and traces, `0x0E` custom enzyme sets, `0x14` colours.

Byte-preserving passthrough for everything else, including `0x01` (compressed DNA — decoded by nobody, and if a version ever emits sequence there, every open reader silently returns an *empty* sequence rather than erroring: a data-corruption-shaped failure, not a crash), `0x03`, `0x07`/`0x0B`/`0x1D`/`0x1E` (history, LZMA+XML), `0x08`, `0x13` (uracil), `0x17` (attachments), `0x1B` (BGZF+BAM trace alignment), `0x1C`, `0x22`.

Implement the block table **as data** — `block_id → {name, parsed?, codec, coordinate_dependent: bool}` — not as a match arm per block. New blocks then require a table row, not a code change.

### 6.3 Write plan, and the problem nobody has addressed

The `UnparsedBlock` passthrough pattern (autosnapgene's genuine and durable contribution) is **necessary but not sufficient**, and this is the most important unsolved issue in the whole interop story.

Passthrough guarantees that unknown *bytes* survive. It does not guarantee they remain *meaningful*. If a user opens a `.dna` file, inserts 500 bp at position 1,200 and saves, then the preserved history tree, uracil positions, custom colour block and embedded trace alignments all reference coordinates that no longer exist. SnapGene will open the file and render provenance that is **silently wrong** — which is worse than dropping the block, because the user has no signal.

> **ADR-4 — Every opaque block is tagged coordinate-dependent or coordinate-neutral. On any sequence-length-changing edit, coordinate-dependent opaque blocks are dropped, and the drop is reported in a modal that lists exactly what was lost and offers "save a copy with provenance intact (unedited)."**

| Block | Coord-dependent? | On sequence edit |
|---|---|---|
| `0x07`/`0x0B`/`0x1D`/`0x1E` history | **Yes** | Drop + report + offer unedited copy |
| `0x13` uracil positions | **Yes** | Drop + report |
| `0x14` DNA/strand colours | **Yes** | Drop + report |
| `0x1B` trace alignment (BGZF/BAM) | **Yes** | Drop + report |
| `0x10`/`0x11`/`0x12` alignable seqs + traces | **Yes** | Drop + report |
| `0x03` restriction digest cache | Regenerable | Drop silently |
| `0x0E` custom enzyme sets | No | Preserve verbatim |
| `0x17` attachments | No | Preserve verbatim |
| `0x08` properties, `0x22` | Unknown → treat as dependent | Drop + report |

Metadata-only edits (renaming a feature, changing a colour) are provably coordinate-neutral and preserve everything.

**Cookie versions:** write conservative known-good values (autosnapgene uses 14). Log observed `(export_version, import_version)` pairs from the test corpus against known SnapGene releases and build the mapping empirically. Never invent new values.

**Sizing, and why the research's estimate is wrong:** the source research budgets "3–6 engineer-weeks for a faithful common-case subset." The existence proof refutes it — sgffp is a dedicated single-purpose reader/writer that burned 19 minor versions and 163 commits over roughly six months of focused solo work and, as of 2026-07-16, still loses history and enzyme sets on save and still has a primer off-by-one. **Budget 8 weeks to the common-case writer, 4–8 months before history and traces round-trip losslessly, and permanent ongoing archaeology** because SnapGene ships roughly quarterly and can add blocks with no notice.

### 6.4 What to do when write support is imperfect — the honesty contract

This is a product decision, not an engineering one, and it should be made now.

1. **Say it in the README, the docs, and the Save dialog.** "Polylinker writes SnapGene `.dna` for the common case. It cannot yet reproduce SnapGene's cloning-history trees or re-embed Sanger trace alignments. Files that contain them are preserved byte-for-byte if you do not edit the sequence, and are reported to you if editing forces them to be dropped. GenBank export is lossless for everything Polylinker models."
2. **A "SnapGene compatibility report" on every `.dna` save**, listing per-block status: preserved verbatim / regenerated / dropped-because-edited / not-understood-but-preserved. One screen. Users will forgive a limitation they can see; they will never forgive one they discover three months later.
3. **Default save format is GenBank.** `.dna` is behind "Export → SnapGene." Make the honest path the easy path.
4. **Make GenBank export good enough that `.dna` matters less.** The acceptance test is "SnapGene 8.2.2 opens our GenBank with every feature, type, colour, primer binding and note intact." **[VERIFY: what exactly SnapGene writes into its own 'GenBank — SnapGene' export beyond standard GenBank — which custom qualifiers carry colours and types, whether ApE-style `/ApEinfo_fwdcolor` is used. The support article 403s. Resolve empirically by exporting a rich plasmid and diffing.]**
5. **Never fail silently.** A writer that drops unknown blocks while appearing to succeed is the single worst outcome in this document.

### 6.5 Other formats

| Format | Read | Write | Notes |
|---|---|---|---|
| GenBank | ✅ | ✅ **canonical** | Multiline qualifiers and `join()` are the bug sites. tg-oss has an open bug on exactly this (#272). |
| FASTA / multi-FASTA | ✅ | ✅ | Trivial. |
| FASTQ | ✅ | — | For ONT whole-plasmid consensus. |
| `.ab1` / ABIF | ✅ | v2 | Spec is public, free, complete (AB, July 2006, 54 pp). 128-byte header, 28-byte dir entries, 11 live type codes — a few hundred lines. **Read `FWO_` tag 1 (`char[4]`) to map channels to bases; do not assume ACGT.** Prefer `PBAS 2`/`PCON 2`/`PLOC 2` (original) over `*1` (edited). Always write version 101, as the spec mandates. Parse every tag as optional — AB explicitly disclaims any guarantee that a tag is present. **There is no dedicated Rust ABIF crate on crates.io; publish one.** |
| GFF3 / BED | ✅ | ✅ | Watch the 1-based-inclusive vs 0-based-half-open split. |
| Xdna (DNA Strider / Serial Cloner) | ✅ | ✅ | 112-byte fixed header. Cheap win — the only binary vendor format Biopython can *write*. |
| GCK (Gene Construction Kit) | ✅ | — | Cheap win. |
| SBOL 3 | v2 | v2 | Nice-to-have synth-bio interchange. RDF/XML, richer and lossy in the opposite direction from GenBank. |
| SnapGene enzyme-set `.txt` | ✅ | ✅ | Far easier import path than decoding block `0x0E`, and gives users their existing custom sets. |

---

## 7. The science

Each subsection: the algorithm, the reference paper, the reference implementation, and how correctness is proven.

### 7.1 Restriction digestion

**Model.** Biopython's, exactly: signed offsets `(fst5, fst3, scd5, scd3)` and a signed `ovhg` (negative = 5′ overhang, positive = 3′, 0 = blunt), loaded from REBASE `emboss_e` / `emboss_r` / `emboss_s` / `bairoch`. The whole arithmetic is two lines:

```
top    = match_start + fst5
bottom = top - ovhg
```

Derive blunt/5′/3′ from `sign(ovhg)`. This one representation handles blunt, 5′ overhang, 3′ overhang, Type IIS cutting at a distance, and Type IIB two-cut enzymes uniformly. Do not invent another.

**Regex compilation.** Expand IUPAC to character classes; wrap in a zero-width lookahead `(?=(?P<name>…))` so overlapping and tandem sites are found — a plain `finditer` consumes the match and misses `AGCTAGCT` for AluI. Non-palindromic enzymes get separate sense/antisense named groups.

**Fix Biopython's circular bug.** Biopython duplicates only `site_length − 1` bases for circular search. That is provably too short for Type IIS enzymes cutting 1–5 nt away, and badly too short for AcuI/BpmI/EcoP15I cutting 16–27 nt away. **Duplicate by `max(site_size, |fst5|, |fst3|, |scd5|, |scd3|) + site_size` and dedupe cuts by `(cut mod length)`.** This is the exact failure mode that breaks Golden Gate simulation on real BsaI/BsmBI backbones, and it must have a regression test with sites straddling position 1.

**Methylation.** Not in Biopython — import REBASE's per-enzyme sensitivity fields. `dam = GATC` (A6m), `dcm = CCWGG` (internal C5m), CpG = every `CG`. For each candidate site, expand a window covering the recognition site and test for an overlapping motif, then look up Blocked / Impaired / Not sensitive. Render blocked sites struck through, not hidden.

**Enzymes needing two sites.** BfiI, BpmI, BsgI, BspMI, Cfr9I, Cfr10I, Eco57I, EcoRII, FokI, HpaII, MboII, NarI, SacII, Sau3AI, SgrAI, plus all Type IIB. These produce in-silico digests that do not match the bench. **Annotate "requires two sites for efficient cleavage" — never present as a clean cut.** A single-NarI-site plasmid is cleaved very poorly, and presenting it as a clean digest costs a user a week.

**Validation.** Property tests: the cut-site set of a circular sequence is invariant under rotation (this is where origin bugs live); digest fragment lengths sum to the plasmid length; complete digest → religation reconstructs the original (assert by `cdseguid`). Golden tests against NEB's published site counts for standard vectors. Differential test against Biopython on 500 Addgene plasmids, with every disagreement adjudicated by hand and added to `polylinker-bench`.

### 7.2 Melting temperature

**Algorithm.** SantaLucia (1998) unified NN as the default; SantaLucia & Hicks (2004) selectable. The 1998→2004 revision changes only the AA/TT stack (ΔH −7.9→−7.6 kcal/mol, ΔS −22.2→−21.3 cal/K·mol).

```
Tm = ΔH / (ΔS + R·ln(C_T/x)) − 273.15,   R = 1.987 cal/(K·mol)
x = 1 for self-complementary oligos, 4 otherwise
```

*(Corrected 2026-07-27: this line previously said `x = 4` for self-complementary
and 1 otherwise, which is the wrong way round. SantaLucia uses C_T/4 for an
ordinary duplex — the two strands are at C_T/2 each — and C_T for a
self-complementary one. Implementing it as written put every palindrome out by
~8 °C. Caught by the differential against Biopython in
`reference/python/tests/xcheck_tm.py`, not by any hand-written test.)*

Salt corrections: SantaLucia 1998 entropy correction (default, `ΔS' = ΔS + 0.368·(N−1)·ln[Na⁺]`), Schildkraut–Lifson 1965, Owczarzy 2004 monovalent, Owczarzy 2008 divalent with the Mg/dNTP chelation term. Defaults: 50 nM oligo, 50 mM Na⁺, 1.5 mM Mg²⁺, 0.6 mM dNTP.

**Source of the numbers.** Take the parameter tables from **`@teselagen/sequence-utils`'s `calculateSantaLuciaTm.js` (MIT, clean-room)** or from Biopython's `Bio.SeqUtils.MeltingTemp` (Biopython Licence / BSD-3, tables `DNA_NN1`–`DNA_NN4`) — **not** from Primer3's `oligotm.c`, which is GPL-2.0. The ~10 ΔH/ΔS numbers per table are published scientific measurements and uncopyrightable facts; only Primer3's C implementation carries the licence.

**Do not promise decimal parity with SnapGene.** The source research recommends implementing Tm "exactly as SnapGene does so numbers match," then states two sections later that SnapGene documents only "a nearest-neighbor thermodynamic algorithm with up-to-date parameters" and that "exact numeric parity is not achievable from public sources." Both cannot be true. **You cannot match to the decimal what is not specified.** What you can do: implement SantaLucia with documented concentrations, empirically calibrate against a panel of ~200 primers run through SnapGene, publish the residual (expect ±0.5–1 °C), and put the method, the parameter set and the concentrations in a **visible methods popover next to every Tm in the UI**. A documented modelling difference reads as rigour; an undocumented one reads as a bug.

**Separate Tm from Ta.** Report a single physically-defined primer Tm. Put annealing-temperature advice in a separate per-polymerase panel (SnapGene's own guidance: Ta = Tm + 6–12 °C for Phusion/Q5/Phire; Ta = Tm − 5 for Taq). Never bake buffer corrections into the reported Tm, or you will never be able to explain a discrepancy.

### 7.3 Primer binding-site detection

Exact 3′-anchored seed of configurable length (8–35 nt, default 14) with at most one isolated mismatch, extended 5′ with free mismatches; compute Tm over the **annealed footprint only** and threshold. This is what SnapGene does and what pydna does (`limit` ≈ 13 nt). **[VERIFY: SnapGene's exact numeric defaults and ranges in the Primers → Hybridization Parameters dialog are not published; read them off an unlicensed 8.2.2 install.]**

**Track footprint and 5′ tail as separate objects.** Every downstream feature depends on that split — Gibson arm detection, restriction-site addition, mutagenesis, att-site tails. A 5′ tail must not contribute to Tm.

### 7.4 Primer design

Primer3's model: a sum of weighted absolute deviations per oligo (Tm LT/GT, size, GC%, position, end stability, self-any/self-end, hairpin, template mispriming, Ns, sequence quality) plus a pair penalty (ΔTm, compl-any, compl-end, product size, product Tm). Enumerate candidates, sort by penalty, pair-scan.

**Implement it ourselves.** Bundling primer3-py is not "closing the gap cheaply" — it is an irreversible relicensing of the entire distribution to GPL-2.0, and possibly worse (see §8.4). The objective function is fully documented in the Primer3 manual; the thermodynamics come from our own MIT-derived `pl-thermo`. Hairpin/homodimer/heterodimer via a port of **`seqfold`** (Lattice Automation, MIT — a Zuker 1981 DP with SantaLucia 2004 DNA and Turner 2009 RNA energies, written specifically because "UNAFold and mfold… format and license are restrictive"). **Avoid ViennaRNA and UNAFold entirely**: ViennaRNA's licence forbids redistribution for any fee beyond media cost and requires contacting the authors for inclusion in a commercial product.

Budget: 6 weeks for a credible Primer3-class optimiser. Validate by differential comparison against `primer3_core` run **out-of-process, in CI only** (using a GPL tool as a test oracle creates no distribution obligation).

### 7.5 ORF finding and translation

NCBI `transl_table` 1–33 verbatim from `Bio.Data.CodonTable` (each table has a distinct **Starts** row; the initiator translates as Met regardless of whether it is AUG, CUG or UUG). Table 11 (bacterial) permits ATG, GTG, TTG, ATT, CTG. Reference implementations: `orfipy` (Singh & Wurtele 2021), EMBOSS `getorf`.

Plasmid-specific requirements the general tools lack: 6-frame scanning that **wraps the origin**, and a "longest ORF only / hide nested ORFs" toggle. Per-feature genetic code override and non-ATG start designation are both easy to overlook and genuinely needed.

### 7.6 Alignment

**Architecture, copying SnapGene's:** seed-and-extend anchoring, then affine-gap Smith–Waterman **only inside inter-anchor gaps and at the ends**. O(n) anchoring; quadratic DP only over short spans.

DP kernels, all permissive: **`edlib`** (MIT, Myers bit-vector — best for high-identity Sanger reads), **`WFA2-lib`** (MIT, exact gap-affine wavefront, ideal for high-identity plasmid comparison), **`parasail`** (BSD-3-style Battelle **[VERIFY: GitHub reports NOASSERTION; read LICENSE.md before relying on it]**).

**Circular whole-plasmid alignment** — the requirement that is hard to retrofit and matters most in 2026, because Plasmidsaurus-style whole-plasmid sequencing has largely displaced Sanger for construct verification:

1. Concatenate reference with itself (`ref+ref`).
2. Seed-and-extend + banded SW of the query against the doubled reference.
3. Take the best-scoring diagonal.
4. Map coordinates back with `pos mod len(ref)`.
5. Split any alignment block straddling the origin into two segments.
6. **Reject alignments longer than `len(ref)`** to avoid double-counting.

Published alternatives for the multi-sequence case: MARS (Ayad & Pissis 2017) and CSA (Fernandes 2009, generalized cyclic suffix tree).

**Sanger.** `.ab1` via our own ABIF reader; basecalling, secondary-peak heterozygote detection and heterozygous-indel deconvolution modelled on **Tracy** (Rausch et al. 2020, BMC Genomics 21:230, BSD). Heterozygote rule: call an IUPAC ambiguity when secondary/primary peak height exceeds ~0.2–0.4 and phred quality is above threshold **[VERIFY: Tracy's and SnapGene's actual defaults are not documented]**. Reads align **independently** to the reference — this is not an assembler, and saying so avoids a category of complaint.

### 7.7 Auto-annotation

> **This is the flagship, and the biggest scoping win in the document.**

The source research recommends reimplementing pLannotate's four-database BLAST+/DIAMOND/Infernal pipeline. **Do not do this on the critical path.** It conflates two very different workloads and buys a 4–6 month packaging nightmare for no user-visible gain.

SnapGene's actual magic moment is documented: **≥96% nucleotide identity against a curated library, tolerant of occasional mismatches and indels, with 1–2 missing terminal codons accepted for CDS features intended for fusions, plus perfect-protein matching so codon-optimised genes still resolve.** That is approximate string matching over a few megabytes of curated features, not homology search over Swiss-Prot.

**Tier 1 (v1, on file-open, <200 ms, pure Rust/WASM, no native binaries, no gigabyte database):**
1. Build a k-mer seed index over `polylinker-features` (k=12, both strands).
2. Duplicate the query for circularity; collect seed hits; chain collinear seeds.
3. Verify each chain with `edlib` in `HW` (infix) mode, computing edit distance and identity.
4. Keep matches at ≥96% identity (user-adjustable), with 1–2 terminal codons of slack for CDS.
5. Six-frame translate and match the protein index for codon-optimised CDSs.
6. Score `= match_length × identity × (match_length / db_feature_length)`.
7. Resolve overlaps by pLannotate's rule: trim each hit by 15% of its length on each side, drop lower-scoring hits that still overlap.
8. Flag any hit covering <95% of the DB feature length as a **fragment**, rendered as an unfilled outline arrow — something SnapGene does not do well and users complain about.
9. Deduplicate by `(feature_id, position mod L)`.

**Tier 2 (v2, opt-in "Deep annotation" button, optional sidecar or remote):** Swiss-Prot via DIAMOND with the PAM30 matrix (chosen specifically to catch codon-optimised and silently-mutated variants), Rfam covariance models via Infernal `cmscan`. Genuine long-tail value. Never on the critical path — Rfam's own docs imply ~360 s/Mb single-threaded, and pLannotate hard-codes `--cpu 1`, so a circularity-doubled 10 kb plasmid is ~7 s for Infernal alone before BLASTN and two DIAMOND passes. Realistic cold start 10–30 s, versus the 2-second budget the research proposed.

**Validation.** Against Addgene GenBank files as a *soft* reference only — remembering those annotations are themselves SnapGene output, so agreement is confirmation of compatibility, not of truth. Hard validation comes from the curated cases in `polylinker-bench`.

### 7.8 Agarose gel simulation

**There is no first-principles model that reproduces realistic band positions.** Biased reptation, Ogston sieving and reptation-with-stretching each describe a regime; none gives usable absolute positions. Every real tool fits an empirical curve — MacVector cites Rill et al. 2002 and Van Winkle et al. 2002; Gelbox fitted its own model to 1 kb-ladder gel images; pydna's `gel.py` is a natural-boundary cubic spline of length→Rf with a Gaussian band profile (`band_spread = max_spread/log10(L)`, height `= (mass/(240·log10 L))·1e10`).

**Do the same, deliberately and visibly.** Ship a monotone cubic spline of `log10(length) → relative mobility`, calibrated per agarose percentage against real 1 kb and 100 bp ladder images, parameterised by percentage and run time. Gaussian band profile. **Explicitly refuse to predict migration of uncut/supercoiled/nicked plasmid** — MacVector documents that no algorithm does this, and refusing is more useful than guessing. Put "empirically calibrated; see methods" next to the gel.

### 7.9 Assembly simulation

**Model.** pydna's `Dseq(watson, crick, ovhg)` — the signed offset of crick relative to watson makes sticky ends, blunt ends and circularity one representation. Ported to Rust in `pl-clone`.

**Homology-overlap assembly (Gibson/HiFi class).** Common substrings via a suffix array (`pydivsufsort` in pydna; `suffix_array` or `libdivsufsort` bindings in Rust), `limit = 25` bp default; `terminal_overlap()` restricts to matches at a sequence terminus, which is what these chemistries actually require. Overlapping regions become graph **nodes**, the sequence between two overlaps becomes an **edge**, sentinel 5′/3′ nodes are added, then enumerate all simple paths (linear products) and all cycles (circular products). Reference: Pereira et al. 2015, BMC Bioinformatics 16:142.

**Type IIS assembly.** Same graph, with 4-nt overhangs (3-nt for SapI) as nodes. Digest with BsaI `GGTCTC(1/5)`, BsmBI/Esp3I `CGTCTC(1/5)`, BbsI `GAAGAC(2/6)`, SapI `GCTCTTC(1/4)`, PaqCI/AarI `CACCTGC(4/8)`; discard fragments carrying the recognition site; index the internal fragment by `(left_overhang, right_overhang)`. A valid one-pot assembly is a simple cycle visiting each required part once. **Additionally flag palindromic overhangs, repeated overhangs, and single-mismatch-neighbour overhangs** — Potapov/Pryor, PLOS ONE 2020, `10.1371/journal.pone.0238592`. DNA Cauldron (MIT) is the reference for the Type IIS layer and overhang-collision checking.

**Validation — the differential harness that justifies the port.** Every assembly in `polylinker-bench` runs through both `pl-clone` and pydna in CI, and the two `cdseguid` values must match. pydna is the oracle; disagreement is a bug in one of us and gets adjudicated by hand.

### 7.10 Codon optimization (v2)

**DNA Chisel** (MIT, Zulkower & Rosser 2020, Bioinformatics 36:4508). Two-phase: resolve hard constraints by local search ignoring objectives (which surfaces mutually incompatible constraints early), then maximise the weighted objective sum subject to those constraints. Expose all three strategies — `use_best_codon` (CAI maximisation, Sharp & Li 1987), `match_codon_usage` (harmonisation), `harmonize_rca` — plus CAI and %MinMax readouts. **Naive CAI maximisation alone is a documented way to make expression worse**; the harmonisation option is not optional. Codon tables computed from RefSeq CDS by our own `build-codon-tables` script, not from Kazusa (unlicensed, GenBank release 160 from 2007).

### 7.11 CRISPR (v2)

PAM scanning: `(?=([ACGT]{21}GG))` on both strands with a lookahead, plus origin wraparound.

**CFD off-target** (Doench, Fusi et al. 2016, Nat Biotechnol 34:184; Suppl. Table 19):
```
score = 1.0
for i in 0..20:
    if wt[i] == sg[i]: continue
    key = 'r' + wt[i] + ':d' + revcom(sg[i]) + ',' + str(i+1)
    score *= mm_scores[key]
score *= pam_scores[pam[1:3]]
```
**The quirk that trips up every reimplementation:** the RNA base is the guide's base as written, but the DNA base in the key is the **reverse complement** of the off-target base, because the matrix is indexed by rN:dN base-pair identity, not by the two aligned bases. Worked check: rG:dA at position 7 (0.57) × rC:dT at position 10 (0.87) = 0.50.

**MIT/Hsu specificity** (Hsu et al. 2013, Nat Biotechnol 31:827): `S_hit = Π(1−W[e]) × 1/(((19−d)/19)·4+1) × 1/n_mm²`, aggregated as `S_guide = 100/(100 + ΣS_hit)`.

**On-target:** consume **`crisprScore`** (MIT) or **`rs3`** (Apache-2.0, ships trained pickles). **Exclude CRISPOR ≥ v4 outright** — its LICENSE.txt is self-contradictory (free for academic and non-profit in one sentence, license-required for non-profits four paragraphs later) and non-OSI either way, and `bin/` carries separate third-party terms. Do not revive Azimuth (BSD-3 but archived 2024-06-17, Python 2). Pin scikit-learn versions — the pickles are version-fragile and an unpinned bump silently changes guide rankings.

### 7.12 The correctness and validation strategy

**7.12.1 The primitive: `cdseguid`.** All three QA layers the source research proposes — golden files, differential parsing, property tests — compare *representations*. For a circular double-stranded plasmid, rotation, strand choice, annotation order and feature-name spelling are all free, so byte-diffs and GenBank diffs produce false failures and mask real ones. Use `seguid` 0.2.1 (MIT, Björn Johansson): `cdseguid` for circular double-stranded, `ldseguid` for linear double-stranded, `csseguid`/`lsseguid` for single-stranded. **Every assertion about a molecule is an assertion about its `cdseguid`.**

**7.12.2 Hazard tiering — how test budget is allocated and how the UI is designed.**

| Tier | Character | Members | Response |
|---|---|---|---|
| **1** | Silent, expensive, hard to notice at the bench | Cut sites hidden by the active enzyme-set filter; methylation-blocked sites; Type IIS overhang identity and Golden Gate ligation fidelity; coordinate arithmetic across the circular origin; reading frame after an indel; enzymes needing two sites | Adversarially fuzzed, differentially tested, and rendered **LOUD** in the UI. Never display a digest without stating what is filtered out. Never assert Golden Gate success without showing the full overhang set and its single-mismatch neighbours. |
| **2** | Wrong but caught quickly | Auto-annotation calls, ORF boundaries, Sanger discrepancy calls | Golden + property tested. Show identity/coverage so the user can judge. |
| **3** | Approximate by nature; users expect drift | Tm, agarose band position, codon-optimization scores | Ship a visible **methods + parameters + citation** popover so a discrepancy with SnapGene reads as a documented modelling choice, not a bug. |

This ranking says the research's instinct to chase Tm parity "to the decimal" is misallocated effort. Tier 3 needs *documentation*, not precision.

**7.12.3 The four test layers.**

1. **Golden files.** A corpus of several hundred real `.dna` files. Assert `.dna → model → GenBank` round-trips feature-for-feature and coordinate-for-coordinate, and that `parse → write` of untouched files is byte-identical. **[VERIFY: whether Addgene's `.dna` files can be legally redistributed as public test fixtures. Addgene's terms are noncommercial-only with an explicit anti-scraping clause. If not, CI must fetch at test time (fragile) or the corpus must be built from lab-contributed files with written permission.]**
2. **Differential testing.** Every corpus file through Biopython's `SnapGeneIterator`, `@teselagen/bio-parsers`' `snapgeneToJson`, `snapgene_reader` and `sgffp`. Any disagreement is a bug in someone; finding out which is cheap and each adjudicated case becomes a benchmark entry. Same pattern for `pl-clone` vs pydna and `pl-thermo` vs Biopython's `MeltingTemp`.
3. **Property-based testing** (`fast-check` 4.8.0 on TS, `proptest` on Rust — both with seed-reproducible shrinking):
   - `revcomp(revcomp(s)) == s`
   - For circular sequences, the restriction-site set is **invariant under rotation** ← where origin bugs live
   - Digest fragment lengths sum to the plasmid length
   - Complete digest → religation reconstructs the original (by `cdseguid`)
   - `translate(orf)` has no internal stops
   - After an insertion of length *k* at position *p*, every feature coordinate ≥ *p* shifts by exactly *k*
   - Rotating a document by *r* and re-annotating yields the same feature set
   - GenBank round-trip is an identity on the model
   - Any `.dna` byte sequence either parses or errors — never panics, never allocates unboundedly (`cargo-fuzz`)
4. **`polylinker-bench` (CC0).** The public truth set. Published as its own repo and its own paper, runnable by SnapGene, Benchling, pydna, UGENE, plascad and Polylinker alike. Publish our pass rate. Invite everyone else's.

**Publish the pass rate before marketing anything.** Scientific users abandon a tool permanently after one wrong answer, and "open source" buys zero benefit of the doubt in a wet lab.

---

## 8. Data and licensing

### 8.1 The hard table

| Dataset | Licence | Verdict | Notes |
|---|---|---|---|
| **REBASE** `emboss_e`/`_r`/`_s`/`bairoch` | Bespoke grant, © R.J. Roberts 2009: *"Those seeking to distribute REBASE files with their software packages are welcome to do so, providing it is clear to your users that they are not being charged for the REBASE data."* | **GO**, isolated | Ship in `data/rebase/` as a **separate package** with its own NOTICE quoting the grant verbatim + the Roberts et al. 2023 NAR citation + release number + download date. The pricing condition **violates OSD #1 / DFSG #1** (which forbid restricting sale), so it must never be inherited by the source tree. Also implement an EMBOSS-style `pl fetch-enzymes` path for Debian/Fedora. |
| **Biopython `Bio.Restriction`** (REBASE EMBOSS 404, 2024) | Biopython Licence / BSD-3 | **GO** | Cleanest drop-in. **Add the REBASE NOTICE that Biopython omits.** |
| `@teselagen/sequence-utils` + seqviz enzyme tables (~350 enzymes) | MIT | **GO** | Fallback if you want to skip REBASE entirely. No provenance headers — add your own. |
| **UniProt / Swiss-Prot** | Believed CC BY 4.0 | **HOLD → GO** | **[VERIFY: `uniprot.org/help/license` did not render; the FTP LICENSE 404s. Almost certainly CC BY 4.0 since 2018, but this is the pipeline's single largest protein-feature source and must not ship on belief. Fetch the release-directory README or a rendered notice manually.]** |
| **Rfam 15.1** (Jan 2026) | **CC0** — confirmed in the CURRENT/README | **GO** | 4,227 families. Ship `Rfam.cm` + `Rfam.seed` for the v2 deep tier. Cite anyway. |
| **Pfam / InterPro** | CC0 | **GO** | — |
| **FPbase** | Data stated free of copyright, commercial OK; site content CC BY-SA 4.0 | **HOLD** | **[VERIFY: `fpbase.org/terms` 403s to automated fetch. Retrieve manually or email Talley Lambert. Load-bearing in the clean-room pipeline.]** Keep site *prose* out of the repo regardless. |
| **NCBI GenBank / RefSeq / UniVec 10.0** | US public domain (submitter-rights caveat) | **GO** | UniVec is an underused, fully free source of backbone/MCS/adapter/linker/primer features. |
| **Barrick 217 part variants, Table S1** (PMC10120640) | CC BY 4.0 incl. commercial | **GO** | 217 widespread/recurrent real-world variants — this is what makes fuzzy matching competitive with SnapGene's. Re-derive from the paper; do **not** re-host the underlying Addgene corpus. |
| **Biopython `MeltingTemp`** NN tables | Biopython / BSD-3 | **GO** | DNA_NN1–4, RNA_NN1–3, seven salt corrections, DMSO/formamide. |
| `@teselagen` `calculateSantaLuciaTm.js` | MIT | **GO** | Clean-room Primer3-compatible reimplementation. Preferred source. |
| **MAFFT** 7.525 | BSD-3 (verified verbatim, 3 clauses) | **GO** | The only bundleable MSA engine. |
| **Kalign 3.6.0** | **Apache-2.0** (COPYING is verbatim Apache 2.0) | **GO** | ⚠️ The source research said GPL-3.0 in one section and Apache-2.0 in another. **Apache-2.0 is correct.** Needlessly excluding it was a flat error. |
| **edlib** | MIT | **GO** | — |
| **WFA2-lib** | MIT | **GO** | — |
| **minimap2** | MIT | **GO** | — |
| **parasail** | BSD-3-style Battelle | **HOLD** | **[VERIFY: GitHub reports NOASSERTION. Read LICENSE.md directly before relying on it.]** edlib + WFA2 make it optional anyway. |
| **BLAST+ / NCBI C++ Toolkit** | US Government public domain **with enumerated third-party exceptions** | **GO with audit** | Exceptions include `include/algo/gnomon/debruijn` = **AGPL** and, missed by the research, **`src/connect/mbedtls` = Apache-2.0 or GPL-2.0** — a second copyleft-capable component far more likely to be in a real build. Also `newick.tab.*`, `bdb_query_bison.tab.c`, `FindSqlite3.cmake`, `parson.c/h`, `util/compress/zlib`, `cityhash`, `include/util/regexp/ctre/`. Audit the full list, not two files. |
| **pydna** 5.5.16 | BSD-3 | **GO** | The cloning kernel and the CI oracle. |
| **DNA Chisel / DnaFeaturesViewer / DnaCauldron** (EGF) | MIT | **GO** | All in maintenance mode (nothing newer than May 2025). |
| **seqfold** | MIT | **GO** | Zuker DP. Use instead of ViennaRNA. |
| **seguid** 0.2.1 | MIT | **GO** | The correctness primitive. |
| **rs3 / Rule Set 3 + weights** | Apache-2.0 | **GO** | v2. Pin sklearn/numpy. |
| **crisprScore** 1.5.1 | MIT (`Copyright (c) 2022 Genentech, Inc.`) | **GO with caveat** | Grant covers the R wrapper. It resolves upstream Python models via basilisk/reticulate **at runtime**, and MIT does not automatically launder what basilisk downloads. **[VERIFY: audit `inst/python` independently rather than relying on the maintainers' assertion.]** |
| **Azimuth / Rule Set 2** | BSD-3 | **GO but dead** | Archived 2024-06-17, Python 2. Do not depend. |
| **CFD weight matrices** (Doench 2016 Suppl. Table 19) | Numeric facts in a Springer-formatted table | **GO** | Consume via crisprScore (MIT) or re-extract the numbers into your own format. Do not copy a Springer XLSX into the repo. |
| **Primer3 / primer3-py** | **GPL-2.0** (verbatim GPL v2 in both LICENSE files) | **NO-GO for bundling** | See §8.4. Test oracle only, out-of-process, CI-only. |
| **DIAMOND** | GPL-3.0 | **NO-GO for linking** | Optional out-of-process sidecar in the v2 deep tier only. |
| **Clustal Omega** | GPL-2.0 | **NO-GO** | **[VERIFY: EBI page returned no content; unverified, but exclusion is the conservative direction.]** |
| **MUSCLE v5** | GPL-3.0 (verified) | **NO-GO** | — |
| **FlashFry** | **GPL-3.0-or-later** (license.txt verbatim) | **NO-GO** | ⚠️ The README badge says "License: MIT". **The badge is wrong; the LICENSE file governs.** Anyone auditing by badge or by scanner will get this wrong. File an upstream issue. |
| **UGENE** | GPL-2.0 | **NO-GO** for code | Data (its bairoch dump) is just REBASE. |
| **ViennaRNA** | Non-OSI; forbids redistribution for any fee beyond media cost; requires author contact for commercial inclusion | **NO-GO** | — |
| **UNAFold / mfold** | Restricted | **NO-GO** | — |
| **CRISPOR ≥ v4** | Non-OSI, self-contradictory | **NO-GO** | GitHub shows only "NOASSERTION" and the repo looks active and normal — easy to vendor by accident. |
| **pLannotate `snapgene.csv` + `BLAST_dbs.tar.gz`** | GPL-3.0 wrapper over unlicensed SnapGene-derived data | **NO-GO — hard** | 159 KB, ~1,367–1,600 rows **[VERIFY: count it]**, Description column is SnapGene's curated prose verbatim (`"chloramphenicol acetyltransferase; cat; confers resistance to chloramphenicol"`). The `sseqid` convention `CmR_(2)` / `KanR_(3)` / `f1_ori_(3)` is a **copying fingerprint**. Its *algorithm* is GO (published in NAR). |
| **GenoLIB** (13,240 features) | Article CC BY 4.0; data derived from 1,901 `.dna` files scraped from snapgene.com on 2014-05-12 | **NO-GO for the data**, GO to cite the paper | A CC BY article licence covers the article, not third-party data reproduced in it. Note that Benjamin Glick, GSL Biotech's founder, is a GenoLIB co-author with a declared interest — that suggests *tolerance*, not a transferable licence to third parties. |
| **Addgene GenBank corpus** | Noncommercial-only + explicit anti-scraping clause; annotations are SnapGene Server output | **NO-GO for bulk derivation** | Bulk-harvesting is doubly compromised: it breaches Addgene's terms *and* launders SnapGene annotations. Take raw sequences from NCBI instead. |
| **SnapGene Common Features / plasmid library** | © SnapGene; EULA bans derivative works | **NO-GO — absolute** | — |
| **ApE** | Proprietary; no redistribution of modified source | **NO-GO** | Reference-only. |
| **Kazusa CUTG** | No licence text at all; GenBank release 160 (2007) | **AVOID** | Unlicensed *and* 19 years stale. Legal and technical arguments point the same way. |
| **CoCoPUTs / HIVE-CUTs (FDA)** | US federal work, no explicit licence | **GO with caution** | **[VERIFY: contractor-authored works at GWU/FDA are not automatically PD.]** Prefer recomputing from RefSeq — a day of work, unambiguously clean. |
| **iGEM Registry / SynBioHub** | Not verifiable (Cloudflare 403) | **HOLD** | Do not assume iGEM parts are freely redistributable data because the physical materials move under OpenMTA — an MTA governs materials, not database rights. |
| **SBOL spec / sbolstandard.org** | Site CC BY-NC-ND 4.0; spec licence unstated | **HOLD** | Use SynBioDex code repos (Apache/BSD) instead of site content. |
| **IBBIS Common Mechanism (`commec`)** | MIT | **GO** | v2 biosecurity screen. |

### 8.2 A correction the source research got backwards

The research's legal section states that SnapGene's Common Features database enjoys the EU **sui generis** database right, "far worse" than US copyright. **This is wrong, and it contradicts the research's own reasoning elsewhere.**

Directive 96/9/EC **Art. 11(1)** limits the right to "database whose makers or rightholders are nationals of a Member State or who have their habitual residence in the territory of the Community." The research uses exactly this to exempt REBASE (US maker). SnapGene's feature database has the same disqualifying nationality: the trademark owner of record is **GSL Biotech LLC, a Delaware LLC at 225 Franklin Street, Boston MA**. Art. 11(3) permits Council agreements extending the right to third countries; none covering the US is known to exist **[VERIFY: this is reasoning from absence — searching Council decisions was out of budget. It is load-bearing for the "REBASE is safe" headline too.]** Siemens' 2025 acquisition does not help: the right attaches (or fails to attach) to the maker at completion, and a later change of owner nationality cannot retroactively create a right that never vested.

**The correct statement of the risk:** exposure is **US thin-compilation copyright under *Feist*** (selection and arrangement of a 13,000-feature curated set is precisely the creative selection that survives *Feist*) **plus straightforward copyright in the descriptive prose**. That is entirely sufficient to justify the NO-GO. Get the reason right, because the wrong reason produces the wrong mitigation.

### 8.3 The annotation-database sourcing plan

`polylinker-features` — **CC BY 4.0**, its own repo, its own DOI, its own release cadence.

**Sources, in build order:**

1. **UniProt / Swiss-Prot** filtered to annotation score ≥3 → CDS-level features: resistance markers, epitope tags, fluorescent proteins, recombinases, enzymes, Cas variants. **[VERIFY licence]**
2. **Rfam 15.1** (CC0) covariance models → ncRNA, riboswitches, terminators, aptamers.
3. **FPbase** REST API (JSON/CSV, no login) → fluorescent proteins with variant discrimination. **[VERIFY terms]**
4. **NCBI UniVec build 10.0** + RefSeq → backbones, origins, MCS, adapters, linkers, standard primers.
5. **Barrick Table S1** (CC BY 4.0) → the 217 widespread and recurrent real-world part variants. This is the single input that makes fuzzy matching competitive rather than brittle.
6. **Hand curation from primary literature** for promoters, origins and terminators, with a **DOI recorded per feature**.

**Schema — every row carries:** `id`, `name`, `aliases[]`, `type` (SO term where one exists), `sequence`, `is_protein`, `source_db`, `source_accession`, `source_licence`, `curator`, `date_added`, `doi`, `notes`, `review_status`.

**Six hard rules:**

1. **Never copy SnapGene's descriptions.** Short names (AmpR, KanR, f1 ori, WPRE, CMV enhancer) are largely unprotectable community nomenclature. Every Description string is rewritten from the primary source or UniProt.
2. **Never reuse SnapGene's `sseqid` naming scheme.** `CmR_(2)`, `KanR_(3)`, `f1_ori_(3)` is a fingerprint of copying.
3. **Automated similarity gate in CI:** compare the final description column against pLannotate's `snapgene.csv` and **fail the build on >30% token overlap for any single row.**
4. **Per-row provenance means a single tainted row can be dropped without rebuilding**, and a licence challenge can be answered feature-by-feature.
5. **Publish the build script, not just the output.** The pipeline is the reproducibility claim.
6. **AI may propose, never assert.** LLMs may triage candidates, draft descriptions and cross-check entries in the offline build pipeline. Nothing AI-derived ships without a human `curator` and a `review_status: reviewed`.

**Governance.** Community PRs with a template requiring source, accession, licence and DOI. Two-reviewer sign-off for new entries. Versioned releases (`2026.10`, `2027.01`). Every annotation Polylinker emits stamps the database version.

**This is a permanent staffed function, not a build task.** SnapGene employs people to do this continuously; that is *why* their library is the product. pLannotate shipped a good database once in 2020–21 and it rotted. Budget 8 weeks initial and ~0.2 FTE forever, and design the contribution model so the community can carry it. If no one can be found to carry it, that is a strong signal to reconsider the whole project.

### 8.4 The project's own licence

> **ADR-6 — Code: Apache-2.0. Annotation database: CC BY 4.0. Correctness benchmark: CC0. REBASE data: separate package, bespoke grant, own NOTICE.**

**Why Apache-2.0 over MIT:**
- **Express patent grant.** MIT has none. This is a space where a Siemens subsidiary could file software patents prospectively.
- **Patent-retaliation clause.** A cheap, real deterrent.
- **The NOTICE mechanism** is exactly the right tool for a project that must attribute REBASE, UniProt, Rfam, FPbase, Doench 2016, the Broad GPP and a dozen upstream libraries.
- **One-way GPL-3 compatibility** means downstream GPL-3 tools (pLannotate, UGENE) can still adopt our components.

**Why not GPL:** it would foreclose adoption by the very tools we want to improve — `@teselagen/ove` (MIT), seqviz (MIT), pydna (BSD-3), and, in the best case, SnapGene itself adopting `polylinker-bench`. The strategic goal is that everyone runs our benchmark and everyone uses our database. Copyleft works against that.

**The GPL contamination question that nobody answered, and that must be closed before architecture is locked:**

> **Is Primer3 GPL-2.0-*only* or GPL-2.0-*or-later*?**

Both `primer3-org/primer3/LICENSE` and `libnano/primer3-py/LICENSE` are verbatim GNU GPL v2 (June 1991) text. A bare GPL-2 LICENSE with **no "or (at your option) any later version" grant in the source headers means GPL-2.0-only**, which is **incompatible with Apache-2.0** (patent-clause conflict) and with GPL-3.0. If Primer3 ever ends up in one combined work with `rs3` (Apache-2.0), Kalign (Apache-2.0) or CGView.js (Apache-2.0), the result is not merely "GPL-2.0 as a whole" — it is **undistributable**.

Action: `grep -rE 'any later version' primer3/src/` before any decision. And note that the proposed mitigation may not even be implementable as described: what OpenCloning and SpliceCraft actually call is primer3-py's `calcTm`/`calcHairpin`/`calcHomodimer`/`calcHeterodimer` — in-process C-extension calls. `primer3_core` (the CLI) does full primer *picking*, not standalone thermodynamics; the CLI equivalents are the separate `oligotm` and `ntthal` binaries. Nobody has checked that those cover the API surface in use, or what per-call subprocess overhead looks like when scoring thousands of candidate oligos.

**Since we implement Tm and primer picking ourselves on MIT-derived code, Polylinker sidesteps all of this.** That is the decisive argument for §7.4's "build it, don't bundle it."

**Maintain a licence compatibility *matrix*, not a flat GO/NO-GO list.** "Each dependency is individually fine" does not imply the combination is distributable. Run `cargo-deny` and `license-checker` in CI with an explicit allowlist.

---

## 9. Legal guardrails

### 9.1 The situation

Ownership chain: GSL Biotech LLC (Chicago) → Insightful Science → rebranded Dotmatics (April 2022) → **Siemens AG, $5.1B, completed 2025-07-01**. The EULA licensor is literally "GraphPad Software, LLC ('SnapGene')," Massachusetts law, effective 2026-01-15. The trademark owner of record is still GSL Biotech LLC — post-merger housekeeping that is mildly useful if standing ever has to be proven.

**The good news, verified:** the EULA contains **no competing-product clause, no benchmarking clause, and nothing about extracting its databases.** That is unusually permissive by 2026 standards. **Re-read it before every release and archive each version** — a Siemens legal review is exactly the sort of thing that adds one.

**The sharp edge:** §4 bans "alter, merge, modify, adapt, translate, decompile, reverse engineer, disassemble, or otherwise reduce the Software to a human-perceivable form" with **no "except as permitted by applicable law" carve-out.** In the EEA that clause is simply **null and void** (Directive 2009/24/EC Art. 8 voids terms contrary to Arts. 5(3) and 6), and CJEU C-406/10 (*SAS v. World Programming*) held that data file formats are not protected expression. In the US, *Bowers v. Baystate* (Fed. Cir. 2003) and *Davidson v. Jung* (8th Cir. 2005) hold that a clicked EULA validly waives fair use and even the §1201(f) interoperability exception. **So in the US the real exposure is contract, not copyright — and the contract only exists if someone clicks Accept.**

**DMCA §1201 never attaches.** `.dna` is a plaintext-structured TLV container with no technological protection measure; SnapGene's own docs describe file "locking" as OS-level read-only permissions. No TPM, no circumvention.

**No patents found.** Assignee searches for GSL Biotech return nothing; the underlying techniques are decades-old published art. Note that clean-room design is **no defence to patents** — independent invention is irrelevant — and Siemens can file prospectively.

### 9.2 Do's and don'ts

**Never:**

1. **Never install SnapGene or accept its EULA on the development path.** Highest-value rule in this document. Put it in `CONTRIBUTING.md`: contributors touching file-format code must not be SnapGene licensees.
2. **Never run a decompiler, disassembler, hex-editor-against-the-binary, `strings`, Ghidra, IDA, Hopper or a debugger on SnapGene.exe / SnapGene.app**, and never open its resource bundle. There is zero technical need — the format is fully described in BSD-3 and MIT code.
3. **Never contact Dotmatics for the format specification**, however tempting their public "please contact us for information about how to read and write SnapGene files" invitation is. Any spec will arrive under an NDA or developer agreement that contractually poisons everyone downstream (Bowers/Davidson), hands them a trade-secret theory they do not currently have, and destroys a clean provenance chain. **Asking is strictly worse than not asking.** Circulate this to collaborators in writing before anyone emails them.
4. **Never extract anything from a SnapGene installation** — features, enzyme sets, colours, plasmid library. This is the one act that could actually generate a claim.
5. **Never use the mark as identity.** Not in the project name, package name (npm/PyPI/crates.io), app icon, splash screen, window title, repo name, or domain. No `snapgene-*.org`.
6. **Never write "SnapGene" into files we generate** beyond the bare magic bytes the format requires. This is the exact Autodesk/ODA trigger: ODA was sued for **trademark**, because DWGdirect wrote "TrustedDWG" strings containing "AutoCAD" into generated files. The format cloning survived; the branding drew the lawsuit.
7. **Never hire an ex-GSL Biotech / Dotmatics / SnapGene engineer onto the format or feature-database code.** Trade-secret misappropriation survives even where the format itself is unprotectable.
8. **Never ship a paid tier, hosted commercial SaaS or enterprise edition** without counsel first. Commercial substitution — not cloning — is what turned *SAS v. WPL* into 13 years of transatlantic litigation, and WPL *won* the central question. A defensible position is not the same as an affordable one.

**Always:**

1. **`PROVENANCE.md` from commit #1.** For every piece of format knowledge: source URL, its licence, date accessed, who used it. This is the independent-derivation record and it costs an hour.
2. **`TRADEMARKS.md`** with the nominative-use disclaimer.
3. **`NOTICE`** for every inherited BSD/MIT/Apache component and every dataset.
4. **`/legal/archive/` with dated captures**, made in week 1 while they are live: the SnapGene ToS, the "convert file formats" page listing the ~24 competitor formats SnapGene itself imports (a self-refuting position for any complaint that reading `.dna` is illegitimate), both USPTO TSDR records, `rebase.neb.com/rebase/rebhelp.html` and `rebcit.html`, and the Autodesk/ODA settlement statement.
5. ~~**Talk to Bar-Ilan's technology transfer office BEFORE the first public commit.** University IP assignment policy derails more academic OSS releases than vendors do. Get written clearance for Apache-2.0.~~ **Withdrawn 2026-08-06.** No technology-transfer or university-IP clearance was sought, and none is planned. The repository is public and the code is released MIT OR Apache-2.0; that is the settled position, not an interim one pending an office's answer. Nothing else in this list depended on it — the provenance record, the dated archive and the trademark discipline are what carry the argument in §9, and they stand unchanged.
6. **Prefer generic chemistry names.** "Homology-overlap assembly" not Gibson Assembly®; "isothermal overlap assembly" not NEBuilder HiFi®; "Type IIS assembly" not Golden Gate-as-a-brand; "BP/LR recombination" not Gateway®. Simulating a laboratory method in software is not practising the method, but the names are registered marks of NEB, Takara and Thermo Fisher.
7. **Do not clone SnapGene's distinctive map visual style pixel-for-pixel.** Trade dress is a real theory. Our map should be *better*, not identical.

### 9.3 Nominative fair use — the exact wording

*New Kids on the Block v. News America* (9th Cir. 1992): (1) the product is not readily identifiable without the mark; (2) only so much of the mark is used as reasonably necessary; (3) nothing suggests sponsorship or endorsement.

**SAFE:** "Reads and writes SnapGene `.dna` files." "Imports files created by SnapGene." An accurate, non-disparaging feature-comparison table.

**UNSAFE:** OpenSnapGene, SnapGene Lite, FreeSnapGene, SnapGene-NG. "SnapGene-compatible" as the *product name* rather than a descriptive sentence. Their logo, icon, colour scheme, trade dress or screenshots in the README. Unverifiable knocks ("SnapGene is slow/buggy").

**GREY BUT LOW RISK:** `"snapgene"` as a format identifier string in an API. Biopython already ships `SeqIO.parse(f, "snapgene")` and PyPI has hosted `snapgene-reader` for years, unchallenged.

### 9.4 What goes in the README

```markdown
# Polylinker

A free, open, offline plasmid editor.

Polylinker reads and writes GenBank, FASTA, GFF3/BED, ABIF (.ab1) and
SnapGene .dna files. Your sequences stay on your computer. There is no
account, no cloud, and no telemetry.

## Compatibility

Polylinker reads SnapGene .dna files, including files produced by
SnapGene Server (as distributed by Addgene). It writes .dna for the
common case; see docs/snapgene-compatibility.md for exactly what is and
is not preserved. GenBank export is lossless for everything Polylinker
models, and SnapGene reads GenBank.

## Correctness

Polylinker publishes its pass rate against polylinker-bench, a public
CC0 truth set of cloning operations that any tool can run:
  <link>   Current: 1,412 / 1,418 (99.6%)
Every computation links to its algorithm, parameters and citation.
Where our numbers differ from another tool's, the methods page explains why.

## Annotations

Auto-annotation uses polylinker-features (CC BY 4.0), the first openly
licensed common-features database. Every annotation records its source
database, accession, licence, percent identity and the database version
that produced it.

## Licence

Apache-2.0. Restriction-enzyme data is REBASE, distributed under its own
terms — see data/rebase/NOTICE. You are not being charged for the REBASE data.

## Trademarks

SnapGene is a trademark of GSL Biotech LLC. Benchling, Geneious, Gibson
Assembly, NEBuilder, In-Fusion, Gateway and TOPO are trademarks of their
respective owners. This project is not affiliated with, endorsed by, or
sponsored by GSL Biotech, Dotmatics, Siemens, or any other company named
here. References to these marks are nominative and descriptive only.
```

### 9.5 If a demand letter arrives

Do not reply ad hoc. Do not take the code down reflexively. Do not admit anything about how the format was learned. Route it to a lawyer, and contact **EFF** (US), **Software Freedom Conservancy**, or **FSFE** (EU) — all three take exactly this fact pattern, and the publicity asymmetry strongly favours a free academic tool over a €5B acquisition.

**Lawyer required, not optional, before any of:** running any decompiler on a SnapGene binary; signing anything with Dotmatics; accepting grant money or institutional funding tied to the project; offering any paid or hosted version; finalising the project name and any comparison table intended for publication.

*(One caveat on that list, which the source research got backwards: it flags "accepting grant money" as a legal hazard requiring counsel. Grant funding is the **only demonstrated cure** for the attrition that kills projects in this domain. Get counsel on the IP-assignment terms of the specific grant, certainly — but do not treat funding itself as a risk to be avoided.)*

---

## 10. Risks

Ranked by **probability × impact**, each with a mitigation and an owner.

### 1. Maintainer attrition — the documented cause of death in this field
**Probability: high. Impact: fatal.**
Every dead project in the survey — GENtle, Serial Cloner, ove-electron, GenoCAD — died of single-maintainer attrition, not technical failure. ove-electron had complete three-platform packaging scaffolded and still shipped nothing after v1.5.5 in December 2022.
**Mitigation:** Structural, not aspirational. ~~(a) A fiscal/legal host from month one so signing certificates, domains, the updater key and grant money belong to an entity, not a person — **Open Bioinformatics Foundation** is the closest cultural fit and already hosts Biopython; NumFOCUS and Software Freedom Conservancy are alternatives.~~ ~~(b) Two maintainers with commit and release keys before v1.0.~~ **(a) and (b) withdrawn 2026-08-06** — see §11.1. No fiscal host is being sought and the two-maintainer gate on v1.0 is dropped. Neither existed, so striking them removes a plan and not a protection; but it does mean this risk is less mitigated than a list of five items suggests, and the honest statement is that (c), (d) and (e) are the whole of it. The release key remains a GitHub Actions secret in one person's control, with no revocation channel — see risk 9. (c) The layered architecture: if the app dies, `polylinker-features`, `polylinker-bench` and `@polylinker/circular-map` survive and were worth building alone. (d) A written handover/archival policy so the project can die *gracefully*. (e) A schedule that is not 4× optimistic — see risk 2.

### 2. Schedule optimism
**Probability: high (it already happened). Impact: severe.**
The source research budgets 4–6 months for v1 on the fork path and 3–6 weeks for the `.dna` writer. Honest estimates are 12–15 months and 8 weeks, with lossless history round-tripping at 4–8 months more. A 4× optimistic schedule is precisely how risk 1 materialises: the developer burns out at month 8 having shipped nothing demoable.
**Mitigation:** Ship the three standalone artifacts first — each is independently complete at weeks 3, 6 and 14, so there is a *result* long before there is an app. Cut v1 scope hard (§3.3). Re-estimate at every milestone and publish the slip.

### 3. Silent scientific wrongness
**Probability: medium. Impact: fatal and unrecoverable.**
An off-by-one in origin-spanning coordinates, a wrong IUPAC expansion, a missed methylation-blocked site, a frame error, a mis-called Gibson junction. Unlike a crash, **nobody reports it** — they just clone the wrong construct and never come back, and they tell their lab meeting.
**Mitigation:** The full §7.12 apparatus, built *before* features: `cdseguid` as the assertion primitive, hazard tiering to allocate test budget, four test layers, `polylinker-bench` published with the pass rate. Plus an in-app "**this answer looks wrong**" button that captures the input file and the operation — because that is how tier-1 bugs will actually reach you when telemetry is off the table.

### 4. Silent data destruction on `.dna` write
**Probability: medium-high. Impact: severe.**
A writer that drops history trees, embedded `.ab1` alignments, uracil positions or custom colours while appearing to succeed. **sgffp is doing this today.**
**Mitigation:** Byte-preserving passthrough for every unrecognized block **plus** ADR-4's coordinate-taint model (drop-and-report, never preserve-and-stale) **plus** the per-save compatibility report **plus** byte-diff round-trip CI on a real corpus.

### 5. The wedge is gone / the demand assumption is untested
**Probability: medium. Impact: strategic.**
Three research areas designated `.dna` write as the flagship. sgffp claimed it in June 2026, and a closed competitor (PlasmidStudio) already ships import *and* export as a freemium web app. Meanwhile the research's own ranking puts "open a map someone sent you" far above "simulate cloning," and nobody asked twenty bench scientists whether they would switch for `.dna` write.
**Mitigation:** Already applied — the flagship in this plan is the annotation database, the benchmark and the map, with `.dna` write as table stakes. Plus: 20–30 structured interviews in week 1 (§12), and cheap quantitative proxies nobody tried — conda/PyPI/npm download curves for pydna, `snapgene_reader`, plannotate, `@teselagen/ove` and seqviz; **asking Addgene directly for the .dna-vs-GenBank download split**, which would settle how load-bearing the format actually is; university software-catalogue listings; YouTube tutorial view counts; and SnapGene crack/keygen search volume as a direct unmet-demand signal.

### 6. Native-sidecar packaging swallows a quarter
**Probability: high if the pLannotate stack is on the critical path. Impact: severe.**
BLAST+, DIAMOND and Infernal each mean cross-compilation for Windows x64, macOS x64, macOS arm64 and Linux; **individual code-signing of every embedded executable** because macOS notarization rejects unsigned nested binaries; hardened-runtime entitlements; Flatpak/Snap sandbox declarations for subprocess execution; and a per-binary licence audit. Plus a multi-hundred-MB database, in an app promising zero-admin offline install.
**Mitigation:** §7.7's tiering. Tier 1 auto-annotation is pure Rust/WASM with no sidecars. The heavy stack is an opt-in v2 tier. And the Rust port of pydna (ADR-5) removes the Python-runtime-in-Tauri problem entirely.

### 7. Linux WebKitGTK rendering bugs you cannot reproduce
**Probability: high. Impact: moderate.**
Tauri ships a dedicated *Linux Graphics Issues* page for blank windows, resize flicker and resize crashes (mostly NVIDIA); WebKitGTK renders fonts bolder and masks the WebGL renderer string.
**Mitigation:** Figure export goes through `resvg` in Rust, so the *deliverable* is engine-independent. Flathub is the recommended Linux channel (pinned GNOME 46 runtime = pinned WebKitGTK). CI screenshot-diffs on Ubuntu 22.04 and Fedora. Documented pivot to Electron if the first three field bugs are Linux rendering.

### 8. Biosecurity and dual-use exposure — **not mentioned once in ~30,000 words of research**
**Probability: low near-term. Impact: severe and reputational.**
A free, unrestricted tool whose purpose is designing novel constructs and exporting them to synthesis vendors sits directly inside the nucleic-acid-synthesis screening regime. An unfunded OSS project that ships "design anything, export an order" with no screening hook is carrying a liability it has not costed — and it is the obvious attack line against exactly this kind of tool.
**Mitigation:** v2 integration of **IBBIS's Common Mechanism** (`commec`, MIT, conda-installable, actively developed by IBBIS staff, reference DBs at `databases.commec.io`) as an **optional local screen before synthesis export** — HMM biorisk profiles, regulated-pathogen matching, benign-sequence filtering. No plasmid editor has one: not SnapGene, not Benchling, not Geneious. This is a genuine first, it preempts the attack, and biosecurity philanthropy is active precisely where science-software philanthropy has closed (see §11). **Decide deliberately — integrate, hook, or explicitly decline with a stated rationale in the README — but do not leave it unmentioned.**

### 9. Parser as attack surface
**Probability: low-medium. Impact: severe.**
A `.dna` file is untrusted binary arriving by email, containing LZMA-compressed history, zlib-compressed XML attachments, BGZF/BAM trace alignments, multiple XML payloads, and attacker-controlled 4-byte length fields feeding allocations and index arithmetic. Round-trip fuzzing is planned in the research; **security fuzzing is not mentioned at all.**
**Mitigation:** §5.5. `cargo-fuzz` in CI from week 3. Decompression caps. XXE off. Refuse-with-error rather than allocate-on-trust. Plus (**half of this no longer applies and half of it now applies more than when it was written; see `docs/RELEASING.md`**) the Tauri shell threat model (CSP, IPC surface) — *not applicable: there is no Tauri shell and no webview* — and **updater key custody** — *applicable, and live since 2026-08-06*. Tauri's update signature "cannot be disabled," so one key compromise pushes arbitrary code to every install. The conclusion outlived the premise: there is now a release Ed25519 key, it is compiled into `pl` and `polylinker` (not into `pl-mcp` or the Python module, which cannot update anything), and `pl update` accepts a release only if that key signed the manifest — so one compromise pushes arbitrary code to everyone who runs it, and there is no revocation channel, by design. The private half is a GitHub Actions secret and is on no developer machine. ~~Still owed: the fiscal host, and a documented rotation procedure.~~ **Amended 2026-08-06:** the fiscal host is withdrawn (§11.1), so custody is not on its way to an entity and this is the arrangement rather than a way station — one key, held by one person through one repository secret, with no revocation channel. A documented rotation procedure is still owed and is the one thing here that would actually help. `crates/pl-update/src/lib.rs` records the non-survival of key compromise as a known cost rather than an oversight.

### 10. Windows code-signing *eligibility* (not cost)
**Probability: medium. Impact: moderate but adoption-suppressing.**

> **NO LONGER A RISK, 2026-08-06 — it is a settled cost.** A risk is something that
> might happen and can be mitigated. Code signing was removed from the roadmap on
> this date, so the eligibility question below has no answer to wait for: the
> builds are unsigned, on every platform, permanently until somebody decides
> otherwise. This entry is kept for the reasoning, and because the *cost*
> paragraph is still true and still has to be paid — but read it as a description
> of a price, not as an outstanding item. Nothing in `docs/RELEASING.md`,
> `SECURITY.md` or the shipped readmes about what an unsigned build costs the
> user changes; that text is a disclosure, and this decision makes it permanent
> rather than provisional.

~~Azure Artifact Signing is $9.99/mo Basic — but restricted to **verified US/CA/EU/UK businesses and self-employed individuals**. A Bar-Ilan lab project or an unincorporated OSS org may simply not qualify. The fallback EV certificate is ~$400/yr plus a hardware token that cannot live in GitHub Actions.~~ Unsigned Windows binaries plus SmartScreen warnings measurably suppress adoption in exactly the locked-down institutional environments the primary persona inhabits. **That has not been mitigated and will not be.** It is the price of the decision, and the project's answer to it is to explain the dialog rather than teach the user to click past it.
**Mitigation:** ~~Check eligibility in week 1, before it is a surprise. Pay Apple's $99/yr on day one — unavoidable, and macOS is heavily represented in wet labs.~~ **Both withdrawn 2026-08-06** — there is no eligibility question left to answer and no Apple Developer membership is being bought. ~~Ship Windows unsigned *initially*~~ **Ship unsigned, permanently and on all three platforms** — the word "initially" is the one this decision changes — with published SHA-256 sums, which is done and is what `SHA256SUMS.txt` and its Ed25519 signature are. GitHub Artifact Attestations / Sigstore provenance is *not* built and is *not* withdrawn; it remains an open idea, and it is not code signing. The remedy the user actually gets is the shipped text: `README-MACOS.txt` and the release notes carry the `xattr -d com.apple.quarantine` command with its explanation, `README-WINDOWS.txt` explains what SmartScreen's warning means without ever printing the words for the click that dismisses it, and every one of the shipped readmes says what the checksum does *not* prove. `tools/ci.ps1` fails the gate if any of that goes missing. ~~Configure `bundle > windows > signCommand` from the start~~ — there is no Tauri bundler; `tools/release.ps1` takes `-WindowsCert` and calls `signtool` directly, resolving it out of the Windows SDK, so signing could still swap in later without touching the build. ~~The fiscal host (risk 1) may also solve the eligibility problem.~~ There is no fiscal host and none is planned (§11.1).

### 11. Upstream dependency risk on tg-oss
**Probability: medium. Impact: moderate.**
`@teselagen/ove` is maintained but decelerating: last commit 2026-05-22, last release 0.8.42 on 2026-04-16, 25–26 open issues, an "upgrade to Blueprint v5" issue open since May 2025. It exists as a byproduct of a commercial company's needs; the predecessor repo was already deprecated once. *(Note: the research's "bus factor 1 / tnrich has 81%" framing is not supported by the recent log, which shows nine distinct contributors in 2026. "Maintained but decelerating" is the honest read.)*
**Mitigation:** We depend on OVE only for the row/sequence view, and we own the circular map — the hard, differentiating half — outright from day one. If tg-oss stalls, we vendor the RowView geometry (a few thousand lines of virtualization and range math) and continue. Contribute enough to earn commit rights if possible.

### 12. Legal — trademark, contract, jurisdiction
**Probability: low. Impact: high.**
Trademark is the most likely thing to actually generate a letter (Autodesk/ODA). Live incontestable US Reg. 5,902,106 (Class 42) plus common-law rights plus **[VERIFY: EUIPO/WIPO Madrid registrations were never checked — TMview fetch failed. Check before the name is fixed.]** Contract is sharper than copyright in the US. And **[VERIFY — the top legal open question]: is Israeli Copyright Act 2007 §24 waivable by contract?** The EU expressly voids contrary terms (Art. 8); the US expressly permits waiver (*Bowers*, *Davidson*); Israel appears unsettled and lacks an express anti-waiver provision. For a Bar-Ilan-domiciled developer, the US and EU analyses are largely academic — **this is the jurisdiction that actually governs**, and it is the least-verified claim in the entire research. Requires an Israeli IP lawyer.
**Mitigation:** §9 in full. Never accept the EULA. Never decompile. Never ask for the spec. Archive evidence now. ~~Talk to Bar-Ilan TTO before commit #1.~~ **Withdrawn 2026-08-06** — technology-transfer clearance is not planned work (§9.2, "Always" item 5), and it was never a mitigation for *this* risk anyway: nothing a university IP office says changes GSL Biotech's trademark position or the Israeli §24 question, which is the actual open item here and still needs an Israeli IP lawyer.

### 13. Accessibility and i18n retrofit cost
**Probability: certain if deferred. Impact: moderate.**
Retrofitting i18n into a React app *plus* an SVG label-layout engine is brutal. CJK text metrics break naive label collision-avoidance. RTL matters for a Hebrew/Arabic user base and is directly relevant to a Bar-Ilan project. Accessibility is a soft procurement blocker (VPAT / EN 301 549) for the pharma and core-facility deployments in the roadmap.
**Mitigation:** Externalise strings from commit 1 even though v1 is English-only. Adopt Okabe–Ito with redundant non-colour encoding from the first map. Design keyboard operation of the sequence editor in, not on. Add non-Latin feature names to the `.dna` round-trip test corpus — the XML payloads are UTF-8 and **no existing parser test covers this.**

### 14. Feature-parity treadmill
**Probability: certain. Impact: moderate.**
SnapGene ships on a roughly quarterly cadence and is actively modernising its UI. Users will benchmark against all 45 user-guide sections, and partial parity reads as "not ready."
**Mitigation:** Do not chase parity. The durable advantages are **openness, API access, data ownership, auditable annotations, published correctness, and price** — none of which SnapGene can match without becoming a different company. Say this in the positioning. And write down now which parts of the strategy survive each plausible Dotmatics counter-move: a free tier that includes editing (kills "we beat the free Viewer"), publishing the `.dna` spec or an SDK (kills the interop wedge, and worse, arrives wrapped in an NDA), or shipping a real scripting API (kills the computational-lab argument). The three artifacts survive all three. That is why they are the plan.

---

## 11. Sustainability

### 11.1 Maintainership

> **WITHDRAWN, 2026-08-06 — the first two bullets below are no longer plans.**
> The project owner removed the fiscal host and the two-maintainer release gate
> from the roadmap on this date. They are struck rather than deleted because one
> of them was written down here as "non-negotiable", and a reader six months from
> now is entitled to see that it was a gate and that it was dropped deliberately,
> not quietly missed. Nothing replaces them. The consequences are real and are
> stated where they bite: the release key is one GitHub Actions secret held by one
> person with no revocation channel (risk 9), and risk 1 — maintainer attrition,
> the documented cause of death in this field — now rests entirely on the layered
> architecture, the handover policy and an honest schedule.

- ~~**Fiscal host from month one.** Open Bioinformatics Foundation (best cultural fit, already hosts Biopython), NumFOCUS, or Software Freedom Conservancy. The host holds: signing certificates, domains, the Tauri updater signing key, the GitHub org, and any grant money. This is the single structural fix for the documented cause of death.~~ **Withdrawn 2026-08-06.** No host is being sought. There are also no signing certificates for one to hold — see §10 risk 10.
- ~~**Two maintainers with commit and release keys before v1.0.** Non-negotiable gate on the v1.0 release, not an aspiration.~~ **Withdrawn 2026-08-06.** v1.0 does not wait on a second maintainer. Wanting one is not withdrawn — §11.5 still counts it as one of the three measures of whether this worked — but it is an aspiration again, and this document should not be read as promising a release gate that no longer exists.
- **DCO, not CLA.** Lower friction; sufficient for provenance. ~~(Revisit only if the fiscal host requires otherwise.)~~ (There is no fiscal host to require otherwise; DCO stands unconditionally.)
- **A support channel that is not GitHub Issues.** Bench scientists email and post to forums; they will not open an issue. Run a mailing list or a Discourse, and triage from there into issues.
- **A triage rota and a response-time promise you can actually keep** — "we read everything within a week, we fix tier-1 correctness bugs within a month, everything else is best-effort." Under-promise.
- **A written handover and archival policy.** Projects should be able to die gracefully.

### 11.2 Funding — the landscape has changed

**The named path in the research is closed.** CZI EOSS Cycle 6 ($11.7M, with Kavli and Wellcome) shows status **Closed** with no announced successor. NLnet's currently open funds are NGI TALER (privacy-preserving payments) and NGI Fediversity (hosting) — neither covers scientific research software, though grants are €5k–50k on a rolling deadline on the 1st of every even month. NCI ITCR's entry page no longer surfaces live NOFOs and was last updated 2024-06-17. **"We will get a grant" is not currently a plan.**

Live instruments, and what each implies about product shape:

| Source | Fit | What it implies |
|---|---|---|
| **Wellcome Trust** and **Kavli Foundation** directly | Good — they funded EOSS cycle 6 and may fund outside it | Open-science infrastructure framing. Leads with the **database** and the **benchmark**, not the app. |
| **ELIXIR implementation studies** | Good | European, interoperability-focused. Leads with **file-format interop** and the **library tier**. |
| **Sloan Foundation** (research software) | Good | Sustainability of scientific software. Leads with **maintainership and governance**. |
| **Biosecurity philanthropy** (Open Philanthropy, NTI\|bio, IBBIS-adjacent) | Emerging and active where science-software money has closed | Requires the **Common Mechanism integration** (risk 8) to be real, not aspirational. Changes v2 priority. |
| **Institutional sponsorship** — Addgene, synthesis vendors (Twist, IDT, GenScript), Plasmidsaurus | Underexplored | Parties with a direct commercial interest in a free, high-quality design tool that exports orders. Addgene in particular is the largest distributor of `.dna` files and the most credible partner for legitimising an alternative. |
| **Paid support contracts / validation packages for regulated labs** | Later | Commercially neutral by the *SAS v. WPL* test — no closed tier, no product substitution. |
| **GitHub Sponsors / Open Collective** | Baseline | Covers a domain. ~~And the $99/yr Apple fee~~ — there is no Apple Developer membership and no certificate to fund, by decision rather than for want of $99 (§10 risk 10). Not a salary. |

**Choose the funder before the architecture, not after.** Each implies a different emphasis, and it is much cheaper to lead with the database because a funder asked for it than to retrofit that framing onto a finished app.

### 11.3 Scientific credibility

**JOSS will not publish the app.** Its scope excludes "minor utility packages… and single-function packages," requires software "sufficiently useful that it is likely to be cited" and "at least six months of public history prior to submission, with evidence of releases, public issues/pull requests," and treats GUI/web tools as generally out of scope unless they "expose a core library through web interface" or demonstrate high architectural rigor. The research's implicit plan — build the app, then publish a paper about it — does not work.

The venues that do work, matched to artifact:

| Artifact | Venue | Timing |
|---|---|---|
| `polylinker-bench` + the correctness methodology | **A benchmark/validation paper** (Bioinformatics, NAR, or GigaScience) | **First.** Citable, needed by the field, and does not require the app to exist. This is the credibility anchor. |
| `polylinker-features` + the clean-room build pipeline | **NAR Database issue** | After 2–3 release cycles. |
| `pl-core` / `pl-clone` / the ABIF crate / `@polylinker/circular-map` | **JOSS** (libraries, ≥6 months history) | Each separately. |
| The application | **Bioinformatics Applications Note** | Last, and only if it has real users. |

Additionally: cite every algorithm and dataset in-app, next to the number it produced. That is both good practice and the answer to "why is your Tm different."

### 11.4 Community

The population that produces contributors (people who write code) and the population that produces users (people who hold pipettes) are different. Serve both, differently:

- **Contributors** come from the library tier: the Rust crates, the PyO3 wheel, the CLI, the MCP server, the standalone circular-map package. Publishing these separately is the recruitment strategy.
- **The database is the community's front door.** A PR template requiring source, accession, licence and DOI turns curation into a low-barrier contribution that a bench scientist with no coding skill can make — and every accepted PR is a person invested in the project. This is the single best community mechanism available, and it is also the flagship. That alignment is not an accident; it is the reason to build the database first.
- **Users** come from teaching. Free course materials, a tutorial written for a first-year graduate student (not a developer — this was a literal complaint about a competing tool within days of its launch), and video. SnapGene's tutorial library is a real moat and the cheapest part of it to match.
- **Documentation ships offline inside the app**, in four modes (tutorial / how-to / reference / explanation), with a methods page per computation.

### 11.5 The measure of success

Not downloads. Three things:

1. **Is `polylinker-bench` run by someone other than us?** If SnapGene, Benchling or pydna publishes a pass rate against it, the field is permanently better and we caused it.
2. **Has `polylinker-features` replaced `snapgene.csv` in pLannotate?** That is the concrete measure of the legal and scientific contribution.
3. **Is there a second maintainer?** Everything else is downstream of this. (A measure, not a gate — the "two maintainers before v1.0" rule was withdrawn on 2026-08-06; see §11.1. Nothing waits on this being answered yes.)

---

## 12. The first two weeks

A literal, ordered task list for one developer starting Monday. Ten working days. Ends in something demoable.

The first three days are deliberately not code. Two of them run experiments that could invalidate the plan, and one starts legal clocks that take weeks to clear. Doing them first is the cheapest thing in this document.

### Week 1

**Monday — legal clocks and evidence**
1. Create the repo (private for now). `LICENSE` (Apache-2.0), `NOTICE`, `PROVENANCE.md`, `TRADEMARKS.md`, `CONTRIBUTING.md`. `CONTRIBUTING.md` includes, verbatim: *"Contributors who work on file-format code must not be SnapGene licensees and must not install SnapGene. Do not run any decompiler, disassembler, `strings`, or debugger against a SnapGene binary. Do not request format documentation from Dotmatics. Record the source, licence and access date of every piece of format knowledge in PROVENANCE.md."*
2. ~~**Email Bar-Ilan's technology transfer office.** Ask for written clearance to release faculty-developed software under Apache-2.0, and whether prior disclosure is required. This takes weeks; start it now.~~ **Withdrawn 2026-08-06** — see §9.2. No clearance is sought and the repository is public regardless, so this is not a clock that is still running.
3. **Email `rebadmin@neb.com`** (Dana Macelis). Ask for a one-paragraph written confirmation that bundling `emboss_e`/`emboss_s`/`bairoch` inside a free open-source application, under an Apache-2.0 codebase with a NOTICE quoting the grant, is within the permission on `rebhelp.html` — **and ask whether they would consider re-stating that permission as CC BY 4.0**, specifically because the "not being charged for the data" condition conflicts with OSD #1 and will get the dataset classified non-free by Debian and Fedora. A dated email in the repo is worth more than any legal analysis.
4. **Archive to `/legal/archive/` with dates** (WARC or dated PDF): SnapGene ToS; the "convert file formats" page listing the ~24 competitor formats SnapGene imports; both USPTO TSDR records; `rebase.neb.com/rebase/rebhelp.html` and `rebcit.html`; the Autodesk/ODA settlement statement.
5. ~~**Check Azure Artifact Signing eligibility** for whatever entity will own this. Fifteen minutes now, or an unpleasant surprise at v1.0.~~ **Withdrawn 2026-08-06** — signing is off the roadmap (§10 risk 10), so there is no eligibility to check and no surprise waiting at v1.0. The surprise it was meant to prevent has instead been accepted and written down: the builds are unsigned and every shipped readme says what that costs.
6. **Draft the user-interview script** (10 questions, 20 minutes) and send it to 30 bench scientists across PI / postdoc / grad / core-facility. Core questions: *What did you last get wrong at the bench that the software could have caught? What would make you switch? Do you ever need to send a `.dna` file back to someone, or only receive them?* Responses will trickle in over two weeks; that is fine.

**Tuesday — the two experiments that could invalidate the plan**
7. **EXPERIMENT A — the `.dna` writer question.** Obtain 20 `.dna` files (Addgene downloads and files from lab colleagues — files a licensed user creates are that user's own data, and the ToS says so). Install the **free SnapGene Viewer only** — this is a licensing decision to make consciously, because installing means accepting the EULA; if you want to preserve a maximally clean position, have an uninvolved colleague run this step. Then: parse each with sgffp → write → open the written file in the Viewer → re-export → diff. Record which files open, which are rejected, which lose blocks. **Also confirm the topology bit** (`0x01` bit 0 vs. "second bit") against a hex dump of a known-circular plasmid. Write up in `docs/experiments/001-dna-writer.md`. *This determines whether two-way SnapGene compatibility is achievable at all, and nobody has ever run it.*
8. **EXPERIMENT B — the rendering scale question.** Load a real 200 kb BAC with ~500 features into the live tg-oss OVE demo. Measure open time, first paint, scroll fps, DOM node count. Check whether the circular view silently pares annotations. Write up in `docs/experiments/002-ove-scale.md`. *This gates the largest technical assumption in the stack.*

**Wednesday — decide, then scaffold**
9. Write `docs/adr/` entries 1–6 (§14), incorporating whatever Tuesday found. If Experiment A shows sgffp's output is rejected by SnapGene, downgrade `.dna` write to "read + GenBank round-trip" for v1 and say so in the roadmap that day. If Experiment B shows OVE collapses on a BAC, drop BAC scale from the CI budget and say so.
10. `cargo new` the workspace: `pl-core`, `pl-fileio`, `pl-enzymes`. CI on GitHub Actions across Windows / macOS / Linux from the first commit — retrofitting three-platform CI is miserable.
11. Implement `pl-core`: the sequence model, `{start, length}` coordinate arithmetic with `mod L`, IUPAC tables, genetic codes 1–33, `cdseguid`/`ldseguid` via the `seguid` algorithm. **Write the property tests in the same commit as the code**, not after: `revcomp∘revcomp == id`; rotation invariance; feature-shift-on-insert.

**Thursday — read `.dna`**
12. `pl-fileio::snapgene` reader. Block table as **data**, not code. Full parse for `0x09`, `0x00`, `0x15`, `0x0A`, `0x05`, `0x06`. Byte-preserving `Opaque` for everything else, each tagged `coordinate_dependent: bool` per ADR-4.
13. Defensive parsing from the first line: per-block allocation caps, XML with entity expansion disabled, refuse-with-error on implausible lengths.
14. Differential test harness: parse the 20-file corpus with our reader, Biopython, `snapgene_reader` and `@teselagen/bio-parsers`; diff every field; print a disagreement table. **Expect disagreements on primer coordinates** — Biopython's own source comments note an extra ±1 there — and adjudicate each by hand into `polylinker-bench`.

**Friday — GenBank, both directions**
15. GenBank reader and writer. Handle `join()`, `complement()`, origin-spanning `join(4500..5000,1..200)`, multiline qualifiers (tg-oss has an open bug on exactly this), and colour qualifiers.
16. Round-trip property test: `genbank → model → genbank` is an identity on the model for the whole corpus.
17. `.dna → model → GenBank` for all 20 files. **Open the results in SnapGene Viewer and check that features, types and colours survive.** This is the v1 acceptance test for interop, being run in week 1.
18. `cargo-fuzz` target for the `.dna` parser. Let it run over the weekend.

### Week 2

**Monday — enzymes**
19. `pl-enzymes`: REBASE `emboss_e`/`emboss_r`/`emboss_s` ingest into a `data/rebase/` package with its own `NOTICE` (verbatim grant + Roberts 2023 citation + release + download date). Implement `pl fetch-enzymes` as the alternative install path.
20. Cut arithmetic: `top = match_start + fst5`, `bottom = top - ovhg`. IUPAC → character class inside a zero-width lookahead. Separate sense/antisense groups for non-palindromic enzymes. `TwoCuts` for Type IIB.
21. **Fix the circular duplication bug**: duplicate by `max(site_size, |fst5|, |fst3|, |scd5|, |scd3|) + site_size`, dedupe by `cut mod length`. **Regression test with BsaI, BsmBI, AcuI and EcoP15I sites straddling position 1** — this is where Golden Gate simulation breaks on real backbones.
22. Property tests: rotation invariance of the cut-site set; fragment lengths sum to plasmid length. Differential test against Biopython on all 20 files, and against NEB's published site counts for pUC19 and pET-28a.

**Tuesday — the circular map, part 1**
23. `@polylinker/circular-map` scaffold: framework-agnostic TypeScript, no React, SVG string out, pure functions.
24. Geometry: backbone circle, tick marks, feature arcs with arrowheads, multi-segment features as arc groups, origin-spanning arcs, enzyme-site ticks.
25. Okabe–Ito palette with redundant non-colour encoding (arrow shape, border style) from the very first commit. Retrofitting an accessible palette after users have seen the maps is a fight you will lose.

**Wednesday — the circular map, part 2 (labels)**
26. Label placement: radial tier assignment, collision detection, leader-line routing, legibility thresholds by zoom. **This is the hardest layout problem in the product and this day is a down payment, not a completion** — budget 12–16 weeks total.
27. Golden-file rendering tests: render 20 known plasmids, commit the SVG, diff on every CI run.

**Thursday — the shell**
28. Tauri v2 + Vite + React 18 + TypeScript. Wire `tauri-plugin-fs`, `tauri-plugin-dialog`, `tauri-plugin-store`.
29. `wasm-bindgen` build of `pl-core` + `pl-fileio` + `pl-enzymes`. **Verify `npm run dev` works with no Rust toolchain installed** — this is the contributor-pipeline invariant and it must be true from day one, not restored later.
30. Wire: open file → parse → model → circular map. Add a linear/row view using `@teselagen/ove`'s RowView.
31. `pl-export`: serialize the live SVG DOM → `resvg` (PNG) / `svg2pdf` (PDF) in Rust. **Never `html2canvas`.** Golden-file test the PNG output across all three CI platforms and assert byte-identity.

**Friday — the enzymes panel, and the demo**
32. Enzymes panel with set selection (Unique, Unique & Dual, 6+, Unique 6+, All). Unique cutters emphasised.
33. **The hidden-sites badge.** A persistent, unmissable indicator: *"⚠ 14 additional cut sites are hidden by the current enzyme set — show all."* Present whenever the active filter hides anything. This is the direct fix for the one documented case of this software category costing a user a month of bench time, it costs an hour, and it should exist before anything else that could be called a feature.
34. Simple restriction digest → fragment list.
35. Tag `v0.0.1`. Write `docs/experiments/003-week-2-demo.md` with screenshots.

### The demo at the end of day 10

Open a real Addgene `.dna` file in a native window on Windows, macOS and Linux. See a clean circular plasmid map with arrowed, accessibly-coloured, leader-line-labelled features. Switch to a linear view and scroll it. Pick an enzyme set and see cut sites — with a loud badge saying how many are hidden. Run a digest and see the fragments. Export the map as SVG and PDF that are byte-identical on all three platforms. Save as GenBank and open the result in SnapGene Viewer with every feature and colour intact.

That is not a product. It is proof that the hard assumptions hold, that three-platform CI works, that the export pipeline is engine-independent, and that the correctness apparatus is real — and it is enough to show a funder, a prospective co-maintainer, and thirty interviewees.

---

## Appendix A — naming

**Requirements:** no trace of "SnapGene." Not a generic English word (weak mark, bad SEO). Pronounceable. Available as a `.org` domain, an npm scope, a PyPI name, a crates.io name, and a GitHub org. Ideally meaningful to a molecular biologist without being twee.

| Name | Meaning | Collision assessment **[all VERIFY]** | Verdict |
|---|---|---|---|
| **Polylinker** | The multiple cloning site — the region where parts are joined. A precise metaphor for a tool that joins DNA parts *and* joins an ecosystem of libraries. | No known software, company or trademark. Distinctive within software; descriptive but not generic within biology. `polylinker.org` believed free. CLI `pl`. npm `@polylinker/*`. | **★ Recommended** |
| **Overhang** | Sticky ends. Short, memorable, evocative. | Common English word → weaker mark, harder SEO, possible climbing/construction app collisions. | Strong alternate |
| **Cutsite** | Restriction site. Clean, descriptive, immediately legible. | Plausibly free; verify npm/PyPI. Slightly narrow — the tool does much more than cut. | Strong alternate |
| **Anneal** | Primer binding. Elegant, single word. | Likely existing use as a package name somewhere; check crates.io and npm carefully. | Alternate |
| **Ligase** | The joining enzyme. | Nice, but "-ase" reads as a bioinformatics *algorithm*, and there may be crates. | Weak |
| ~~Vecta~~ | — | **Collision:** Vecta.io, a diagramming tool. | Rejected |
| ~~Replicon~~ | — | **Collision:** Replicon Inc., a time-tracking software company. Same industry (software), live trademark. | Rejected |
| ~~Kilobase~~ | — | **Collision:** Kilobaser (Austrian DNA-synthesizer company) — same field. | Rejected |
| ~~Helix / Origin / Amplicon~~ | — | Heavily taken (Helix editor, Helix DNA, Illumina Helix; OriginLab; Amplicon UK). | Rejected |
| ~~PlasmidForge / OpenPlasmid~~ | — | No collision, but generic, "Forge" is overused, and "Open-" prefixes read as a clone. | Rejected |

**Recommendation: Polylinker.**

**One-line description:**
> **Polylinker — a free, open, offline plasmid editor with annotations you can audit.**

**Longer tagline for the site:** *Reads your lab's real files, including SnapGene `.dna`. Annotates from an openly licensed database that cites every source. Publishes its own correctness. Never sends a sequence anywhere.*

**Before the name is fixed** — collision checks that must be run, none of which were possible from a desk: USPTO TSDR and EUIPO/TMview (in Class 9 and Class 42), npm, PyPI, crates.io, GitHub org availability, `.org`/`.io`/`.dev` domains, and a plain web search for existing biology software. Also **check EUIPO/WIPO for SNAPGENE registrations** while you are in there — the research never did, and it matters for EU-facing copy.

---

## Appendix B — decisions log

| # | Decision | Rationale | Reversal cost |
|---|---|---|---|
| **ADR-1** | Tauri v2 + React 18 + Rust core | Web UI is forced by the only reusable components; Tauri gives filesystem, associations, free signed updater; Rust compiles to 4 surfaces | Low — frontend ports to Electron unchanged |
| **ADR-2** | Append-only content-addressed op log; undo/redo and history are the same mechanism | SnapGene's history is its signature feature *and* its documented memory liability | **Very high** — cannot be retrofitted |
| **ADR-3** | Consume `@teselagen/ove` for the row view; **own** the circular map | Row view = high effort, low differentiation. Circular map = the visible quality signal, must be ours, and is a gift to the ecosystem | Medium |
| **ADR-4** | Opaque `.dna` blocks are tagged coordinate-dependent; drop-and-report on sequence edits, never preserve-and-stale | Byte passthrough preserves bytes, not meaning. Stale provenance is worse than absent provenance | Low |
| **ADR-5** | Port pydna's `Dseq` + assembly graph to Rust; pydna becomes the CI oracle | Removes Python-runtime-in-Tauri packaging entirely; keeps one language in the app; the differential harness *is* the correctness strategy | High (8 weeks) |
| **ADR-6** | Apache-2.0 code / CC BY 4.0 database / CC0 benchmark / REBASE separately packaged | Patent grant + retaliation + NOTICE; database attribution is the curators' currency; the benchmark must be runnable by literal competitors | High |
| **ADR-7** | `{start, length}` with `mod L` as the universal coordinate representation | Makes invalid circular intervals unconstructable; rotation invariance becomes a one-liner | **Very high** |
| **ADR-8** | Tier-1 auto-annotation is k-mer + `edlib` in WASM; the BLAST/DIAMOND/Infernal stack is an opt-in v2 tier | SnapGene's magic is approximate string matching, not homology search. Saves a 4–6 month sidecar-packaging workstream | Low |
| **ADR-9** | No cloud, no accounts, no telemetry, no paid tier | The only durable axis against Benchling; the only reliable defence against *SAS v. WPL*-shaped litigation | **Irreversible in practice** |
| **ADR-10** | Ship `polylinker-bench` and `polylinker-features` **before** the app | Each is independently valuable and publishable; guarantees a non-catastrophic failure mode | Low |
| **ADR-11** | The library index is a **rebuildable cache in a hand-rolled format**, not SQLite | FTS5 answers the wrong question about plasmid names, and answers it silently; the one query nobody else offers is one FTS5 cannot express | Low — reversible, see below |

#### ADR-11 in full: why not SQLite

`docs/PLAN.md` line 190 specified a SQLite index. Three architectures were
designed independently and scored by three judges each; the rebuildable
zero-dependency cache won 41.0 to SQLite's 33.0.

**The deciding reason is not the dependency.** It is that **FTS5 answers the
wrong question about this data, and answers it silently.** Verified against real
plasmid feature names:

```
MATCH '"uc"*'   against  pUC ori       ->  no rows
MATCH '"101"*'  against  pSC101 ori    ->  no rows
trigram tokenizer, any query under 3 characters   ->  no rows
plain substring ->  finds all of them
```

A plasmid name is not word-shaped, so a prefix tokenizer misses the middle of
it. A user typing `uc` would read "not in my library" where the truth is "not
asked" — the failure this project treats as disqualifying.

Meanwhile the query nobody else offers — degenerate, both-strand,
origin-wrapping motif search — is one FTS5 cannot express at all, and it costs a
`for` loop. Measured: the real corpus packs to 122 Mbase and scans in ~365 ms;
the searchable text is about a megabyte and a substring pass over it is
microseconds. There is no performance problem here for a storage engine to
solve.

**What the format buys instead.** Every failure mode degrades to "discard and
rebuild", because the file is derived: a crash leaves an orphaned temporary and
an intact index (the live file is never opened for writing); bit rot is caught
by a SHA-1 trailer verified on every open — strictly *more* than default SQLite,
whose amalgamation ships no checksum VFS; a stale layout or a changed parser
forces a rebuild by version number; concurrent writers are last-writer-wins over
two complete files, with no lock and therefore no stale-lock recovery.

**Reversal condition, written down now rather than discovered later.** The day
the index holds anything the user authored — tags, assigned folders, per-file
notes, stars, custom ordering, recent files — it stops being a cache and
"rebuild" becomes data loss. Line 144 of this plan asks for folders and recent
files in the same sentence as search, so this is one release away. At that
moment the answer is **redb** (zero transitive dependencies, pure Rust, builds
for wasm32), **not SQLite**, and that state goes in a *separate, never-rebuilt*
file keyed to a stable per-file identifier — **not** to a sequence hash, which
in an application whose main verb is "edit the plasmid" would silently detach
the label the user typed the moment they changed a base.

**Deliberately not in v0.1:** user-authored state (above); mismatch-tolerant and
gapped search, which is refused by name rather than by empty result; ranking and
relevance, since results are ordered deterministically by `(path, record,
position)`; protein and translated search; annotate-at-import, which is only as
good as `features/features.tsv` and that is 89 records at release 2026.07.28,
all 89 signed off by a curator — and 113 records as of 2026-08-12, of which 89
are signed and 24 are `proposed` and therefore not searched by default (21 were
proposed on 2026-08-10; the curator withdrew `PLF:4006` on 2026-08-11, three
Class B promoter rows were appended the same day, and a fourth, `PLF:4015`, on
2026-08-12 on the curator's instruction); and
Type IIS
enzymes, so **no Golden Gate query is advertised** — the shipped table has no
BsaI, BsmBI, BbsI or SapI, and `--enzyme BsaI` says so instead of guessing.

---

### Closing note

The single most useful thing in this document is not a technical decision. It is this: **every project that has attempted this died of maintainer attrition, and the standard mechanism for maintainer attrition is a schedule that is four times optimistic.** The source research proposed 4–6 months to v1. The honest number is 12–15. If that number is unacceptable, the correct response is not to compress the schedule — it is to build only the three standalone artifacts, ship them, publish them, and let someone else build the app on top. That outcome would still leave the field measurably better than it is today, which is more than any previous attempt in this space can claim.