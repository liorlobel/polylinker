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

## 3a. Primer design (`pl-design`)

Thermodynamic parameters come through `pl-thermo`, which took them from
Biopython's `Bio.SeqUtils.MeltingTemp` — recorded in that crate's module doc and
in §5 below. Nothing new was imported for the designer.

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
