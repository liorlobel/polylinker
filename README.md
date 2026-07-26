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
| [`crates/`](crates) + [`bins/pl`](bins/pl) | **The Rust core and CLI.** `pl-core` (model), `pl-enzymes` (digestion), `pl-fileio` (`.dna`, GenBank, FASTA), and the `pl` command. **Zero external dependencies**, 501 KB static binary, 66 tests. |
| [`prototype/dna-reader.html`](prototype/dna-reader.html) | **Usable today.** Opens `.dna`, GenBank and FASTA; draws circular/linear maps; live restriction digest; exports GenBank, FASTA and vector SVG. One file, no install, no network — runs from a USB stick on a locked-down PC. |
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
target/release/pl convert "plasmids/**/*.dna" --to genbank -o converted/
target/release/pl info    plasmid.dna
target/release/pl digest  plasmid.dna --unique
target/release/pl blocks  plasmid.dna      # what the container is made of
```

No toolchain? Open [`prototype/dna-reader.html`](prototype/dna-reader.html) in a
browser, drop a file on it, and press **Save GenBank** — no install, no admin
rights.

GenBank is plain text and is read by ApE, UGENE, Benchling, Biopython and
SnapGene itself, so converting costs you nothing and un-strands your data.

### Building

`pl-core` has no dependencies, so `cargo build` needs no network. A linker is
the only external requirement:

- **Linux/macOS** — works out of the box.
- **Windows** — needs the MSVC linker (Visual Studio Build Tools, "Desktop
  development with C++") or a MinGW-w64 toolchain. Rust alone is not enough;
  `rustup` does not ship a linker. Building under WSL works today.

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
- On the 4.64 Mb *E. coli* genome: **70 ms** to parse and report, **590 ms** for
  a 50-enzyme digest, 30 MB peak RSS.

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
