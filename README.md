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
| [`docs/DNA-FORMAT.md`](docs/DNA-FORMAT.md) | Empirical spec of the `.dna` container. Validated on a 41-file corpus (138 bp → 4.64 Mb). |
| [`reference/python/snapdna.py`](reference/python/snapdna.py) | Reader + writer, stdlib only. **Byte-exact round-trip on 41/41 files.** |
| [`prototype/dna-reader.html`](prototype/dna-reader.html) | Working in-browser reader *and* writer. Circular/linear map, live restriction digest, file-anatomy view. No dependencies, no build step, no network. |
| [`reference/python/ab1_probe.py`](reference/python/ab1_probe.py) | ABIF (`.ab1`) chromatogram reader. Parses 374/394 real traces. |
| [`docs/PLAN.md`](docs/PLAN.md) | The architecture and roadmap this repo is built from. |
| Application | **Not started.** |

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

## Try the prototype

Open `prototype/dna-reader.html` in any browser and drop a `.dna` file on it.
Nothing is uploaded; there is no server. It also writes a `.dna` file from
scratch, which is the open experiment described in
[`docs/DNA-FORMAT.md` §4](docs/DNA-FORMAT.md).

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
