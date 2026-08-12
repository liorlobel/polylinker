# Polylinker

**A free, open, offline plasmid editor with annotations you can audit.**

Reads your lab's real files, including SnapGene `.dna`. Annotates from an openly
licensed database that cites every source. Publishes its own correctness. Never
sends a sequence anywhere.

> **Status: pre-release.** The desktop app, the `pl` command line, the browser
> build, Python bindings and an MCP server all work today, across 21 workspace
> crates and 160,152 lines of Rust, 93,892 of it dependency-free (141 `.rs`
> files under `crates/` and `bins/`), with 1,945 `#[test]` functions and a
> 73-step gate (`Step` invocations in `tools/ci.ps1`) that cross-checks the
> answers against Biopython, pydna, SciPy and the SEGUID reference
> implementation. Counted 2026-08-10, and recounted on every test run since:
> tests are lines matching `^\s*#\[test\]`, the attribute at the start of a
> line, so a `#[test]` written mid-sentence in a doc comment is prose rather
> than a test; lines are `wc -l`; crates are the `members` of the root
> `Cargo.toml`; **dependency-free** is a property of a crate and not a mood —
> a member every one of whose dependencies is another member of this
> workspace. Nineteen of the twenty-one are, and the two that are not are the
> two that face outwards: `bins/pl-gui` (eframe, rfd, egui-phosphor) and
> `crates/pl-py` (pyo3). All six numbers are recomputed from the tree by
> `the_readme_headline_counts_are_the_counts_in_the_tree` in
> `bins/pl/src/main.rs`, which fails the gate when this paragraph disagrees
> with it. They are asserted rather than written down because writing them
> down did not work: the original figures here — 16 crates, ~47,500 lines,
> 812 tests, 39 steps — were two to three times stale; the hand recount that
> replaced them on 2026-08-04 — 125,788 lines, 119 files, 1,593 tests,
> 43 steps — went stale the same day it landed; and the recount after *that*
> was pinned by the test above while still calling every one of those lines
> dependency-free, counting `pl-gui`'s and `pl-py`'s as though they took
> nothing. A number a test recomputes is not the same thing as a sentence a
> test reads, which is why the adjective now has a marker of its own.
>
> **One thing here is a settled decision rather than an unfinished one**, and it
> is the one that decides whether you should trust the download: the builds are
> **unsigned**. There is no code-signing certificate and none is planned, so
> Windows and macOS do not recognise the publisher and say so — SmartScreen's
> *"Windows protected your PC"* on first run, a flat Gatekeeper refusal on
> macOS, and on a managed machine a policy that may refuse to run them at all.
> The release *manifest* is a separate question and is
> signed: `SHA256SUMS.txt` carries an Ed25519 signature made by a key whose
> public half is compiled into `pl` and the editor, which is what `pl update`
> checks before it keeps a byte. An operating system has never heard of that key, so
> it buys you nothing at the SmartScreen dialog and everything afterwards. The
> features database used to be the second entry on this list and is not any
> more — the 89 records `pl annotate` searches by default each carry a
> named curator in `features/SIGNOFF.tsv`, and an approval lapses by itself the
> moment the row it approves changes. The table also holds 24 rows a program
> added and nobody has read; those are `proposed`, they are not searched, and
> `--include-proposed` is how to see them. See
> [Where this actually is](#where-this-actually-is).

---

## Why

A third-year graduate student on a locked-down university PC cannot edit a
sequence, run an alignment, simulate a digest, or customise an enzyme set —
because those are all behind SnapGene's paid tier, and they have no budget line.
Meanwhile every open annotation tool in the field depends on a features database
that was scraped from a proprietary product and redistributed without a licence.

Polylinker exists to fix the second problem, and the first one follows.

## What is actually different

Not "SnapGene but free" — that has been tried at least five times and lost every
time. Three things nobody has built:

1. **An openly licensed, provenance-tracked features database.** Every
   annotation records its source database, accession, licence, percent identity
   and the database version that produced it. No magic, no scraped prose.
2. **A public correctness benchmark.** A CC0 truth set of
   `(input, operation, expected checksum)` triples using rotation- and
   strand-invariant `cdseguid` checksums, so two tools can be compared on
   *the molecule they produced* rather than on byte-identical output. Any tool
   can run it, including the commercial ones.
3. **A publication-quality map renderer** — a ring for a plasmid, a horizontal
   track for a PCR product, a linearised vector or a gBlock — published
   standalone under a permissive licence so seqviz, plascad, OpenCloning and
   pLannotate can all use it.

Each ships before the app, stands alone, and survives the app.

## Where this actually is

| Component | State |
|---|---|
| [`crates/`](crates) + [`bins/pl`](bins/pl) | **The Rust core and CLI.** `pl-core` (model, SEGUID checksums, sequence search), `pl-enzymes` (digestion, Type IIP and Type IIS), `pl-clone` (sticky ends, fragments, PCR, Gibson and Golden Gate), `pl-fileio` (`.dna`, GenBank, FASTA), `pl-draw` (maps as SVG, PDF, EPS and PNG — a ring for a circular molecule, a horizontal track for a linear one, from one description that all four writers consume), `pl-thermo` (melting temperature), `pl-primer` (binding sites), `pl-design` (choosing a primer pair, **no I/O**), `pl-abif` (Sanger traces), `pl-index` (the library: packed sequence store and queries, **no I/O**), `pl-scan` (the library index on disk: one of the two crates that touch the filesystem, `pl-update` being the other), `pl-features` (the features database and the annotator over it: k-mer seeding, infix alignment, six-frame protein matching, and the sign-off gate that decides which rows a release may search), `pl-update` (the compiled-in Ed25519 key that signed release manifests are checked against, and an updater that verifies a signature before it keeps a byte — reached from `pl update`, a verb you type, and from the editor's off-by-default check, which asks and never downloads; from nowhere else, and two tests read the sources to keep it that way; see [`docs/RELEASING.md`](docs/RELEASING.md)), `pl-wasm` (browser ABI), and the `pl` command. **Zero external dependencies.** |
| [`bins/pl-gui`](bins/pl-gui) | **The desktop app**, `polylinker.exe`. egui; one static binary, no webview. **File ▸ New (Ctrl+N) makes a molecule out of bases you paste in** — a gBlock from an email, a vendor's plain sequence, 300 bp out of a message — taking line breaks, a FASTA header, the numbers off a sequence listing, lower case and U; it says what it ignored, refuses anything that is not a nucleotide by naming the character and its position, and asks for circular or linear at creation because that answer changes every answer after it. Opening or pasting a molecule searches it against the 89-record features database compiled into the binary and lists what it found at the top of the Features tab **as proposals** — each with its identity *and* its coverage, whether the match was nucleotide or protein, the record's `PLF:` id, and whether a curator has ever checked that record. **Nothing reaches your document until you press Accept**, one proposal or all of them; each accepted feature is one undo step and carries the same provenance note `pl annotate --genbank` writes, from the same function. The panel says what the database has no rows for, so an absence is not read as an answer. Turn the search off with "Annotate on open"; it never asks the network either way. **Paste an oligo into the Primers tab and it shows you every place that oligo anneals** — both strands, the annealed footprint kept apart from any 5' tail, a melting temperature over the footprint alone, each site drawn on the map and boxed in the sequence view, and the site *count* stated before the list, because a primer that binds twice is a failed PCR. It is the same `pl-primer` engine `pl primers` calls, with the same defaults and the same seed bounds, and a test compares the two answers rather than trusting them to match. **The amino-acid track is something you can take away**: Copy protein (Ctrl+Shift+P) puts the selection's reading on the clipboard, the Features toolbar copies the selected feature's, and Save ▸ Protein FASTA… writes every reading in the document. Each record carries `transl_table=` and a GenBank location in its header, because Polylinker offers all 27 NCBI tables with a per-feature override and thirteen of them do not treat `TGA` as a stop — a residue string alone does not determine its own bases. A ragged length, a reverse strand, a `join()`, an internal stop and a CDS running off the end of a linear molecule are each said in the header rather than left to be noticed. Contacts nothing unless you switch on the update check under Help, which ships off and never downloads. |
| [`bench/`](bench/README.md) | **`polylinker-bench` v0.1** — a CC0 truth set, 176 cases, every expected value from an independent oracle. Polylinker scores **176/176, zero failures, nothing declined**. |
| `prototype/dna-reader.html` | **Usable today.** The same Rust core compiled to wasm32 and inlined into one HTML file: opens `.dna`, GenBank and FASTA, draws maps, digests, exports GenBank/FASTA/SVG. **Its map is the TypeScript renderer in [`packages/circular-map`](packages/circular-map), not `pl-draw`**, so it still draws a linear molecule as a ring with a gap in it — the horizontal track added to `pl-draw` on 2026-08-07 has no browser twin yet. No install, no network, no account — runs from a USB stick on a locked-down PC. Built by [`tools/build-web.ps1`](tools/build-web.ps1); not committed, because it is 257 KB of base64 that changes every rebuild. |
| [`reference/python/dna2gb.py`](reference/python/dna2gb.py) | Bulk `.dna` → GenBank converter. Superseded by `pl convert`, which also preserves sequence case (Biopython does not — see the file's docstring). |
| [`docs/DNA-FORMAT.md`](docs/DNA-FORMAT.md) | Empirical spec of the `.dna` container. Validated on a 41-file corpus (138 bp → 4.64 Mb). |
| [`reference/python/snapdna.py`](reference/python/snapdna.py) | Reader + writer, stdlib only. **Byte-exact round-trip on 41/41 files.** |
| [`reference/python/ab1_probe.py`](reference/python/ab1_probe.py) | ABIF (`.ab1`) chromatogram reader. Parses 374/394 real traces. |
| [`docs/PLAN.md`](docs/PLAN.md) | The architecture and roadmap this repo is built from. |
| [`bins/pl-mcp`](bins/pl-mcp) | **MCP server**, read-only, no dependencies — so an assistant can ask about a plasmid without being able to overwrite one. |
| [`crates/pl-py`](crates/pl-py) | **Python bindings** (PyO3, abi3), so a script already using Biopython can call the parts that are hard to get right without being rewritten. |
| [`docs/AUDIT-2026-07-28.md`](docs/AUDIT-2026-07-28.md) | A 123-agent audit of the whole workspace: 90 confirmed findings, 19 refuted, 90 of 90 fixed. Kept in the repo because the findings that mattered most were **checks that could not fail**, and that is worth being public about. |
| Windows installer | **`polylinker-<version>-windows-x64.msi`**, built by [`tools/build-msi.ps1`](tools/build-msi.ps1) from [`tools/installer/Polylinker.wxs`](tools/installer/Polylinker.wxs). Installs for you alone by default — no administrator, no elevation prompt — with "for everyone" offered for machines where you are one. The installer itself contacts nothing, and it registers no service, no scheduled task and no auto-updater: nothing it puts on the machine ever runs on its own. ([`tools/ci.ps1`](tools/ci.ps1) fails the build if any network or scheduling facility appears anywhere under `tools/installer/`.) It **adds** Polylinker to the "Open with" list for eight sequence formats and takes none of them away from SnapGene or anything else you already use. The MSI carries no file list of its own: it is generated from the same `SHA256SUMS.txt` the archive is verified against, because a second list is how a licence text stops shipping. The readable [`Install-Polylinker.ps1`](tools/installer/Install-Polylinker.ps1) still ships inside the zip for anyone who would rather run something they can read. See [`docs/RELEASING.md`](docs/RELEASING.md). |
| Signing | **Code signing: not done, and not planned.** There is no code-signing certificate and no Apple Developer ID, and obtaining them is not on the roadmap — see [`docs/RELEASING.md`](docs/RELEASING.md), which states the decision and what it costs you at the download. That is the signature an *operating system* checks, so Windows and macOS go on saying they do not recognise the publisher, and a machine under someone else's policy may refuse the binaries outright. **Manifest signing: done, 2026-08-05.** `SHA256SUMS.txt` ships with an Ed25519 signature (`SHA256SUMS.txt.sig`) whose public key is compiled into the two programs that can use it, `pl` and `polylinker` (`pl-mcp`, the Python module and the wasm build do not carry it, because none of them can update anything), so a download can be traced to whoever holds the release key rather than merely to whoever served the page. Every release page prints the OpenSSL command to check it by hand, or `pl update` does it for you — it refuses everything that key did not sign. The private half is a GitHub Actions secret and is on no machine here. |
| Features database | **113 records as of release 2026.08.12, 89 reviewed and 24 proposed.** Machine-assembled from public sources, then signed off row by row; the 89 were signed on 2026-07-28, of the 21 added on 2026-08-10 one has been withdrawn by the curator and the remaining 20 have not been read by anyone, three more were appended on 2026-08-11, and a fourth on 2026-08-12 — none of those four has been read either. A row moves past `proposed` only when `features/SIGNOFF.tsv` names it with a sha256 of its content that still matches, so an approval lapses by itself when the row changes and the shipped set shrinks without anyone deciding to shrink it. That mechanism, not the current count, is what enforces "the tool may propose and never assert" — and the 24 are what it looks like when it works: they are in the repository, they are not in what `pl annotate` searches, and `--include-proposed` is the only way to see them. **It is not comprehensive, and the gaps are not small: the 89 rows the tool searches contain no promoter, no terminator and no origin of replication** — 89 against SnapGene's 1,367 — because those classes have no automatable source that gives a defensible boundary. Ten regulatory elements — six promoters, three terminators and a poly(A) signal — are now proposed and awaiting a curator; three of the promoters (T3, araBAD and human EF-1alpha) were added on 2026-08-11 after re-measurement contradicted the reasons they had been held for, and the mouse PGK promoter was added on 2026-08-12 **on the curator's instruction**, not because any rule changed — the stage had refused to promote it itself, and issuing it is a decision the program is not allowed to take. Each was appended so that no published id moved — [`features/PROPOSED.md`](features/PROPOSED.md) is that curator's worklist, and since 2026-08-11 every row in it carries the primary source that settles it and a recommendation — sign, withdraw, or a decision only a curator can make — with the decisions first and the arithmetic afterwards. The CMV enhancer was **withdrawn** on 2026-08-11 rather than signed: the promoter half of the block it belongs to was refused on the evidence, and shipping the enhancer alone tells a reader by omission that a CMV region has no promoter. Its id, `PLF:4006`, is retired rather than freed — the row's declaration stays in the build with the reason attached, so nothing else can ever be issued under that number. Five more were built and then refused, because two independent depositors have to draw the edges where we drew them before a row may call its boundary a consensus, and lac, tac, trc, the CMV promoter and the SV40 poly(A) signal are each corroborated by exactly one; origins are still untouched ([`features/README.md`](features/README.md) enumerates the rest). The desktop app and `pl methods annotate` both say so on screen, computed from the table rather than written down, because a user who watches `AmpR` light up and sees no `ori` will otherwise conclude their plasmid has none. `pl licences` prints the live count and the attribution. |

### Getting your sequences out of `.dna`, today

The lock-in is the file format, and it is already broken. Nothing here uploads
anything.

```bash
cargo build --release
target/release/pl convert  "plasmids/**/*.dna" --to genbank -o converted/
target/release/pl info     plasmid.dna
target/release/pl digest   plasmid.dna --unique
target/release/pl blocks   plasmid.dna      # what the container is made of
target/release/pl checksum plasmid.dna      # SEGUID v2 identity
target/release/pl design   plasmid.gb --region 1204..2551   # a PCR primer pair
```

### Why there is a checksum command

A plasmid has no canonical form: rotation, strand choice, annotation order and
feature-name spelling are all free. So "is this the same molecule?" cannot be
answered by comparing files, and a GenBank diff produces false failures while
hiding real ones.

`cdseguid` is invariant under exactly the freedoms a circular duplex has, and
nothing else. Converting a `.dna` to GenBank and checksumming both gives the
same answer — which is what makes the conversion *provably* lossless rather than
merely plausible:

```text
pACYC184-Ppho-fab2-6his.dna   cdseguid=vdhk71L0TZ6x3sJznO5P3_jLRlw
pACYC184-Ppho-fa.gb           cdseguid=vdhk71L0TZ6x3sJznO5P3_jLRlw
```

No toolchain on the machine that has the files? Build the browser tool once and
carry it:

```powershell
.\tools\build-web.ps1        # -> prototype\dna-reader.html, one self-contained file
```

Open it, drop a file on it, press **Save GenBank**. No install, no admin rights,
no network — the wasm core is inlined, so it works over `file://` from a USB
stick.

GenBank is plain text and is read by ApE, UGENE, Benchling, Biopython and
SnapGene itself, so converting costs you nothing and un-strands your data.

### Building

`pl-core` has no dependencies, so `cargo build` needs no network. A linker is
the only external requirement.

- **Linux/macOS** — works out of the box.
- **Windows** — needs a linker. `rustup` does not ship one, and the failure is a
  confusing `linker 'link.exe' not found` deep in a build log. Install the MSVC
  toolset:

  ```powershell
  winget install --id Microsoft.VisualStudio.2022.BuildTools --exact --override "--quiet --wait --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.26100"
  ```

  A `--profile minimal` rustup install also omits clippy and rustfmt:
  `rustup component add clippy rustfmt`.

Verify a build end to end, including against real files:

```powershell
.\tools\verify.ps1 -Corpus "C:\path\to\your\plasmids"
```

That script checks for the linker first and prints the fix rather than letting
cargo fail obscurely.

Built and verified natively on Windows (MSVC 14.44, Rust 1.97.1) and on Linux.

### What has been proven so far

- The `.dna` container is fully solved for reading. All 41 corpus files parse
  with **every byte accounted for**.
- Writing round-trips **byte-exactly** on all 41 files.
- Blocks 2 and 3 — jointly **78% of a typical file** — are regenerable caches
  (an enzyme recognition table and its cut-position index), not user data. A
  writer can omit them.
- Restriction digest agrees with Biopython on **5,587 cut sites across 33 real
  plasmids, with zero disagreements**, including circular wraparound.
- Parsing a 4.64 Mb genome takes **13 ms**; a 50-enzyme digest over it takes
  **287 ms in pure Python**. Compute is not the bottleneck — rendering is.
- `.dna` → GenBank conversion is **faithful on 41/41 files** — sequence,
  topology, feature coordinates, strand, multi-segment joins, colours and
  primer binding sites all verified against the source.
- The Rust reader and the independently written Python reader **agree on 41/41
  files**, across 79 features and 16.9 Mb of bases. Two implementations of an
  undocumented format agreeing is evidence; one agreeing with itself is not.
- **Biopython accepts every GenBank file `pl` writes** (41/41) and reads back
  matching coordinates, strands and joins. Biopython is the strict foreign
  parser standing in for ApE, UGENE and Benchling.
- `pl` **preserves sequence case; Biopython destroys it** in both directions —
  its GenBank writer lower-cases and its reader upper-cases, verified with
  Biopython on both ends. Seven contigs in the corpus carry soft-masked bases
  that only survive the Rust path.
- GenBank parsing covers **303/303 real files** and 2.26 M features, including
  three that declare a length but ship no bases, eight standalone annotation
  tracks with no `ORIGIN` block at all, and one that is 148 bytes of nothing —
  all classes Biopython either mis-reports or refuses outright.
- Content sniffing found **20 files whose extension lies**: four SCF and sixteen
  ZTR chromatograms named `.ab1`. This is why detection never trusts the
  extension.
- **SEGUID v2 checksums agree exactly with the reference implementation** (the
  Python `seguid` 0.2.1) on **148/148 sequences across all five forms and
  14.5 Mb** — generated palindromes, homopolymers, periodic and near-periodic
  sequences, and real molecules from the corpus.
- Rotating a real plasmid **preserves its `cdseguid`**, checked over 36
  rotations of 9 plasmids. The restriction-site set of a circular sequence is
  invariant under rotation, tested exhaustively over every rotation of several
  sequences and all 58 enzymes — the property `docs/PLAN.md` calls "where origin
  bugs live".
- On the 4.64 Mb *E. coli* genome (a 17.7 MB `.dna`): **70 ms** to parse and
  report and **590 ms** for a 50-enzyme digest on Linux; **103 ms** and
  **1,195 ms** natively on Windows. 30 MB peak RSS.
- The whole suite passes on **Windows (MSVC) and Linux**, from the same source.
- The wasm build and the native binary **agree on 41/41 files**, and the GenBank
  they write is **byte-identical** on all 41. The browser page holds no parser of
  its own; there used to be a second implementation in JavaScript, and two
  implementations of an undocumented format is two things to keep correct.
- The wasm module declares **zero imports** — no JS glue, no `wasm-bindgen`, no
  runtime to trust.

### What the desktop window looks like, and why

The chrome -- colour, spacing, typography, rounding, shadow -- is a design system
shared with the author's other `eframe` application, so the two programs read as
one piece of software. It follows your desktop's light or dark setting by
default; the toolbar switch overrides that and is remembered, and **Help > Follow
the desktop's theme** hands it back.

Two things about it are decisions rather than taste, and both are held by tests
in [`bins/pl-gui/src/theme.rs`](bins/pl-gui/src/theme.rs):

- **The accent is the orange from the application's own icon, and it is a
  *pair*.** `#E69F00` is 2.25:1 on white -- half of what WCAG AA asks of text --
  so light mode uses `rgb(140, 97, 0)`, which is the same colour scaled by 0.609
  and therefore the same hue to a hundredth of a degree. The two swap roles with
  the theme: the bright value goes wherever it is the lighter of the two things
  being compared. No fill anywhere hardcodes its own foreground.
- **Every ink is measured against every surface it is actually painted on**, in
  both themes, and the surfaces are listed per role rather than in one blanket
  loop, so each entry can be pointed at the line of code that draws it. Two
  colours inherited from the other application did not clear AA here and were
  moved rather than copied.

Three typefaces are compiled in and none is downloaded: **IBM Plex Mono** for
sequence, **IBM Plex Sans** for everything else, and **Inter SemiBold** for
window and dialog titles only. Inter is deliberately kept out of both text
chains -- its capital `I` and its lowercase `l` are the same bare stem, and this
application's proportional text is enzyme names, where `AflII` and `Aflll` are
different answers. Every vendored face ships its licence beside the binary;
[`NOTICE`](NOTICE) records the version, the byte count and the SHA-256 of each.

The window opens at 1280 x 840 and will not go below **990 x 560** -- a minimum
set by measuring the toolbar rather than by choosing a round number: below it the
status line, which is where an export tells you what it dropped, runs out of room
entirely.

### A note on measuring a GUI from a script

If you screenshot or measure this app from a helper process on Windows, make
that process **per-monitor DPI aware first**:

```powershell
[Win32]::SetProcessDpiAwarenessContext([IntPtr](-4))
```

Without it Windows reports *virtualised* coordinates. On a 125% display a
1600×1050 window is reported as 1280×840, and a screenshot taken at those
numbers captures the top-left 80% of the window — which looks exactly like the
UI being clipped. That artifact cost real time here and produced a bug report
for a bug that did not exist. `PL_GUI_DEBUG_GEOMETRY=1` prints what the app
believes its own geometry is, which is the number to trust.

### One core, three surfaces

```
crates/pl-core ─ pl-enzymes ─ pl-fileio ─┬─ bins/pl        native CLI    (365 KB)
                                         └─ crates/pl-wasm wasm32        (150 KB)
                                                              └─ inlined into one HTML file
```

Correctness lives in one place. The CLI, the browser tool and any future GUI are
distribution, not logic.

## Try it

Open [`prototype/dna-reader.html`](prototype/dna-reader.html) in any browser and
drop a file on it. Nothing is uploaded; there is no server.

**Test .dna** writes a `.dna` from scratch that deliberately omits the two
derived cache blocks — the open experiment in
[`docs/DNA-FORMAT.md` §4](docs/DNA-FORMAT.md). If SnapGene opens it, write
support is essentially solved.

Run the checks yourself:

```bash
python reference/python/tests/test_roundtrip.py "your/**/*.dna"
python reference/python/tests/validate_digest.py "your/**/*.dna"   # needs biopython
node prototype/check_page.js "your/**/*.dna" "your/**/*.gbk"       # needs jsdom
```

## Compatibility

Polylinker aims to read SnapGene `.dna` files and to write them for the common
case. GenBank is the canonical interchange format and the default save format,
because it is lossless for everything Polylinker models and SnapGene reads it.

Where `.dna` write support is imperfect it will say so, in the save dialog, per
block. A writer that silently drops what it does not understand is the worst
possible outcome and will never ship here.

## Citing this

[`CITATION.cff`](CITATION.cff) holds the metadata, and GitHub reads it for the
"Cite this repository" button. Name the version you ran — `pl --version` prints
it and the commit, and [`CHANGELOG.md`](CHANGELOG.md) says what was in each one.

**There is no DOI.** One would come from archiving a release to Zenodo, which
nobody has done, so a citation here resolves to a repository and a version
number and not to an identifier. The `doi:` field is absent from `CITATION.cff`
rather than empty, because an identifier that resolves to nothing is worse than
none.

## Licence

**MIT OR Apache-2.0 for code**, at your option — `LICENSE-MIT` and `LICENSE`,
which is the Apache-2.0 text. Both are committed and both ship inside every
archive. Until 2026-08-06 only the Apache half was in the repository while
`Cargo.toml`, the release notes and `packages/circular-map/package.json` all
offered the choice, which is not a choice.

The features database is **CC BY 4.0** and is in this repository, in
[`features/`](features), with its own [`features/NOTICE`](features/NOTICE) that
must travel with any copy of it. The benchmark is **CC0** and is in this
repository too, in [`bench/`](bench). Neither is in a separate repository. This
paragraph said both would be, in the future tense, after both had already
landed here.

**No restriction-enzyme database is redistributed here.** What ships is 58
enzymes in `crates/pl-enzymes` — 50 Type IIP and the 8 Type IIS ones Golden Gate
needs — transcribed from published references. Their sites and cut geometry were
cross-checked against Biopython's REBASE-derived tables, which is a check and not
a source: a cut coordinate is a measurement, and Biopython's code carries a
licence while the numbers do not. See [`PROVENANCE.md` §3](PROVENANCE.md) and the
Data section of [`NOTICE`](NOTICE).

REBASE itself is the database real work wants — 58 enzymes is a teaching set,
and `pl methods digest` says so in the paragraph you would paste into a paper.
It carries terms of its own, and **it has not been licensed and is not
distributed here.** `NOTICE` records that it would arrive in a separate package
under those terms; that package does not exist. This paragraph said the opposite
until 2026-08-06 — that enzyme data "is REBASE, redistributed under its own
terms with its own `NOTICE`" — which claimed a redistribution relationship with
a database this project has never obtained, and pointed at a `NOTICE` that says
the reverse.

## Trademarks

SnapGene is a trademark of GSL Biotech LLC. Benchling, Geneious, Gibson
Assembly, NEBuilder, In-Fusion, Gateway and TOPO are trademarks of their
respective owners. This project is not affiliated with, endorsed by, or
sponsored by GSL Biotech, Dotmatics, Siemens, or any other company named here.
References to these marks are nominative and descriptive only.

See [`TRADEMARKS.md`](TRADEMARKS.md) and [`PROVENANCE.md`](PROVENANCE.md).
