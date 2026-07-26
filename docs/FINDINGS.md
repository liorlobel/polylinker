# Empirical findings, 2026-07-26

Results from the first investigation session. Everything here was measured, not
assumed. Where a hypothesis was tested and failed, that is recorded too.

Corpus: 41 `.dna` files, 394 `.ab1` files, 303 GenBank files — all belonging to
the repository owner. Sizes from 138 bp to 4.64 Mb.

---

## 1. The `.dna` container is solved for reading

| Metric | Result |
|---|---|
| Files parsed | **41 / 41** |
| Bytes accounted for by block framing | **100%** of every file |
| Byte-exact round-trip (read → write) | **41 / 41** |
| Feature segments out of bounds | 0 |
| Non-IUPAC characters in any sequence | 0 |

Full specification in [`DNA-FORMAT.md`](DNA-FORMAT.md).

## 2. Blocks 2 and 3 are regenerable caches — 78% of a typical file

The two largest blocks in every file are not user data.

- **Block 3** contains a comma-separated ASCII list of IUPAC restriction
  recognition sites — literally `GACNNNNNNGTC,CCAGGCCTGG,GATCCGGATC,...`.
  100% of tokens are pure IUPAC. Counts of 469, 473 and 485 sites were observed
  in different files, so this is a snapshot of the enzyme set active at save
  time, not a fixed table.
- **Block 2** scales at ~2.13 bytes per base above a fixed ~1.8 kB floor, is
  high-entropy and near-incompressible — packed integers, consistent with a
  cut-position index keyed to block 3.

Dropping both loses no user-authored information and shrinks files by a **mean
of 78%, maximum 96%**.

### A hypothesis that was tested and rejected

Block 2 as `version byte, then per-site [count][positions...]` records
interleaved in block-3 order: **rejected**. It desynchronises partway through
every one of 14 files tested. The true layout is probably a counts table
separate from a positions table, or a variable-width encoding — but this was
deliberately not pursued further, because a cache we intend to omit is not worth
decoding.

### The one open experiment

Every corpus file was written by SnapGene and every one contains blocks 2 and 3,
so the corpus cannot say whether they are *required* on read.
`prototype/dna-reader.html` generates a valid `.dna` without them. **Open it in
SnapGene.** If it opens, `.dna` write support is essentially complete.

## 3. Topology flag confirmed — bit 0, not bit 1

Published prose has claimed the topology flag is the second bit; the open
implementations all mask `0x01`. Measured across the corpus:

| Flags | Count | Meaning |
|---|---|---|
| `0x1f` | 19 | circular, double-stranded, Dam + Dcm + EcoKI |
| `0x1e` | 19 | linear, double-stranded, Dam + Dcm + EcoKI |
| `0x02` | 3 | linear, double-stranded, no methylation (older writer) |

Circular files carry `0x01`. **The code was right, the prose was wrong.** This
closes an item the plan flagged for week-1 verification.

## 4. The history tree is xz-compressed

Block 7 begins `FD 37 7A 58 5A 00` and inflates ~7× to UTF-8 XML. A parser
treating it as text sees binary garbage. Independently corroborated: SnapGene's
own bundled-licence directory lists **xz**.

Contents are a recursive provenance graph — each parent construct embedded whole
with its own sequence and features, annotated with the operation and enzymes
that joined them, including `<RegeneratedSite>` entries at each junction. Only
3 of 41 files carried one, so it is written only for constructs actually built
inside the application.

## 5. Restriction digest validated against Biopython

| Metric | Result |
|---|---|
| Real plasmids compared | 33 |
| Cut sites cross-checked | **5,587** |
| Enzymes disagreeing | **0** |

Includes circular wraparound, which is where naive implementations fail. This is
the QA pattern the whole project should follow: no biology routine ships without
a differential test against an established implementation.

## 6. Performance — compute is not the bottleneck

| Input | Parse | 50-enzyme digest |
|---|---|---|
| 15.6 kb plasmid (79 KB file) | 1 ms | 1 ms |
| 2.30 Mb genome (8.1 MB file) | 6 ms | 139 ms |
| 4.64 Mb genome (17.6 MB file) | **13 ms** | **287 ms** |

Pure Python, single-threaded. Extrapolating to the full ~470-enzyme set gives
~2.7 s interpreted, and well under 100 ms in Rust or WASM with Aho-Corasick.

**Implication:** do not reach for Rust/WASM early for *performance* reasons.
Reach for it for the four-distribution-surfaces argument. The engineering risk
lives in the view layer — rendering 4.6 million bases, and automatic label
placement on a crowded map — not in the algorithms.

## 7. ABIF: 5% of `.ab1` files are not ABIF

Of 394 files carrying an `.ab1` extension:

- **374 parsed** as ABIF (v101), median read length 998 bp, max 1,443 bp.
- **20 were something else entirely** — SCF (`.scf` magic) and ZTR
  (`\xaeZTR\r\n\x1a\n` magic), simply misnamed.

**Sniff magic bytes; never trust the extension.** SnapGene handles these via the
bundled Staden `io_lib`, which reads all three formats.

### PBAS1 and PBAS2 disagree in 80% of files

The two basecall arrays — edited and original — **differed in 93 of 117 files
checked**, typically by 6–14 bases. Biopython reads `PBAS2`. Choosing the wrong
one silently changes the reported sequence, with no error. Always read tag
number 2, and read `FWO_` to map channels to bases rather than assuming ACGT.

## 8. SnapGene's own open-source bill of materials

Its bundled licence directory names what it is built from:

**Alignment and assembly:** parasail (SIMD Smith-Waterman), MAFFT, MUSCLE,
Clustal Omega, T-Coffee, CAP3, TM-align
**Traces and sequencing:** io_lib (Staden — ABI/SCF/ZTR), samtools/htslib
**Science:** ViennaRNA
**Platform:** Qt 6, OpenSSL, xz, zlib, MiniZip, Crashpad, Sentry, EmfEngine

Two useful conclusions:

1. **The science layer of a clone can use the same components.** These are the
   field-standard tools, and SnapGene did not write its own aligner either.
2. **The GPL components live in `Tools/` as separate executables**, invoked as
   subprocesses rather than linked. That is the standard technique for using
   copyleft tools from a permissively-licensed application, demonstrated by the
   incumbent. Note that ViennaRNA's licence forbids redistribution for a fee —
   prefer `seqfold` (MIT) — and CAP3 is academic-use only.
