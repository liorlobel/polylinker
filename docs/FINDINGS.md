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

---

## The library corpus, measured (2026-07-27)

Measured on the development machine (Core Ultra 9 275HX, 24 logical cores,
127 GiB RAM, NTFS) against a real lab drive of 344 plasmid files, the
folder that motivates the library index. Numbers, not estimates; the commands
are in the session record.

**The corpus is bimodal, and that is the design constraint.** 68,813 files
total, 472 GiB. Of those, 2,042 are sequence files totalling 10.72 GiB — but
the plasmid formats (`.dna .gb .gbk .ape .seq`) are 1,075 files with a median
of **3,184 bytes**, while `.fa/.fasta/.fna` contribute 9.7 of the 10.7 GiB as a
handful of NGS and SILVA reference files up to **1.39 GB each**. Treating
"sequence file" as one population is wrong: the single largest file would cost
roughly six times the parse time of the entire rest of the library. **Size-gate,
and say what was gated.**

**Enumeration is cheap; the API choice is not.** Metadata-only walk of all
68,813 files: 2.1 s warm, 19.8 s cold. The same walk with
`Get-ChildItem -Include *.dna,*.gb,…` takes **15.2 s warm — 7.2× the full
walk**. Enumerate once with `-File` and filter extensions in memory.

**Reading is not the thing to be afraid of.** Per file: metadata 0.076 ms,
open + 4 KB 0.169 ms, open + read every byte 1.83 ms (2,311 MB/s; a 1.29 GB
FASTA reads at 5,572 MB/s).

**Correction to an earlier hypothesis.** A walk of this folder was observed to
run over ten minutes, and cloud placeholders were assumed to be the cause. They
are not. `FILE_ATTRIBUTE_OFFLINE`, `RECALL_ON_OPEN` and `RECALL_ON_DATA_ACCESS`
are **zero across all 68,813 files**; 68,654 carry `FILE_ATTRIBUTE_PINNED`
(0x80000), i.e. every file is materialised locally and no hydration is possible.
The likeliest cause is the `-Include` cost above against a cold metadata cache,
plus OneDrive syncing concurrently — but **this remains unreproduced**, and the
original cold state cannot be recovered without a reboot. Recorded as
unexplained rather than closed.

**A landmine for any directory walker.** 68,811 of 68,813 files *are* reparse
points, tag `0x9000601A` = `IO_REPARSE_TAG_CLOUD_6`, with empty `LinkType` and
`Target`. The conventional defence against symlink cycles — skip reparse points
— would therefore skip **the entire corpus**. Follow the tag, not the flag.

**Parse in-process.** Process startup is 7.94 ms; parsing a mean corpus file is
0.533 ms and a small plasmid 0.052 ms. A subprocess-per-file indexer would be
~94% startup. 3,000 files single-threaded: **1.6 s** in-process against 25.4 s
out-of-process.

**Parallelism is not needed for v0.1.** 1.6 s cold, single-threaded. 24 cores
would take it to ~0.1 s: optimising a non-problem at the price of thread-safety.

**Memory is a non-issue.** The real corpus is 23.2 Mbase → **11.1 MiB** packed
at 4 bits per base. The 3,000 × 5 kb design target is 7.2 MiB, 0.0055% of RAM.
Hold it all resident; never page or mmap.

**Search is fast enough, with thin headroom.** A linear nibble scan runs at a
stable **~335 Mbase/s** single-threaded, and degeneracy costs ~3% because early
exit dominates. The design target scans in **45 ms** and the real corpus in
**69 ms**, both inside the 100 ms "feels instant" budget — but that budget is
**exhausted at roughly 33 Mbase**. Above that, parallelism or a real index
becomes necessary. This is the number to re-measure before claiming the design
scales.

Loose ends worth their own look: 30 of 913 corpus files parse without yielding
a length; four sequence files are zero bytes; `pl info` on a multi-record FASTA
reports `records 9712 in this file` and shows the first.
