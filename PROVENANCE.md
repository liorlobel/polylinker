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
   signed off by a contributor who has never accepted the SnapGene EULA**
   before the first public release. The corpus needed for this is freely
   available: Addgene distributes `.dna` files publicly.
3. Legal advice should be obtained before the first public commit, together
   with Bar-Ilan University technology-transfer clearance for Apache-2.0
   release. Institutional IP-assignment policy derails more academic open-source
   releases than vendors do.

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

This does not change the position below: production enzyme coverage means
REBASE, under REBASE's own terms, in its own package.

Production enzyme data will come from **REBASE** (Roberts lab / New England
Biolabs) under REBASE's own terms, packaged separately with its own `NOTICE`,
and not commingled with Apache-2.0 code.

---

## 4. Archive

`legal/archive/` should contain dated captures, made while the pages are live:

- [ ] SnapGene EULA / Terms of Service
- [ ] SnapGene's "convert file formats" page listing the competitor formats it
      itself imports — a self-refuting position for any complaint that reading
      `.dna` is illegitimate
- [ ] USPTO TSDR records for SNAPGENE
- [ ] EUIPO / WIPO records for SNAPGENE
- [ ] `rebase.neb.com/rebase/rebhelp.html` and `rebcit.html`
- [ ] The Autodesk / Open Design Alliance settlement statement
