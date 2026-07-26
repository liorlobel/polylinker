# Polylinker

**A free, open, offline plasmid editor with annotations you can audit.**

Reads your lab's real files, including SnapGene `.dna`. Annotates from an openly
licensed database that cites every source. Publishes its own correctness. Never
sends a sequence anywhere.

> **Status: pre-alpha.** There is no application yet. What exists today is a
> validated file-format specification, two working reference implementations,
> and a 135 KB architecture plan. See [Where this actually is](#where-this-actually-is).

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
3. **A publication-quality circular map renderer**, published standalone under a
   permissive licence so seqviz, plascad, OpenCloning and pLannotate can all use it.

Each ships before the app, stands alone, and survives the app.

## Where this actually is

| Component | State |
|---|---|
| [`crates/`](crates) + [`bins/pl`](bins/pl) | **The Rust core and CLI.** `pl-core` (model, SEGUID checksums), `pl-enzymes` (digestion), `pl-clone` (sticky ends, fragments, PCR), `pl-fileio` (`.dna`, GenBank, FASTA), `pl-wasm` (browser ABI), and the `pl` command. **Zero external dependencies**, 365 KB static binary. |
| [`bins/pl-gui`](bins/pl-gui) | **The desktop app**, `polylinker.exe`. egui; one static binary, no webview. |
| [`bench/`](bench/README.md) | **`polylinker-bench` v0.1** — a CC0 truth set, 176 cases, every expected value from an independent oracle. Polylinker scores **171/176, zero failures**. |
| `prototype/dna-reader.html` | **Usable today.** The same Rust core compiled to wasm32 and inlined into one HTML file: opens `.dna`, GenBank and FASTA, draws maps, digests, exports GenBank/FASTA/SVG. No install, no network, no account — runs from a USB stick on a locked-down PC. Built by [`tools/build-web.ps1`](tools/build-web.ps1); not committed, because it is 257 KB of base64 that changes every rebuild. |
| [`reference/python/dna2gb.py`](reference/python/dna2gb.py) | Bulk `.dna` → GenBank converter. Superseded by `pl convert`, which also preserves sequence case (Biopython does not — see the file's docstring). |
| [`docs/DNA-FORMAT.md`](docs/DNA-FORMAT.md) | Empirical spec of the `.dna` container. Validated on a 41-file corpus (138 bp → 4.64 Mb). |
| [`reference/python/snapdna.py`](reference/python/snapdna.py) | Reader + writer, stdlib only. **Byte-exact round-trip on 41/41 files.** |
| [`reference/python/ab1_probe.py`](reference/python/ab1_probe.py) | ABIF (`.ab1`) chromatogram reader. Parses 374/394 real traces. |
| [`docs/PLAN.md`](docs/PLAN.md) | The architecture and roadmap this repo is built from. |
| Application | **Not started.** |

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
  sequences and all 50 enzymes — the property `docs/PLAN.md` calls "where origin
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

## Licence

Apache-2.0 for code. The features database will be CC BY 4.0 and the benchmark
CC0, both in separate repositories with separate licences. Restriction-enzyme
data is REBASE, redistributed under its own terms with its own `NOTICE`.

## Trademarks

SnapGene is a trademark of GSL Biotech LLC. Benchling, Geneious, Gibson
Assembly, NEBuilder, In-Fusion, Gateway and TOPO are trademarks of their
respective owners. This project is not affiliated with, endorsed by, or
sponsored by GSL Biotech, Dotmatics, Siemens, or any other company named here.
References to these marks are nominative and descriptive only.

See [`TRADEMARKS.md`](TRADEMARKS.md) and [`PROVENANCE.md`](PROVENANCE.md).
