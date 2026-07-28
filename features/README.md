# polylinker-features

An openly licensed, provenance-tracked database of common plasmid features.

**Licence: CC BY 4.0** for the data. See [`NOTICE`](NOTICE) for the attributions
this carries, and [`SOURCING.md`](SOURCING.md) for how each source was cleared
and by what evidence.

> **Status: v0.1 pre-release, 70 records. Every row is `proposed` —
> machine-assembled, with no human sign-off. Nothing here is shippable yet.**
> `Db::reviewed()` returns an empty database until a curator puts their name
> against each row. That is the intended state, not an unfinished one; see
> *Rule 6* below.
>
> **This is a dated snapshot** (sources retrieved 2026-07-27 and 2026-07-28) and
> does not reflect the most current data available from NLM, UniProt, EMBL-EBI or
> Rfam. Per-field retrieval dates and source hashes are in `provenance.tsv`.

## Why this exists

Every open plasmid annotator in this field ultimately depends on one CSV that
was scraped from SnapGene's proprietary Common Features list in 2021 and
redistributed with no licence at all. Its Description column is SnapGene's
curated prose verbatim. It is simultaneously the most-used dataset in the
ecosystem and its clearest legal exposure, and it has not been meaningfully
updated since.

`docs/PLAN.md` §8.3 calls replacing it *"the highest-leverage contribution
available to anyone in this field."*

## What makes a row defensible

Two things this schema does that a single-table feature list cannot.

**Provenance is per *field*, not per row.** One record legitimately mixes
licences: the name and family may come from UniProt (CC BY 4.0), the nucleotides
from an INSDC record (free-and-unrestricted, with a credit expectation to the
original submitter), and the boundary rule and description are our own work. A
single `source_licence` column would have to pick one and be wrong about the
rest. `provenance.tsv` is keyed by `(record_id, field)`, so a licence challenge
is answered field by field and a single tainted field can be dropped without
rebuilding anything.

**A boundary is a claim, so it records how it was reached.** "Where does AmpR
start?" has three different kinds of answer:

| `boundary_rule` | Meaning | Derived? |
|---|---|---|
| `orf_atg_to_stop` | Start codon through stop of the frame that translates to the verified reference protein | **yes** |
| `orf_mature_peptide` | The ORF minus a cleaved signal sequence | **yes** |
| `literature_defined` | A publication states it; the DOI is the evidence | no |
| `consensus_of_insdc` | Read off several independent depositors who agree | no |
| `designed_sequence` | A designed part; the boundary is the design | no |

The derived rules are the strongest position available: *we did not copy a
boundary from anyone, we computed one, and the arithmetic is reproducible from
the accession.* Every CDS row is accepted only if translating its nucleotides
reproduces its reference protein **exactly** — a check that is load-bearing
rather than ceremonial. UniProt's `P62593` is a single merged entry covering
TEM-1/2/3/4/5/6/8/16/24 whose cross-references point at *different alleles with
different sequences*; taking the first blindly plants a wrong sequence under a
right name.

## Files

| File | What it is |
|---|---|
| `features.tsv` | One line per feature. The biology. |
| `provenance.tsv` | One line per `(record, field)`. Where each field came from. |
| `SOURCING.md` | Which sources were cleared, with quoted licence evidence. |
| `NOTICE` | Attributions required by the sources in use. |
| `build/build.py` | The harness: id allocation, validation, the id-stability audit, both writers. §8.3 rule 5: *publish the build script, not just the output.* |
| `build/lib_columns.py` | The schema, in one place. Pinned to `crates/pl-features/src/lib.rs` through the header of the file it writes — the Rust loader compares that header against its own `FEATURE_COLUMNS` and refuses the file if they differ. |
| `build/stage_uniprot.py` | Stage 2. UniProt → ENA, one pinned cross-reference per entry, exact translation match. |
| `build/stage_rfam.py` | Stage 3. Rfam seed alignments, with the miRBase and Wikipedia exclusions enforced at parse time. |
| `build/stage_curated.py` | Stage 5. Hand-curated designed parts, one citation each. Most of its table is declared but held; see *Honest coverage*. |
| `build/check_proposed.py` | Proves the shipped tables assert nothing — and proves the check itself can fail. |

Rebuild, then verify:

```bash
PLF_BUILD_DATE=2026-07-28 python features/build/build.py
python features/build/check_proposed.py
python features/build/taint_gate.py
python features/build/archive_legal.py --check
cargo test -p pl-features
```

`PLF_BUILD_DATE` is what makes the output reproducible. Without it the builder
uses today's date, which is written into `#!version`, into every row's
`date_added` and into every own-work provenance `retrieved` — so the same
sources rebuilt on a different calendar day produce a different file. Pin it to
the release date to reproduce a release byte for byte.

The id-stability audit defaults to auditing against the file it is about to
overwrite, which is the *published* table only on a clean checkout. After one
local build it compares the output with itself and cannot fail. Pass
`--baseline <path-to-the-released-features.tsv>` when that distinction matters,
which is any time a row is deliberately re-pinned.

`build.py` exits non-zero if any row was rejected or any stage failed. The
`features.tsv` it writes is always loadable — rejected rows are reported and left
out — so a non-zero exit means the build is incomplete, not that the output is
broken.

### How ids are allocated

Each stage owns a permanently reserved block of the `PLF:` space, and a row's id
comes from **where it is declared**, never from where it landed in the output:

| Block | Stage | Issued |
|---|---|---|
| `PLF:0001`–`PLF:0999` | AMRFinderPlus resistance and selection markers | 24 |
| `PLF:1000`–`PLF:1999` | UniProt → ENA natural proteins | 14 |
| `PLF:2000`–`PLF:2999` | Rfam structured RNA | 24 |
| `PLF:3000`–`PLF:3999` | Hand-curated designed parts | 8 of 28 declared |

A candidate that fails verification, or that cannot yet be expressed in the
schema, leaves its number unissued rather than pulling every later id down by
one. `PLF:3000` is reserved for FLAG and is empty today; when the schema can
carry it, FLAG gets that id and nothing already published moves. The build
re-reads the previous `features.tsv` and refuses to write if any published id
has come to mean a different sequence.

TSV rather than JSON, and normalised across two files, on purpose. This is a
*curated* database whose main interaction is pull requests, and one line per
feature makes a diff reviewable by a biologist. Reviewability is what keeps a
curated database alive — the incumbent's rotted partly because nobody could see
what changed between releases.

## The six rules (`docs/PLAN.md` §8.3)

1. **Never copy SnapGene's descriptions.** Short names — AmpR, KanR, f1 ori — are
   largely unprotectable community nomenclature. Every description is written
   from the primary source.
2. **Never reuse SnapGene's `sseqid` scheme.** `CmR_(2)`, `KanR_(3)`,
   `f1_ori_(3)`: 21.5% of their rows carry that `_(N)` uniquifier and it is a
   fingerprint of copying. Ours are `PLF:0001`.
3. **A CI taint gate** compares our descriptions against theirs and fails the
   build on excessive overlap. It fetches their file transiently at a pinned
   commit, compares, and deletes — no byte is ever committed, enforced by
   `.gitignore` on the name and by `tools/hooks/pre-commit` on the content hash.
   It is `features/build/taint_gate.py`, it runs in CI, and it fails **closed**
   on a network error. Its measured result over all 70 descriptions: no shared
   five-token run anywhere, no row above 60% containment, and two rows above the
   30% warning line — PLF:0002 and PLF:0003, longest shared runs of one and two
   tokens, which is the shared vocabulary of two aminoglycoside
   phosphotransferases rather than shared phrasing. That is the written
   justification the threshold asks for. Disclosing the gate is an asset: it is
   the concrete evidence behind the project's premise.
4. **Per-row provenance** means a single challenged row can be dropped without
   rebuilding, and a licence question can be answered feature by feature.
5. **Publish the build script, not just the output.**
6. **AI may propose, never assert.** Machine extraction may triage candidates and
   draft text. Nothing ships without a named human curator and
   `review_status: reviewed`. This is enforced by the loader, not by discipline.

## Honest coverage

**polylinker-features v0.1 contains 70 feature records: 24 antibiotic-resistance
and selection markers, 14 natural regulatory and enzyme proteins, 24 structured
RNA elements, and 8 designed parts (epitope tags and 2A peptides). Every record
carries an explicit boundary rule, at least one `boundary_evidence` pointer, and
a per-field provenance chain with licence. It is released under CC BY 4.0, with
attribution notices for the U.S. National Library of Medicine, the UniProt
Consortium, EMBL-EBI (ENA and Rfam), Rfam's per-family primary sources, and
INSDC submitters as required by those sources.**

Composition, measured from the shipped file:

| Block | Records | Class | Boundary rule |
|---|---|---|---|
| AMRFinderPlus markers | 24 | `cds` | `orf_atg_to_stop`, translation-verified |
| UniProt → ENA proteins | 14 | `cds` | `orf_atg_to_stop`, translation-verified |
| Rfam structured RNA | 24 | `regulatory` (19), `misc` (5) | `consensus_of_insdc` |
| Curated designed parts | 8 | `synthetic_part` | `literature_defined` |

38 of 70 rows are coding and carry a protein reference verified by exact
translation. 10 rows carry `patent_flag = 1`. **Five** licences are in play
across 818 provenance rows: our own work (446), INSDC-free (156), CC0-1.0 (96),
`unresolved-see-SOURCING-Risk-4` (70) and CC BY 4.0 (50); by source, polylinker
446, Rfam 96, ENA 84, AMRFinderPlus 72, the INSDC feature-table specification 70
and UniProt 50.

Two of those numbers are new and both were previously absent rather than wrong.
`genbank_key` had **no** provenance at all on any row, and it is the one column
`SOURCING.md` Risk 4 flags as legally unresolved: its values are INSDC feature
keys, taken from a specification whose own licence Risk 4 records as
[UNVERIFIED]. Writing `own-work` there would have been a claim we cannot make,
so the licence string says what is actually true and the blank sha256 records
that the specification was deliberately not fetched. And the Rfam nucleotides now
carry **two** provenance rows rather than one: `ena / INSDC-free` for the
depositor's bases, `rfam / CC0-1.0` for the alignment membership that says those
bases are a member of that family. Labelling the bases themselves CC0 attributed
a depositor's sequence to Rfam's waiver — the same conflation this project had
already caught and corrected on the UniProt → ENA leg.

### Aliases that resolve to more than one record

`SOURCING.md` §3 makes the alias table the mechanism that collapses spellings on
a map onto one record, so a spelling that resolves to two records is worth
stating rather than leaving to be discovered. Twelve do, all of them within a
family and all of them deliberate:

| Alias | Records | Why |
|---|---|---|
| `bla`, `ampR`, `beta-lactamase` | PLF:0001, PLF:0009 | TEM-1 and TEM-116. The pUC lineage carries TEM-116; a map says `AmpR` for either. |
| `neo` | PLF:0002, PLF:0003 | aph(3')-Ia and aph(3')-IIa, both called `neo` in the literature. |
| `kanR` | PLF:0002, PLF:0010 | Tn903 and the Gram-positive aphA-3 cassette. |
| `spc` | PLF:0004, PLF:0011 | AadA and ANT(9)-Ia both give spectinomycin resistance. |
| `cmR` | PLF:0005, PLF:0012 | CatA1 and the clostridial CatP. |
| `ble` | PLF:0008, PLF:0018 | Tn5 ble and Sh ble: same family, different phyla. |
| `hygB` | PLF:0019, PLF:0020 | The E. coli and Streptomyces hygromycin enzymes. |
| `MLS`, `emR` | PLF:0021, PLF:0022 | ErmB and ErmC, the same resistance phenotype. |
| `smR` | PLF:0023, PLF:0024 | StrA and StrB, adjacent genes drawn as one block. |

A caller that resolves an alias to a single record will get one of a pair; the
descriptions say which is which and the sequences are 25-34% identical, so
sequence matching does not confuse them. What is *not* here any more is `tetA`,
which used to resolve to three records (PLF:0006, 0013, 0014) — those are now
`tetA(A)`, `tetA(B)` and `tetA(C)`. Drug names (`gentamicin`, `phleomycin`,
`thiamphenicol`) have been dropped as aliases: a drug is not a gene name and
selecting on it is a different concept from being it.

### Boundary rules and alternative initiation codons

Seven rows carry `boundary_rule = orf_atg_to_stop` over a sequence beginning
`GTG` or `TTG` — PLF:0006, 0015, 0017, 0020, 0023, 1000 and 1007. That is not a
contradiction and the rows are correct: the rule means *start codon through stop
codon of the frame that translates to the verified reference protein*, and
`GTG`/`TTG` are real initiation codons read as formyl-Met when they initiate.
tet(A) genuinely begins `GTG`.

The enum's string form is nonetheless narrower than its definition, and
`BoundaryRule::is_derived()` treats this rule as the strongest derivation claim
in the schema, so an auditor reading the label literally would think seven rows
misclaim it. The value is not being renamed — it is published — so the fact is
recorded here and in the enum's own doc comment instead, and every affected row
states its initiator codon in `notes`, measured rather than assumed.

### What this is not

It is **not** a drop-in replacement for pLannotate or SnapGene Common Features:
at 70 rows against their 1,367 it is about 5% of the row count, and it covers
partly different ground. It is **not** complete or comprehensive, and it does not
cover all common plasmid features. It carries **no** coverage claim for
commercial catalogue vectors — pET-28a, pGEX-4T-1 and pMAL-c2 return `Count=0`
from GenBank and there is no clean source for their maps or MCSs. It is **not**
"public domain," "unencumbered," "CC0-equivalent" or "attribution-free" for any
part of it, including the Rfam-derived rows: three upstream sources expressly
decline to grant unrestricted permission. It is **not** cleared of patent claims
— CC BY 4.0 excludes patents by its own terms — and it has **not** been legally
reviewed by counsel. No accuracy benchmark against SnapGene or pLannotate is
claimed here.

### Known gaps

All measured, not assumed, and all documented in `SOURCING.md`.

- **Designed parts are mostly held back by the schema, not by sourcing.** 28
  tags, linkers, protease sites and 2A peptides are declared in
  `build/stage_curated.py` with a citation and a named verification witness
  each; only 8 became rows. The other 20 are peptides with no natural gene —
  FLAG, His6, Strep-tag, SBP, AviTag, ALFA, the GS and EAAAK linkers — and the
  loader requires nucleotides on every row (`lib.rs:556`) while permitting a
  protein reference only on class `cds` (`lib.rs:573`). Back-translating a
  peptide would mean writing a sequence no record contains, so those rows are
  not written. **This is the largest single decision waiting on the curator**;
  see *Open schema question* below.
- **The 8 designed parts that did build carry one natural encoding each**, taken
  from the gene the peptide belongs to and verified by translation. Vector
  versions of these elements are routinely re-coded, so nucleotide matching will
  miss most real occurrences. Right, but not yet useful — the same schema
  question governs.
- **Mammalian selection markers.** `pac`/PuroR and `bsd`/BsdR are absent from
  AMRFinderPlus, confirmed by enumerating its drug-class field: zero PUROMYCIN
  and zero BLASTICIDIN entries. HygR and ZeoR *are* present but under catalogue
  symbols rather than vernacular ones (`aph(4)-Ia` and `ble-Sh`, not `hph` and
  `Sh ble`), which is why they read as missing until the field was enumerated
  properly; both now ship. Codon-optimised eukaryotic versions of any of them
  are absent from every source cleared so far.
- **Engineered fluorescent proteins.** Swiss-Prot curates natural proteins; its
  entire GFP family is 13 wild-type entries. Wild-type avGFP and DsRed ship;
  EGFP, sfGFP and mCherry are simply not in any cleared source and need
  literature curation.
- **Terminators and polyA signals: Rfam contributes zero.** Re-measured rather
  than inherited. Rfam's type vocabulary has no terminator or attenuator class
  at all, and the word appears only inside free-text curator comments. By name:
  `rrnB`, `T7Te`, `tL3`, `SV40 polyA`, `BGH`, `CYC1` and `ADH1` all return zero.
  Hand curation, roughly 20 rows.
- **No EMCV IRES, and no WPRE.** Rfam has no EMCV model — `RF00229 IRES_Picorna`
  is an enterovirus/rhinovirus family, checked by pulling five of its seed
  identifiers and looking them up. The EMCV IRES is on essentially every
  bicistronic mammalian vector, and WPRE is on most lentiviral transfer
  plasmids. Both need hand curation.
- **HIV-1 TAR is barred by our own miRBase exclusion.** `RF00250` is typed
  `Gene; miRNA;`, so the gate that keeps miRBase out also keeps TAR out. A real
  cost of a rule that is right on balance.
- **Promoters, origins and terminators** have no automatable source that gives a
  defensible boundary; depositors disagree with each other, with 21 distinct
  spellings for "origin of replication" and point-versus-range disagreement on
  the same element. Class B is curatorial by nature, and that curation is the
  real work.

### Open schema question for the curator

`SOURCING.md` §3 argues that exact protein matching is *the only sane option for
tags*, because a short designed peptide has dozens of synonymous encodings. The
shipped schema takes the opposite position: `reference_aa` is permitted only on
class `cds`, and `Class::SyntheticPart`'s own documentation says tags are not
translated-matchable. Both cannot be right.

Nothing in this build resolves it, deliberately — it changes what the annotator
finds, so it is a design decision and not an integration fix. Until it is
decided, 20 of the most-used features in molecular biology sit declared,
cited, verified and unissued in `build/stage_curated.py`, each holding the PLF id
it will take when the answer arrives.
