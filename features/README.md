# polylinker-features

An openly licensed, provenance-tracked database of common plasmid features.

**Licence: CC BY 4.0** for the data. See [`NOTICE`](NOTICE) for the attributions
this carries, and [`SOURCING.md`](SOURCING.md) for how each source was cleared
and by what evidence.

> **Status: v0.1 pre-release. Every row is `proposed` — machine-extracted, with
> no human sign-off. Nothing here is shippable yet.** `Db::reviewed()` will
> return an empty database until a curator puts their name against each row.
> That is the intended state, not an unfinished one; see *Rule 6* below.

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
| `build/build.py` | The pipeline. §8.3 rule 5: *publish the build script, not just the output.* |

Rebuild with:

```bash
python features/build/build.py
```

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
   commit, compares, and deletes — no byte is ever committed. Disclosing that
   gate is an asset: it is the concrete evidence behind the project's premise.
4. **Per-row provenance** means a single challenged row can be dropped without
   rebuilding, and a licence question can be answered feature by feature.
5. **Publish the build script, not just the output.**
6. **AI may propose, never assert.** Machine extraction may triage candidates and
   draft text. Nothing ships without a named human curator and
   `review_status: reviewed`. This is enforced by the loader, not by discipline.

## Honest coverage

v0.1 covers standard bacterial antibiotic-resistance and selection markers,
built from NCBI's AMRFinderPlus catalogue with every CDS translation-verified.

It is **not** a drop-in replacement for pLannotate or SnapGene Common Features:
it is a small fraction of the row count and covers partly different ground. It
is **not** complete or comprehensive. It carries **no** coverage claim for
commercial catalogue vectors. It is **not** "public domain" or "unencumbered" —
three upstream sources expressly decline to grant unrestricted permission — and
it is **not** cleared of patent claims, which CC BY 4.0 does not grant anyway.

Known gaps, all documented in `SOURCING.md`:

- **Mammalian selection markers** (`hph`/HygR, `pac`/PuroR, `bsd`/BsdR) are
  absent from AMRFinderPlus, which is a clinical bacterial catalogue. Confirmed
  by enumerating it, not assumed. They need hand curation.
- **Epitope tags and engineered fluorescent proteins** cannot come from
  Swiss-Prot: it curates natural proteins. `DYKDDDDK` (FLAG) returns zero hits
  across all of UniProtKB; `family:"GFP family" AND reviewed:true` returns 13
  entries, every one a wild-type protein. EGFP and mCherry are simply not there.
- **Rho-independent terminators** are not modelled by Rfam. Confirmed negative.
- **Promoters, origins and terminators** have no automatable source that gives a
  defensible boundary; depositors disagree with each other. Class B is curatorial
  by nature, and that curation is the real work.
