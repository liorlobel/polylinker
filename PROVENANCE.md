# Provenance of file-format knowledge

This file is the independent-derivation record for everything Polylinker knows
about proprietary file formats. It exists so that, if the question is ever
asked, the answer is documented rather than reconstructed from memory.

**Rule: every piece of format knowledge gets a row here, with its source, that
source's licence, the date, and who used it. Add the row in the same commit as
the code.**

---

## 1. The SnapGene `.dna` container

### How it was derived

By reading the bytes of `.dna` files with a hex dump and a Python script, and
observing structure. Specifically:

- A corpus of **41 `.dna` files belonging to the repository owner**, generated
  by their own laboratory work (plasmids, assembly contigs, and two bacterial
  genomes). These are the owner's own research data, not vendor-supplied
  material.
- Structure was inferred from the framing itself: a 1-byte type, a 4-byte
  big-endian length, and a payload. Several payloads are plain UTF-8 XML and are
  simply readable. One is xz-compressed XML, identified by its magic bytes.

Findings are written up in [`docs/DNA-FORMAT.md`](docs/DNA-FORMAT.md).

### What was NOT done

- No decompiler, disassembler, debugger, Ghidra, IDA or Hopper was run against
  any SnapGene binary.
- `strings` was not run against any SnapGene binary.
- No SnapGene application resource, database or data file was read, extracted,
  copied or reproduced. In particular `standardCommonFeatures.ftrs` — the
  proprietary common-features database — was **never opened**. Its existence
  and file size are noted only as context for scale.
- Dotmatics/GSL Biotech was not contacted for a specification, and must not be.
  Any specification they provide would arrive under terms that contractually
  poison every downstream contributor. **Asking is strictly worse than not
  asking.**

### Corroborating public sources

The format is independently described by at least four existing open
implementations. Nothing in `docs/DNA-FORMAT.md` depends on private knowledge.

| Source | Licence | Used for |
|---|---|---|
| Biopython `Bio.SeqIO` `"snapgene"` reader | Biopython License (BSD-3-like) | Cross-checking block semantics |
| `autosnapgene` | MIT | The unparsed-block passthrough pattern |
| `@teselagen/bio-parsers` | MIT | Feature/primer XML shape |
| PlasCAD | MIT | Block-type enumeration |

### Known disclosure in this repository's history

**The repository owner is a SnapGene licensee and has SnapGene 8.2.2 installed
on the machine where the initial format analysis was performed.** This is
recorded here deliberately rather than omitted.

What this does and does not mean:

- The format findings derive from the owner's **own data files**, which is the
  cleanest possible provenance — those files are the owner's property and
  reading them is not reverse engineering of the vendor's software.
- The SnapGene installation directory was listed, and its bundled
  open-source-licence folder (`COPYING/`) was read. Both are ordinary file
  listings, not reverse engineering, and the `COPYING` directory exists
  precisely to be read. No proprietary data file was opened.
- However, SnapGene's EULA §4 prohibits reverse engineering **with no
  "except as permitted by applicable law" carve-out**, and under US precedent
  (*Bowers v. Baystate*, *Davidson v. Jung*) a clicked EULA can validly waive
  statutory interoperability exceptions. The exposure here is **contract, not
  copyright** — and it only binds someone who accepted the EULA.

**Consequences adopted by this project:**

1. Contributors who work on `.dna` format code **should not be SnapGene
   licensees**. See `CONTRIBUTING.md`.
2. The format specification in this repository should be **re-derived and
   signed off by a contributor who has never accepted the SnapGene EULA**.
   The corpus needed for this is freely available: Addgene distributes `.dna`
   files publicly.
3. Legal advice should be obtained, together with Bar-Ilan University
   technology-transfer clearance for Apache-2.0 release. Institutional
   IP-assignment policy derails more academic open-source releases than vendors
   do.

### Status of 2 and 3, as of 2026-08-05

**Neither was done, and the repository was made public anyway.** Both items
carried the words "before the first public release" and "before the first
public commit" until that date. Those words are gone from the two paragraphs
above because the deadline they named has passed — **not** because the work
behind them has been done. It has not.

The decision was the repository owner's, and it is recorded here rather than in
a commit message so that it travels with the file it qualifies: the project is
a laboratory tool for the owner's own use, and on that basis these two gates
were set aside rather than met. Note that publishing does not narrow who can
read the format code, whoever ends up running the binaries.

Nothing above this section is affected. "How it was derived" and "What was NOT
done" are statements of fact about how the work was actually carried out — the
corpus was the owner's own `.dna` files; no decompiler, disassembler, debugger
or `strings` was run against any SnapGene binary; no vendor resource or database
was opened; Dotmatics was not contacted — and they remain true.

Item 2 is still the single highest-value contribution an outside collaborator
could make, for the reason `CONTRIBUTING.md` gives.

---

## 2. ABIF (`.ab1`) Sanger chromatograms

Derived from the **publicly published Applied Biosystems ABIF specification**
(July 2006, 54 pp), which AB released for exactly this purpose. No reverse
engineering involved.

Empirical notes added from a 394-file corpus of the owner's own traces:

- 20 of 394 files (5%) carrying an `.ab1` extension are **not ABIF at all** —
  they are SCF (`.scf` magic) or ZTR (`\xaeZTR\r\n\x1a\n` magic). Sniff magic
  bytes; never trust the extension.
- The `PBAS1` and `PBAS2` basecall arrays **differ in 93 of 117 files checked**.
  They are the edited and original basecalls respectively. Choosing the wrong
  one silently changes the reported sequence.

---

## 3. Restriction enzyme data

Recognition sites used in the prototype were transcribed independently from
standard published references. They are facts about enzymes, not a copied
database.

**Verified 2026-07-27** against Biopython's `Bio.Restriction`, which is
REBASE-derived and BSD-licensed: all 51 sites and top-strand cut offsets agreed
exactly. The signed `ovhg` column and the eight Type IIS entries (BsaI, BsmBI,
Esp3I, BbsI, SapI, BspQI, PaqCI, AarI) were taken from the same place, in the
same spirit — a cut geometry is a measurement, and Biopython's implementation
carries a licence while the numbers do not. `docs/PLAN.md` §7.2 makes the
equivalent call for thermodynamic parameters and names the source to avoid
(Primer3's GPL-2.0 `oligotm.c`).

**The shipped table is 58 entries today: 50 Type IIP and those 8 Type IIS.** The
51 above is the count on the date given and is left as written, because it is
what was checked that day. `BstXI` came out afterwards — its overhang is `NNNN`,
a property of the DNA rather than of the enzyme, which is why `Compatibility` in
`crates/pl-enzymes` still uses it as the example of an end that can only be
answered "the sequence decides". `crates/pl-enzymes/src/lib.rs` and
`reference/python/tests/validate_digest.py` both hold 58, independently, and
`pl methods digest` prints `ENZYMES.len()` rather than a number typed into prose.

This does not change the position below: production enzyme coverage means
REBASE, under REBASE's own terms, in its own package.

Production enzyme data will come from **REBASE** (Roberts lab / New England
Biolabs) under REBASE's own terms, packaged separately with its own `NOTICE`,
and not commingled with Apache-2.0 code.

---

## 3a. Primer design (`pl-design`)

Thermodynamic parameters come through `pl-thermo`, which took them from
Biopython's `Bio.SeqUtils.MeltingTemp` — recorded in that crate's module doc,
under *Where the numbers came from* (`crates/pl-thermo/src/lib.rs`). The pointer
used to say "§5 below" when this file stopped at §4, and the record it meant has
never lived in a numbered section here at all. **A §5 now exists** — the archive,
which §4 pushed down on 2026-08-04 — and it is not that record either, so a stale
"§5 below" now lands somewhere real and wrong rather than nowhere. The sections
here are §1, §2, §3, §3a, §4 and §5. The record itself was never missing, only
the route to it. Nothing new was imported for the designer.

**Not used, and nobody opened them:** `oligotm.c`, `thal.c`, `libprimer3.c` or
any other file of the Primer3 source tree, including the C sources vendored
inside the installed `primer3-py` wheel. Primer3 is GPL-2.0. Its default
`PRIMER_WT_*` weights were deliberately not copied either, even from the manual:
they are an undefended set of conventions wearing an authoritative-looking
provenance, and importing them would launder one into the other. The weights in
`pl_design::Weights` are ours and the tool prints them.

`seqfold` (Lattice Automation, MIT), which `docs/PLAN.md` §7.4 nominates for a
Zuker fold, has **not** been read or ported. `pl_design::fold` is a perfect-helix
screen written from the stacking parameters already in `pl-thermo`, and its
module doc says so rather than implying a fold. Every free energy it produces is
printed with a `>=`, and `fold::SCREEN_NOTE` travels with it into every report,
so the limit is stated where the number is shown and not only in
`pl methods design`.

### Two corrections a reviewer had to make, recorded rather than quietly fixed

**The 3'-terminal stability criterion is Rychlik (1993), Methods Mol Biol
15:31-40**, computed on Breslauer, Frank, Blöcker & Marky (1986) PNAS 83:3746
parameters. `pl_design::params` and the crate's provenance table cited Rychlik,
Spencer & Rhoads (1990) NAR 18:6409 instead, which is the *annealing-temperature*
paper (`Ta = 0.3·Tm_primer + 0.7·Tm_product − 25`) and says nothing about
3'-terminal stability. `docs/research/dossier.md` had it right; the error was
introduced on the way into the crate, and it shipped in a public crate doc and
in `pl methods design`. A provenance table exists to make exactly this
impossible, so it is worth recording that it did not.

**The length defaults 18 / 20 / 27 are the field-wide convention and were not
taken from Primer3.** They coincide with Primer3's documented
`PRIMER_MIN_SIZE` / `PRIMER_OPT_SIZE` / `PRIMER_MAX_SIZE`, which a reviewer
rightly flagged as worth a sentence given how carefully the `PRIMER_WT_*`
weights are disclaimed above. 18 and 20 are universal; 27 is the ordinary
synthesis-cost ceiling, and `Constraints::LEN_MAX`'s own doc argues it from
coupling efficiency rather than from anyone's defaults. Numeric parameter
defaults are facts rather than copyrightable expression in any case, but the
asymmetry — weights disclaimed at length, lengths silent — is the kind of gap
that reads as an omission.

### Per-enzyme flanking-base requirements — **NO-GO, probed 2026-07-29**

Most restriction enzymes cut poorly within a few bases of a fragment terminus,
and per-enzyme requirements have been measured and tabulated by enzyme
manufacturers. **That table is not reproduced here and `pl-design` ships no
default spacer.**

Probe record, in the form `features/SOURCING.md` requires:

| URL | Result |
|---|---|
| `neb.com/.../cleavage-close-to-the-end-of-dna-fragments` | HTTP 403, no body retrieved |
| `neb.com/en/policies/terms-of-use` | HTTP 403, no body retrieved |

The operative licence sentence could not be retrieved from the source itself. A
search-engine summary is the "third-party summary" class of evidence
`SOURCING.md` refuses, and was not treated as establishing anything. **An
unretrievable licence is a hold, never a permission** — `SOURCING.md`'s own rule,
and Risk 9's point that a legal position citing a URL which serves an empty shell
fails at exactly the moment it is tested.

Why this differs from §3 above, since a reviewer will otherwise read it as
inconsistency: the enzyme sites and `methylation.rs` each have an **independent
open leg** (standard published references cross-checked against Biopython's
BSD-3 `Bio.Restriction`; REBASE `damlist`). The flanking table has none. It is a
dataset one manufacturer generated by its own experiments on synthetic
substrates, and re-typing somebody's measurements does not make them an
independent derivation.

What is done instead: cite the phenomenon (Kaufman & Evans 1990 BioTechniques
9:304; Moreira & Noren 1995 BioTechniques 19:56 (no accent -- that is how PubMed
indexes the author, and the two spellings disagreed across this tree until a
reviewer noticed) — both verified to exist via
PubMed 2026-07-29, **neither read**, and neither cited as the source of any
number), warn whenever a site is added with no spacer, and let the user supply
their own bases with `--spacer`. Route to a table, if one is ever wanted: a
written grant from the manufacturer recorded in `legal/`, or an independent
measurement, which is a bench project and not a software one.

---

## 4. Open published specifications — DEFLATE, zlib, PNG, TrueType

The PNG export back end — `crates/pl-draw/src/deflate.rs`, `png.rs` and
`font.rs` — is three file formats' worth of format knowledge (DEFLATE with its
zlib wrapper, PNG, TrueType) taken from four published specifications, and
**none of it arrived by anything resembling the route §1 describes.** Every one
of those four is a document its own authors published in order that it be
implemented, retrievable today by anybody, on terms printed inside the document
itself.

This section exists because the rule at the top of this file says every piece of
format knowledge gets a row. It is **not** here because the knowledge is
encumbered, and the difference is the reason the section is separate rather than
another table under §1:

- §1 is one vendor's undocumented container, inferred from the repository
  owner's own files, carrying a live contract question, a disclosure, and a rule
  in `CONTRIBUTING.md` about who may touch the code.
- §4 is four published standards, read as published. There is no vendor, no
  EULA, no clean-room question, nothing to re-derive before release, and **no
  restriction whatever on who may work on this code.** A SnapGene licensee may
  write PNG code here; that is not a concession, it is what an open
  specification means.

A reader who found the two sharing one table would be entitled to infer that
PNG and `.dna` came to us the same way. They did not.

### What was implemented, and from which document

| Implemented | In | Specification | The document's own terms |
|---|---|---|---|
| DEFLATE: LZ77, canonical Huffman with depth limiting, the length/distance base and extra-bit tables, the code-length alphabet and its permutation | `deflate.rs` | **RFC 1951**, *DEFLATE Compressed Data Format Specification version 1.3*, P. Deutsch, May 1996, Informational | "Permission is granted to copy and distribute this document for any purpose and without charge" — © 1996 L. Peter Deutsch |
| The zlib wrapper (`CMF`/`FLG` and its %31 check) and Adler-32 | `deflate.rs` | **RFC 1950**, *ZLIB Compressed Data Format Specification version 3.3*, P. Deutsch and J-L. Gailly, May 1996, Informational | the same sentence, verbatim — © 1996 Deutsch and Gailly |
| The PNG container: signature, chunk framing and CRC, `IHDR`, `sRGB`/`gAMA`/`cHRM`, `pHYs`, filter type 0 | `png.rs` | **The PNG specification** — three overlapping publications, all open; see the retrieval table | W3C Software and Document Notice and License for the W3C printings; PNG 1.2 is published freely by the PNG Development Group |
| `head`, `maxp`, `hhea`, `loca`, `glyf`, `cmap` format 4, `hmtx`, composite glyphs, and the implied on-curve point | `font.rs` | **The OpenType specification** 1.9.1 (Microsoft) and **Apple's TrueType Reference Manual** | both published on the vendors' own developer sites for implementers; neither sits behind an agreement of any kind |

CRC-32 is not a fourth format. The PNG specification defines the chunk CRC
itself, gives the polynomial in full and attributes it to ISO 3309 and
ITU-T V.42; `deflate::crc32` is that polynomial in its reflected (`0xEDB88320`)
form, which is the form PNG specifies.

**The two Liberation Sans binaries are not covered by this section.** A font
file is a work, not a fact about a format. Their provenance — the upstream
release archive, the sha256 of every committed file, the OFL, the Reserved Font
Name and why nothing is subsetted — is in `NOTICE`, and
`the_vendored_faces_are_the_files_notice_records` joins that record to the bytes
so it cannot go quietly stale.

### The constants, and where each one comes from

Every value in this table was read back out of what the code actually produces
(see the last subsection), not transcribed from the source:

| What `pl-draw` writes | Value | Where it comes from |
|---|---|---|
| `IHDR` depth and colour type | colour type 2, bit depth 8 | truecolour RGB; 8 and 16 are the depths PNG allows for type 2 |
| `gAMA` | gAMA 45455 | the gamma PNG prints for an sRGB image |
| `cHRM` | 31270, 32900, 64000, 33000, 30000, 60000, 15000, 6000 | white point, then red, green, blue — x before y, ×100000 — the sRGB primaries in the order PNG prints them |
| `sRGB` | rendering intent 0 | perceptual, the first of the four PNG defines |
| `pHYs` | unit specifier 1 | the metre. PNG has no notion of inches, which is why `per_metre` exists |
| the zlib header | 0x78 0x9C | RFC 1950 §2.2 — deflate, 32 KB window, no preset dictionary, and 0x789C = 31 × 996 so the mandatory %31 check passes |
| the face's design grid | 2048 units per em | `head` + 18 of the committed Liberation Sans Regular, read by `Face::parse` |

### Retrieved, and what each retrieval settled — 2026-08-04

In the form `features/SOURCING.md` requires. Every URL below returned a body —
none of §3a's 403s — and the two rows marked *partial* returned the document but
not the passage wanted.

| URL | What it settled |
|---|---|
| `rfc-editor.org/rfc/rfc1951.txt` | Title, author, May 1996, Informational; the copying permission quoted above; and that §3.1.1, §3.2.2, §3.2.5 and §3.2.7 — the four sections `deflate.rs` cites in its constants — are the sections it thinks they are |
| `rfc-editor.org/rfc/rfc1950.txt` | Title, both authors, May 1996; the identical permission sentence; §2.2 the data format, §9 the Adler-32 appendix |
| `libpng.org/pub/png/spec/1.2/PNG-Chunks.html` | Every value in the constants table above: the four rendering intents, the gamma, all eight chromaticities in order, the `pHYs` unit codes, and colour type 2's allowed depths |
| `w3.org/TR/2003/REC-PNG-20031110/` | That the W3C Recommendation of 10 November 2003 is ISO/IEC 15948:2003(E), and the chunk-CRC polynomial with its ISO 3309 / ITU-T V.42 attribution. *Partial:* the sRGB chunk section did not come back in the retrieved text, which is why the row above cites PNG 1.2 for the values |
| `w3.org/TR/png-3/` | That the current publication is the Third Edition, a W3C Recommendation of 24 June 2025 under the W3C permissive document licence, and that it states it is intended to become an International Standard but is not yet one. *Partial:* the chunk values were not in the retrieved text |
| `learn.microsoft.com/en-us/typography/opentype/spec/{otff,head,hhea,maxp}` and `developer.apple.com/fonts/TrueType-Reference-Manual/RM06/Chap6loca.html` | Every byte offset in `Face::parse`, re-derived from the published field orders: `numTables` at 4, table records at 12 in 16-byte units with offset at +8 and length at +12; `head` `unitsPerEm` at 18 and `indexToLocFormat` at 50; `maxp` `numGlyphs` at 4; `hhea` `numberOfHMetrics` at 34. All nine agree. Apple's `loca` chapter supplies the short-form rule in as many words — "The actual local offset divided by 2 is stored" — which is the halving `Face::parse` undoes |

**Which printing of the PNG specification.** Stated rather than smoothed,
because the row would otherwise imply a precision it does not have: what is
recorded here is a check made on 2026-08-04 against the documents above, not a
claim about which of the three printings was open while the code was written. It
does not matter for provenance — all three are open, and a constant is the same
constant in each — but it is the kind of detail this file exists to get right.

### What was NOT done

- **No implementation was read or ported.** Not zlib's `deflate.c` or
  `inflate.c`, not libpng, not `stb_truetype`, not FreeType, not fontTools'
  Python. Nothing third-party is vendored (`NOTICE`) and nothing under
  `crates/` takes a dependency at all, so there is no route by which any of them
  could have arrived. The base and extra-bit tables in `deflate.rs` are the
  specification's own tables: statements about a bit format, of the same kind as
  §3's enzyme sites.
- **The cross-checks in `reference/python/tests/` are run, never read.** They
  are executed against artifacts and their verdicts believed; no line of any of
  them is transcribed into Rust, and none is linked into anything shipped. The
  single place another implementation's *behaviour* is deliberately relied on is
  `xcheck_glyphs.py` subclassing fontTools' `BasePen` so that the
  implied-on-curve expansion is fontTools' and not a second statement of our own
  reading — and that is a call at test time, not a transcription.
- **Nobody was asked for a specification, because nobody had to be.** That is
  the whole distance between this section and §1's third bullet.

### The oracles, and what is known about their licences

Test-time only: executed, not linked, not distributed, not read. `NOTICE`'s
*Test-time only* list records Biopython and jsdom in the same class.

| Oracle | Judges | Licence, checked 2026-08-04 |
|---|---|---|
| CPython's `zlib` module | Every zlib stream this crate writes, one-shot and byte-at-a-time | It binds the reference implementation, zlib (© 1995-2026 Jean-loup Gailly and Mark Adler), under the zlib License — read from `zlib.net/zlib_license.html`. CPython's own licence was not separately retrieved, and nothing here turns on it |
| Pillow (PIL) | That the PNGs open, and that every pixel and the dpi survive | MIT-CMU, from the package's own metadata |
| fontTools | Every glyph outline, contour by contour | MIT, from the package's own metadata |
| resvg, via `resvg_py` | The whole raster against an independent SVG renderer | Upstream resvg is Apache-2.0 or MIT at the user's option. **Which resvg `resvg_py` 0.3.4 vendors was not established** — its own metadata states no licence. Recorded as open rather than resolved, and it does not bear on provenance here because the tool is run and never read or linked |

Nothing above is a source of format knowledge. They are second opinions, and
the reason the file is written the way it is: the specification tells us what
the bytes should be, and an implementation nobody here wrote tells us whether
we read it correctly.

### This row is checked, not merely written

`pl_draw::tests::the_provenance_rows_record_the_constants_the_code_actually_writes`
reads this file with `include_str!`, encodes a PNG, walks its chunks, compresses
a real zlib stream and parses the committed face — then requires this section to
contain every value it measured. Each needle is built from that measurement
rather than typed into the test, so a constant that changes in the code and not
here fails, and so does a number changed here and not there. It is scoped to the
text between this heading and the next, so a digit that happens to appear under
§1 cannot satisfy a claim about PNG.

What it cannot do is judge whether the specification agrees. Nothing in this
repository can; that is what the four oracles above are for, and why the
retrieval table carries dates.

---

## 5. Archive

`legal/archive/` should contain dated captures, made while the pages are live:

- [ ] SnapGene EULA / Terms of Service
- [ ] SnapGene's "convert file formats" page listing the competitor formats it
      itself imports — a self-refuting position for any complaint that reading
      `.dna` is illegitimate
- [ ] USPTO TSDR records for SNAPGENE
- [ ] EUIPO / WIPO records for SNAPGENE
- [ ] `rebase.neb.com/rebase/rebhelp.html` and `rebcit.html`
- [ ] The Autodesk / Open Design Alliance settlement statement
