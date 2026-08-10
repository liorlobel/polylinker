<!--
This document is the sourcing record for polylinker-features. It exists because
the database's central claim -- that it is an openly licensed replacement for a
dataset scraped from proprietary software -- is only as good as the evidence
that each source was actually cleared.

How it was produced, stated plainly so its weight can be judged:

  * Six sources were probed independently, each required to QUOTE the operative
    licence sentence with the URL it was fetched from. Recalled licences, badges
    and third-party summaries were not accepted as evidence.
  * Every non-NO_GO verdict was then handed to a separate adversarial pass whose
    instructions were to REFUTE it.

The challenge round overturned material findings in 5 of the 6 probes, and in
one case caught a fabricated verbatim quote with an invented line number -- in a
document whose entire purpose was evidence discipline. Single-pass verdicts were
not safe. Treat the challenge round as the record of fact.

NOTHING HERE IS LEGAL ADVICE, and four items are explicitly flagged as questions
for counsel. Sign-off on the legal position is the PI's, not this document's.
-->

# polylinker-features — Sourcing Decision Document

**Date:** 2026-07-27 · **Status:** for PI sign-off on the legal position · **Author:** synthesis of 6 source probes + 6 adversarial challenges

**Read this first.** I have not independently re-fetched any URL in this document. Everything below inherits verification from two rounds of adversarial probing. Where a claim survived both rounds I state it plainly; where only one round touched it, or neither, I mark it **[UNVERIFIED]** inline. Nothing here is legal advice. Four items are explicitly flagged as counsel questions and should not be resolved by anyone in the lab.

**Process note that matters for how much weight to give the rest.** The challenge round overturned material findings in **5 of 6** probes, and in one case (Sequence Ontology) caught a *fabricated verbatim quote with a fake line number* in a document whose entire purpose is evidence discipline. The single-pass verdicts were not safe. Treat the challenge round as the record of fact, and treat any future source probe as provisional until challenged.

---

## 1. Source table

Verdicts are the **challenger's** wherever prober and challenger disagreed. Disagreements are named, never averaged.

| Source | Licence (as established) | Covers the *data*? | Final verdict | What it yields | Rows: available → usable in v0.1 |
|---|---|---|---|---|---|
| **UniProt/Swiss-Prot** → ENA CDS via EMBL xref | UniProt data: CC BY 4.0, **plus a per-copy notice condition** in `ftp.uniprot.org/pub/databases/uniprot/LICENSE`. ENA leg: INSDC free-and-unrestricted **plus per-record credit to the original submission** | Yes (UniProt fields). ENA nucleotides are **not** covered by UniProt's CC BY — different provenance, must be recorded per field | **GO_WITH_CAVEAT, confidence medium** — *challenger downgraded from high*. Verdict label unchanged; metadata overturned | Curated natural protein + verified nt CDS + exact coordinates in a real construct | 2,441 (broad query) → **~25–60** hand-picked accessions |
| **NCBI GenBank vector records** (E-utilities) + **UniVec** | No affirmative grant. NCBI: "no rights to transfer." NLM Terms require the phrase *"Courtesy of the U.S. National Library of Medicine"* and impose a **currency obligation on redistributors** | Yes in practice, but nobody upstream can warrant it | **GO_WITH_CAVEAT** — *challenger overturned `attribution_required: false` → **TRUE***, and confidence high → medium. This is a direct metadata reversal, not a nuance | The **only** source of depositor coordinates in real constructs. Also the alias raw material | 7,303 "cloning vector" records → **8–12 exemplar backbones** (~700 raw feature rows before dedup, *my estimate*) |
| **UniVec** (same host) | Bare warranty disclaimer, no copyright/licence statement. NGB entries are third-party commercial sequences licensed *to NCBI*, not onward | No | **NO-GO as a coordinate source**; usable only as a name gazetteer | Segments defined by redundancy elimination — boundaries are build-order artifacts | 6,111 segments → **0 sequences**, name hints only |
| **Rfam 15.1** | CC0 1.0 (verified in shipped `COPYING`), **but** family Summary text is Wikipedia-derived (CC BY-SA) and 37.8% of families are miRBase-derived with **no licence at all** | Yes, for the alignments/models/assignments | **GO_WITH_CAVEAT** — *challenger overturned prober's clean **GO***. Conditional on excluding Wikipedia summaries and miRNA families | Structured RNA elements with real sequence, no Infernal needed. **Confirmed negative: Rfam does not model rho-independent terminators** | 4,227 families → **~20–25** allow-listed |
| **FPbase** | Self-contradictory on one page: "CC BY-SA 4.0" and "free of all copyright restrictions." Same page **disclaims the power to grant** | Unresolvable | **HOLD** — both rounds agree. *Challenger overturned the prober's remedy*: emailing the maintainer cannot cure it, because he has already said in writing he cannot give unrestricted permission | **No DNA field exists.** Usable only as a name→accession index | 1,042 records / 991 seqs → **0 sequences**; **349** GenBank accessions usable as pointers |
| **Barrick S1 Table** (McGuffie & Barrick 2024, PLOS ONE) | CC BY 4.0, and now **explicitly** so — figshare record 25915588 carries a machine-readable CC BY 4.0 on this exact file, md5-matched | Yes for the PLOS layer — but the layer launders nothing | **GO_WITH_CAVEAT** — *not refuted; the only probe that survived*. Challenger added a material encumbrance the prober missed | 217 variant/reference/alignment triples + real-world prevalence counts | 217 → **0 in the feature DB**; bench-only (see §5, Risk 5) |
| **pLannotate `snapgene.csv`** | GPL-3.0 covers **code only**. No licence of any kind, anywhere, for the data. Upstream SnapGene reserves all rights | **No** | **NO_GO** — *unchallenged*. Note: unchallenged in the **restrictive** direction, so the gap creates no risk of over-permission | 1,367 rows of ID → curated editorial prose. No sequence, no coordinates | 1,367 → **0**. Fetched at CI only, never committed |
| **Sequence Ontology** | **Contested.** `LICENSE` says CC BY 4.0; the repo `README` §License says CC BY-**SA** 4.0. The LICENSE file was machine-generated from the OBO Foundry badge by an external contributor's bulk script — it is a restatement of the badge, not independent corroboration | Unresolved | **HOLD** — *challenger overturned **GO** → **HOLD***. Full verdict reversal. The prober also fabricated a supporting quote | Controlled vocabulary + INSDC↔SO crosswalk. **No sequences** | 2,615 terms / 260 SOFA → **0 in v0.1** |
| **NCBI AMRFinderPlus Reference Gene Catalog** | Inherits NCBI posture + NLM FTP attribution. No LICENSE file found in the DB directory | Same posture as GenBank | **GO_WITH_CAVEAT** — surfaced late, only lightly probed | **Curated resistance-marker CDS in *nucleotide* and protein, current (2026-05-19), standard allele names** | **[UNVERIFIED]** total count → **~40** |
| **wwPDB / RCSB PDB polymer entities** (`data.rcsb.org` REST) | **CC0 1.0** for archive data, stated in the HTML of `wwpdb.org/about/usage-policies` and archived under a sha256 at `legal/wwpdb-usage-policies.html`: *"Data files contained in the PDB archive are available under the CC0 1.0 Universal (CC0 1.0) Public Domain Dedication."* Depositor attribution is *encouraged*, not required | Yes, for the deposited data. **No** for RCSB's own website layer, which `rcsb.org/pages/policies` licenses separately under CC BY 4.0 | **GO_WITH_CAVEAT (new, 2026-07-28)** — cleared **narrowly**: the build may read the deposited one-letter sequence of a named polymer entity (`entity_poly.pdbx_seq_one_letter_code_can`) and nothing else. Reading no annotation layer is what keeps the CC0 claim and the CC BY layer apart, and it is enforced by the fetch being a single field | The only fetchable, citable witness for a **designed peptide**. An epitope tag or a linker has no gene, so UniProt and ENA have nothing to offer; deposited structures do — FLAG is the entirety of 8RMO entity 1, and the tag is what was crystallised | 18 named entities → **14** peptide references |
| **PlasMapper 3.0 FeatureDB** | Apache-2.0 and GPL-3.0 in two subtrees, nothing at root — and `scrapeFeatures.py` proves the data is scraped from Addgene | No | **NO-GO (new)** | — | 266 → **0** |
| **SEVA** | "Copyright 2019 … All Rights Reserved," no terms page | No | **NO-GO** absent a written grant | — | **0** |
| **Addgene** | Informational, **noncommercial** use only; anti-scraping; Content clause broader than the scraping clause | No | **NO-GO** (confirmed verbatim) | — | **0** |
| **iGEM Registry / SynBioHub / JBEI** | Unretrievable. iGEM ToS: 403 CloudFront *(challenger; the prober's "status changed to 504" is not reproducible and should not enter the record)*. SynBioHub/JBEI: SPA shells, no licence text | Unknown | **HOLD, unverified** — "I found no licence" ≠ "there is no licence" | — | **0** |

### Disagreements, named

1. **NCBI, `attribution_required`: `false` → `TRUE`.** The prober never opened the NLM Terms and Conditions, which govern the FTP servers they pulled UniVec from and which mandate a specific phrase. **We adopt the challenger's TRUE.**
2. **Sequence Ontology, `GO` → `HOLD`.** The prober's two "independent" sources were one circular source, and SO's own maintainers wrote the BY-SA statement and left it standing through 2025 edits. **We adopt HOLD and drop SO from v0.1.**
3. **Rfam, `GO` → `GO_WITH_CAVEAT`.** Share-alike (Wikipedia) and unlicensed (miRBase) material rides inside a source declared CC0-clean. Bounded and avoidable, but only by an *enforced* exclusion. **We adopt GO_WITH_CAVEAT.**
4. **UniProt, confidence `high` → `medium`**, plus a named per-copy notice obligation the prober never located. **We adopt medium.**
5. **FPbase remedy overturned.** "Email the maintainer for a CC0 grant" is foreclosed by FPbase's own disclaimer. Only the index-only route survives.
6. **Barrick S1: the prober was right and the challenger could not break it** — but the challenger found that `variant_sequence`, the one column the prober recommended keeping, is Addgene-derived. See Risk 5.

---

## 2. Extraction plan, in build order

### Stage 0 — Legal scaffolding (before any ingest; ~2 days)

Nothing is fetched into the repo until these exist.

**0.1 `NOTICE` file** containing, verbatim:
- `(c) 2002-2024 UniProt Consortium` + the CC BY 4.0 URL, and a statement of changes made (CC BY's "indicate if changes were made" condition is live — protein → verified CDS → feature record is a transformation).
- `Courtesy of the U.S. National Library of Medicine`.
- A pointer to NCBI's Disclaimer and Copyright notice at `https://www.ncbi.nlm.nih.gov/home/about/policies/` (the old `/About/disclaimer.html` now 302s there), rendered so it is **evident to users of the product**, not buried in a repo file.
- EMBL-EBI attribution for the ENA and Rfam legs.
- Rfam CC0 + per-family primary-source credit.
- A dated-snapshot banner: *"This dataset is a dated snapshot (retrieved YYYY-MM-DD) and does not reflect the most current data available from NLM."*

**0.2 Provenance schema — per *field*, not per row.** A single row can mix CC BY 4.0 (UniProt naming), INSDC (the nucleotides), and our own work (the boundary rule and the description). Conflating them is the exact failure this project exists to avoid.

**0.3 Archive the evidence that self-evidences.** `www.uniprot.org/help/license` and `ebi.ac.uk/ena/browser/about/policies` are JavaScript shells with **zero licence text**. Archive `rest.uniprot.org/help/license` (JSON), `ftp.uniprot.org/pub/databases/uniprot/LICENSE`, the knowledgebase `README`, `Rfam/CURRENT/COPYING`, and the NLM Terms page — as files, with sha256 and retrieval date, in `legal/`.

**0.4 CI taint gate.** Fetch-at-CI, pinned to the **immutable commit SHA** `61ed152e9f8c9abc3c5c1b01eabfc28b63cda551` (tags are mutable; `master` no longer contains the file), assert `sha256 == 793631d9eebf721efae9e1d6cd483b1cbb62f5adad41174afa8f8b78b1342d5c`, compute, delete. `.gitignore` the filename **and** a pre-commit hook rejecting any staged blob with that hash. Commit only the hash + pinned SHA + date. Fail **closed** on network error with a distinct `taint-gate-unavailable` status — never auto-pass.

Metric, as designed by the probe and adopted unchanged:
- **Containment** `|A ∩ B| / |A|` (ours over theirs), not Jaccard — their descriptions run longer (median 11 tokens, max 45).
- Strip stopwords first (their top tokens are the/of/from/for/to/and at 615/542/349/246/206/193 occurrences).
- Absolute floor: ≥3 shared non-stopword tokens (66 of their rows are ≤3 tokens; one shared "protein" hits 33%).
- **Any shared contiguous 5-token n-gram = hard fail**, regardless of ratio.
- ≥30% containment → warn + written justification in the PR. ≥60% or any shared 5-gram → hard fail.

**Rejected and why:** storing hashed shingles *is* redistribution in obfuscated form — their vocabulary is 2,619 distinct tokens, a salted-hash dictionary attack over any biology wordlist recovers it in seconds, and a committed salt in a public repo is public. Chained overlapping shingles are *more* reconstructive than token sets. Global token frequencies are legally fine but cannot implement a per-pair containment metric.

**0.5 Naming firewall.** Never use their `sseqid` scheme. `CmR_(2)`, `KanR_(3)`, `f1_ori_(3)` all exist verbatim in their file; 294 of 1,367 sseqids (21.5%) carry a `_(N)` uniquifier, and in all 167 multi-variant groups the siblings share an identical display name — that suffix pattern is a derived-list fingerprint. Our IDs are our own (`PLF:0001`…). *(Enforced since 2026-08-09 by `tools/ci.ps1` step 'no SnapGene sseqid fingerprint in our ids', which greps the shipped table for the `_(N)` shape. Note what that does **not** cover: our `name` column is compared against nothing of theirs, and no check anywhere in the tree does that.)*

---

### 0.6 The route this document did not consider — SnapGene arriving through INSDC *(added 2026-08-10)*

**The finding.** §0.4's gate compares DESCRIPTION TEXT against `snapgene.csv`. It cannot see a COORDINATE arriving from SnapGene, and one route does exactly that. ENA folds a submitter's SnapGene `/label` into the `/note`, so an ordinary depositor who annotated their plasmid in SnapGene and deposited it publishes SnapGene's **boundary convention** inside a record this document cleared as a source (§1, NCBI/ENA, GO_WITH_CAVEAT). **14 of 481** records surveyed while building Stage 5 carry the fingerprint (2.9%), and **90.9%** of that survey holds at least one extent that a SnapGene-annotated record also holds.

Two things are at stake and conflating them produces a useless check:

- **their editorial PROSE** — copyrightable, covered by §0.4, and Risk 1 already names it;
- **their BOUNDARY CONVENTION** — where they decided "the CMV promoter" starts and stops. §3 of this document calls convention "the actual intellectual content of a feature database" and says it must be derived independently. That is the thing arriving through INSDC, and nothing was watching it.

**A coordinate-level taint check was specified, measured and rejected. It cannot be built honestly, and here is why rather than a promise to try later.**

1. **The pinned artifact has no coordinates.** `snapgene.csv` is `sseqid, Feature, Type, Description` — 1,367 rows, no sequence column, no coordinate column, 1 row (0.1%) stating a length in prose. There is nothing in it to compare an extent against. Their feature *bases* are in pLannotate's `BLAST_dbs.tar.gz`, a GitHub release asset that is not in the pinned tree, carries no licence, and sits on a host `ALLOWED_FETCH_HOSTS` refuses. **Acquiring a bulk copy of SnapGene's extents in order to prove we did not copy SnapGene's extents is a larger act of copying than the one being disproved**, and would be a new §1 sourcing decision, not a build fix.
2. **The sequences are biology and nobody owns them.** The T7 promoter is the T7 promoter. An exact match proves nothing about copying, so a check keyed on "our sequence appears in their file" fires on nearly every legitimate row — and this project has a name for a check like that.
3. **The measured false-positive rate settles it.** Over the 55 distinct extents in the 481-record survey, the rule "fewer than two independent submissions annotate this exact extent" fires on **46 (84%)**, rising to **100%** for extents held by fewer than five submissions. Our twelve Class B candidates escaped only because they are the most-deposited regulatory elements in existence; the next thirty are rarer. On bGH poly(A), **57.1%** of independent non-SnapGene submissions land on SnapGene's extent by themselves — 0.8 bits of information.
4. **The ground truth is not knowable from inside the corpus.** Two independent detectors for "this record was annotated in SnapGene" — the `label:` tell and their `Feature`-column naming — agree on 481 records at **Cohen's κ ≤ 0.067**, and negatively above four names. A depositor who retypes the note by hand is invisible to every tell there is, which is precisely the case a taint check would exist for. **That is not hypothetical, and the instance is inside this database's own witness set** *(measured 2026-08-10; `OP697991.1` is cited in `provenance.tsv` and `MH325107.1` is named in `PLF:4006`'s `notes` as the excluded witness, so this re-derives from two ENA fetches and nothing else)*: `OP697991.1`, one of exactly two independent submissions corroborating `PLF:4006`'s extent, carries four `/note`s whose descriptive half is byte-identical to the corresponding `/note` in `MH325107.1` — a record the screen DOES flag — in the same two-part shape ENA emits when it folds a `/label`, differing by the token `label: ` and nothing else. The screen passes it, correctly by its own rule and wrongly as a matter of fact. Widening the rule to that shape is not obviously right either: `MN224159.1` and `OR659033.1`, which corroborate `PLF:4010`, use the same short names with no descriptive half at all, and those short names are simply what the elements are called.

**Which of those four a reader can re-run, and which they cannot** *(added 2026-08-10, on review)*. **Point 1 re-derives from the tree, and was re-measured on that date**: fetch the pin `taint_gate.py` names and `snapgene.csv` has exactly the four columns `sseqid, Feature, Type, Description` over 1,367 rows, holds no value anywhere that is a coordinate, a span, or a nucleotide string, and has exactly one row (0.1%) stating a length in prose — so the claim that there is nothing in the artifact to compare an extent *against*, which is the load-bearing one, is checkable and holds. So do §0.5's figures: 294 of 1,367 sseqids (21.5%) carry `_(N)`, and all 167 multi-variant groups have siblings sharing one display name. **Points 3 and 4 do not re-derive.** Their figures — 14/481, 90.9%, 46 of 55, κ ≤ 0.067, 57.1% — come from a one-off corpus of 481 INSDC records assembled while building Stage 5; that record list was not preserved, and nothing in this repository rebuilds it. They are cited as the evidence for a decision **not to build** something, which is the one use that survives being unreproducible. Anyone reaching for them to justify anything else has to rebuild the survey and commit it first.

**What is enforced instead, and it is structural rather than statistical.** `features/build/insdc_posture.py`: every stage in `build.STAGES` must declare `INSDC_POSTURE`, naming one of four postures and saying in its own words what it does about this route. The gate refuses a stage that declares nothing, and checks the mechanical part of whatever it declares — that a `no_insdc` stage names no INSDC host, that a `no_feature_table` stage names no record flat-file endpoint, that a `feature_table_forced` stage's named test actually refuses a CDS that differs from its protein, and that a `feature_table_convention` stage's named SnapGene screen still sees the tell and still does not fire on a clean record. It runs inside `taint_gate.py` **before** the fetch, so it still says something on a day the pin is unreachable, and in `tools/ci.ps1`, which is the first half of that gate to have a local twin.

The four postures as declared today:

| Stage | Posture | Why |
|---|---|---|
| `stage_amrfinder` (in `build.py`) | `no_feature_table` | Two FASTA files; the extent is the reading frame |
| `stage_uniprot` | `feature_table_forced` | Takes the depositor's own CDS location — and the nucleotides must translate residue for residue to a named UniProt canonical, so nobody could have moved that boundary |
| `stage_rfam` | `no_feature_table` | Extent is an Rfam seed-alignment interval; `ena_fetch()` refuses anything but `/fasta/` |
| `stage_curated` | `no_insdc` | A tag has no gene; nothing here is fetched from an INSDC host at all |
| `stage_classb` | `feature_table_convention` | The exposed case, and the reason the vocabulary has a fourth entry |

**Say plainly what this buys and what it does not.** It does not prove that no coordinate in `features.tsv` agrees with SnapGene's, and it cannot. It proves that no stage reached the table without a human answering the question, and that four specific mechanisms named in those answers still work. §6's disclosure list is amended accordingly: the taint gate must be described as a check on **descriptions**, never as a check on the database.

**And one thing that is a data rule, and must not be called a taint check.** §4's week 1–2 line already asks for "≥2 independent GenBank exemplars **showing where depositors actually place it**". Only the first half of that sentence was executed: `stage_classb.verify()` required two independent submissions to *contain the bases*, measured where each of them drew the edges, wrote that into `notes`, and tested nothing. `MIN_PLACEMENTS` now makes the second half executable — two independent submissions must annotate a feature at **exactly** the shipped extent before a row may claim `boundary_rule = consensus_of_insdc`. It names no vendor, reads no `/note`, and cannot show that an extent came from anywhere; it says only whether our own evidence forced it. **Five of the twelve Class B candidates fail it and do not ship** (`PLF:4002` lac, `PLF:4003` tac, `PLF:4004` trc, `PLF:4005` CMV promoter, `PLF:4011` SV40 early poly(A)) — each corroborated by exactly one submission, which is one lab's opinion and not a consensus.

---

### Stage 1 — AMRFinderPlus resistance markers *(first, because it is the best value per unit of legal risk)*

**Uniquely contributes:** curated nucleotide CDS for resistance markers, with standard allele nomenclature, **currently maintained** (2026-05-19) — unlike every other candidate. This is a materially better source for selection markers than filtering Swiss-Prot by annotation score.

```
https://ftp.ncbi.nlm.nih.gov/pathogen/Antimicrobial_resistance/AMRFinderPlus/database/latest/
  AMR_CDS.fa    (11 MB, nucleotide)
  AMRProt.fa    (4.4 MB, protein)
```

Steps: list the directory to confirm the catalog TSV filename **[UNVERIFIED — the probe confirmed only the two FASTAs]**; pull both FASTAs; filter to a hand-written allow-list of markers actually used in cloning vectors (`bla`/TEM-1, `aph(3')-Ia`, `aph(3')-IIa`/nptII, `cat`, `tet(A)`+`tetR`, `aadA1`, `aac(3)-IV`, `ble`, `hph`); for each, **translate the CDS and require an exact match to its AMRProt entry** — mismatch means drop, not fix.

**[UNVERIFIED and important]:** AMRFinderPlus targets *clinical bacterial* AMR. Whether `hph` (HygR), `pac` (PuroR), `bsd` (BsdR) and `Sh ble` (ZeoR) are present is unknown and must be checked before Stage 1 is scoped. If absent, those move to Stage 5 curation.

---

### Stage 2 — UniProt → ENA CDS, for natural proteins not in AMRFinderPlus

**Uniquely contributes:** authoritative naming, family, organism and provenance metadata for natural proteins, and a *verifiable* protein↔nucleotide link.

```
# 1. Discovery (bulk /stream tested: 2,329 rows / 498,728 bytes in 0.74 s)
https://rest.uniprot.org/uniprotkb/search?query=(reviewed:true)+AND+(annotation_score:3+OR+annotation_score:4+OR+annotation_score:5)+AND+(database:embl)+AND+(keyword:KW-0046)&format=tsv&fields=accession,id,protein_name,gene_names,organism_name,length,annotation_score,xref_embl
```
**Query gotcha:** `annotation_score` is a `general` filter, not a range filter. `annotation_score:[3 TO 5]` returns HTTP 400. Counts come from the `X-Total-Results` header.

```
# 2. The TSV xref gives only the nucleotide accession, NOT the CDS id. Use JSON:
https://rest.uniprot.org/uniprotkb/P62593.json
   → uniProtKBCrossReferences[database=='EMBL'] → properties['ProteinId']   (J01749 → AAB59737.1)

# 3. CDS nucleotides
https://www.ebi.ac.uk/ena/browser/api/fasta/AAB59737.1        → 861 nt

# 4. MANDATORY: translate, require exact match to the UniProt canonical
# 5. Parent record for coordinates
https://www.ebi.ac.uk/ena/browser/api/fasta/J01749.1          → 4,361 bp; CDS at complement(3293..4153)
```

**Step 4 is not optional.** P62593 is a merged multi-allele entry covering TEM-1/2/3/4/5/6/8/16/24; its 9 EMBL xrefs point to *different alleles with different sequences*. `AAB59737.1` and `CAA23886.1` match exactly; `CAA45828.1` differs at Q37K, E102K, G236S. Taking `xref[0]` blindly silently plants wrong sequences.

**Ingest only the EMBL cross-reference.** P62593 also carries 15 DrugBank, 6 KEGG, 2 ChEMBL and 1 DrugCentral xrefs. KEGG states it "is not a public database" and that non-academic use "requires a commercial license"; DrugBank's terms returned HTTP 403 and are **[UNVERIFIED]**. UniProt cannot relicense any of them. Field-level allow-list at ingest, always.

**Do not attempt a bulk 2,441-row ingest.** The coverage gap is disqualifying for most of the scope: `DYKDDDDK` returns **0** hits across all of UniProtKB; "FLAG tag"/"HA tag"/"myc tag" as protein_name each return **0**; `family:"GFP family" AND reviewed:true` returns **13** entries, all wild-type. The 99 free-text "EGFP" hits are incidental mentions in unrelated entries (CRLF3_HUMAN, SMG5_MOUSE…) — **do not report "EGFP: 99" as coverage.** Restrict Stage 2 to a hand-picked allow-list: `lacI`, `lacZα`, `araC`, `cI`/`cI857`, T7 RNAP, Cre, FLP, φC31 Int, SacB, CcdB, GST (Sj26), MBP (`malE`), wild-type avGFP `P42212`, DsRed `Q9U6Y8`.

Prefer ENA over NCBI for the CDS fan-out (NCBI E-utilities is the rate-limited step at 3 req/s without a key). Cache locally — EMBL-EBI reserves the right to block usage that hinders its operations.

---

### Stage 3 — GenBank exemplar records, for boundaries and non-coding features

**Uniquely contributes:** the only depositor-annotated coordinates in real constructs anywhere in this document. Everything else gives bare sequence.

```
https://eutils.ncbi.nlm.nih.gov/entrez/eutils/efetch.fcgi?db=nuccore&id=J01749&rettype=gb&retmode=text&tool=polylinker&email=YOU@example.org
```
≤80 comma-joined IDs per call (80 records = 1.38 MB in one HTTP 200). 3 req/s bare, 10 with `&api_key=`. `tool=` and `email=` must be **registered with NCBI** — supplying them in the URL alone is explicitly stated as insufficient.

Parse: split on lines equal to `//`; slice between `^FEATURES` and `^ORIGIN`; feature keys at column 6 (`/^ {5}(\S+) +(\S.*)$/`); **unwrap continuation lines with `re.sub(r'\n {21}', ' ', block)` before extracting qualifiers** — otherwise every multi-line `/note` and `/translation` is truncated. Use Biopython `SeqIO` in production.

**Five hard rules, each derived from a measured failure:**

1. **Never key on `/label`.** Zero occurrences across 94 fetched records; it is not an INSDC qualifier. It is a SnapGene/VectorNTI/ApE/Addgene-export convention. An importer keyed on `/label` silently ingests nothing. Names live in `/note` (615 of 665 uses), `/gene`, `/product`, `/standard_name`.
2. **Never trust the COMMENT block.** L09137.2 (canonical pUC19) has exactly **one** feature — `source 1..2686`. Its bla/lac/polylinker exist only as unparseable VecBase free text, and the comment's `1629-2417` is 789 bp against a real 861 bp bla CDS — it annotates the mature peptide.
3. **Never transfer coordinates between records of the same plasmid.** L09137.2 and M77789.2 are both pUC19, both 2,686 bp, and neither is a rotation of the other: L09137 is a rotation of the **reverse complement** of M77789 at shift 2002. Re-anchor every feature by alignment across both strands and all rotations.
4. **Use a curated accession allow-list, not name search.** `pET-28a[All Fields]` = 15 hits topped by a patent sequence and a razor-clam mRNA; `[Title]` cuts it to 2; `pET28a[Title]` returns 7 *different* records. There is no reliable query enumerating "all records for vector X."
5. **Add a sanity layer before ingest.** X06403 (pACYC184) contains `misc_feature 219..3805 /note="chloramphenicol resistance gene"` — 3,587 bp for a ~660 bp gene, containing the tet gene inside it. NCBI performs no biological QC. Check CDS length vs product, ATG/stop, overlap plausibility.

Two schema landmines to design around now: `rep_origin` is a **point** in J01749 (`rep_origin 2535`, no qualifiers) and a **913 bp range** in X06403 — record which. And gene-vs-CDS spans are not equal: pBR322's are identical, pGEX-2T's gene (1286..2216) swallows a 70 bp promoter the CDS (1356..2216) excludes.

Starting allow-list: J01749, L09137.2, M77789.2, X06403, U13850, X52327, U03442, plus 4–6 more. Expect 7/80 (9%) of "cloning vector … complete sequence" records to carry nothing but `source`.

---

### Stage 4 — Rfam, for structured RNA elements

**Uniquely contributes:** IRES, riboswitches, ribozymes and plasmid replication-control RNAs — elements with no protein representation that GenBank vector records do not reliably annotate.

```
https://ftp.ebi.ac.uk/pub/databases/Rfam/CURRENT/database_files/family.txt.gz   (378,804 B; 4,227 rows × 35 cols)
   load-bearing: col0 rfam_acc, col1 rfam_id, col3 description, col9 comment, col14 num_seed, col18 type
https://ftp.ebi.ac.uk/pub/databases/Rfam/CURRENT/fasta_files/RF00106.fa.gz      (7,538 B; 166 seqs)
https://ftp.ebi.ac.uk/pub/databases/Rfam/CURRENT/Rfam.seed.gz                   (109,390 curated seed seqs)
```

**Four enforced exclusions and one transform:**
- **`tr U→T`.** The FASTA is literal RNA alphabet. Skip this and every Rfam-derived feature silently returns zero hits — it looks like an empty-database bug.
- **Exclude `type = "Gene; miRNA;"`** — 1,598 of 4,227 families (37.8%) are miRBase-derived, and miRBase publishes **no licence at all** (homepage, /help/, /download/ all checked: zero occurrences of licence/copyright/CC0/public domain; only a citation demand). Irrelevant to plasmids anyway.
- **Never ingest family Summary text, and never fetch `wikitext.txt.gz`.** Rfam's own docs say the summaries come from Wikipedia (CC BY-SA 4.0); Rfam ships a 1,518-row table mapping families to Wikipedia articles. Rfam's CC0 cannot relicense it. *(Caveat on the caveat: the RNA WikiProject historically seeded Wikipedia **from** Rfam, so per-family direction of copying is murky — which is itself a reason not to touch it.)*
- **Prefer `Rfam.seed.gz` over full-region FASTA** for anything user-facing. The per-family FASTAs are every cmsearch hit across ENA — chromosomal and environmental, heavily redundant.
- **Carry per-family primary-source credit.** CC0 imposes no legal duty; Rfam's docs explicitly ask for it and EMBL-EBI's Terms expect attribution notwithstanding a resource-level CC0.

Sanity note: for RF00106, `family.txt` reports `num_full=156` but the shipped FASTA contains **166**. Trust the FASTA; treat `num_full` as advisory.

**Confirmed negative, and it costs us:** Rfam does **not** model standalone rho-independent terminators. The 30 families matching "terminator"/"attenuat" are all riboswitches, leaders or sRNAs whose *mechanism* involves a terminator hairpin. rrnB T1, T7Te, tL3 must be curated by hand.

---

### Stage 5 — Human curation (see §4)

### Stage 6 — Benchmark repo, **separate from the feature DB**

Barrick S1 lands here and only here.

```
curl -sSL -o S1.csv 'https://journals.plos.org/plosone/article/file?id=10.1371/journal.pone.0304164.s001&type=supplementary'
# 368,649 bytes, UTF-8 with BOM, CRLF, md5 86393bb2e0158d5ade78d768a749bd92
```
Read with `encoding='utf-8-sig'`; run the **whole** file through `csv.reader` first, then drop `row[0].lstrip().startswith('#')` — there are **17** comment records (not 16), **two** of which are CSV-quoted because they contain commas. A naive line-prefix filter leaks a phantom 1-column header.

**Mandatory column exclusions** (`db`, `db_id`, `description`, `reference_sequence` — all SnapGene-derived via pLannotate). Prefer the GitHub mirror, which *structurally* omits two of them: `raw.githubusercontent.com/barricklab/widespread-recurrent-part-variants/main/data/supplemental_table_1.csv.gz` (sequences byte-identical for 217/217; the 88/217 description diffs are trailing whitespace only).

**Keep only:** the prevalence columns (`num_of_variant_occurrences`, `num_of_unique_pis`) as a ranking prior — available nowhere else, and aggregate counts are thin-copyright facts. `variant_sequence` itself is held pending counsel (Risk 5).

**The matching benefit was overstated and this is why S1 is not urgent.** From the alignment column: 79/217 variants differ from reference by exactly 1 nt, 84 by 2 nt, 203/217 (93.5%) are ≥96% identical, median identity 99.60%. SnapGene's documented threshold is ≥96%. An ordinary local aligner at 96% already catches 93.5% of these **without this file**. Only ~14 rows (6.5%) are genuinely additive.

---

## 3. The central design question: where do boundaries come from?

**Reframe first.** "Boundaries" is not one problem, it is two, and conflating them is why the plan looked stuck.

- **Boundary *convention*** — does "AmpR" include its promoter? does bla start at the signal peptide or the mature peptide? does "ori" mean a point or a 913 bp span? This is the actual intellectual content of a feature database. It is *exactly* what SnapGene's Common Features encodes and *exactly* what we must derive independently.
- **Boundary *instantiation*** — given a reference sequence, where does it sit in the user's plasmid? This is a computation the annotator performs at runtime. We never ship it, so we never need to source it.

The plan's anxiety is about the first. The answer is that convention is **computable for one feature class and curatorial for the rest**, and the architecture should split on exactly that line.

### The three classes

**Class A — CDS-derived (boundaries are *facts*).** Resistance and selection markers, reporters, recombinases, nucleases, natural fusion partners, in-frame tags. The boundary is the ORF: start codon through stop codon of the reading frame that translates to the verified reference protein. Nobody chose it; it is a property of the sequence. This is the strongest possible provenance story — *we did not copy a boundary, we computed one, and here is the arithmetic.*

**Class B — non-coding regulatory (boundaries are *conventions*).** Promoters, enhancers, terminators, polyA signals, operators, RBS/Kozak, origins, LTRs, insulators, WPRE. No automatable source gives a defensible boundary. GenBank depositors disagree with each other (21 distinct spellings for "origin of replication"; point-vs-range for the same element).

**Class C — synthetic/designed (boundaries are *stipulated by a paper*).** Epitope tags, linkers, protease sites, 2A peptides, MCSs, codon-optimised ORFs, engineered FPs. The boundary is whatever the designing paper says. One citation per row, and that citation *is* the provenance.

### Answering the 6-frame question directly

**Yes — translated matching is decisively better for resistance markers, reporters and tags, and it does *not* cost boundary precision.** But it must be a second tier, not a replacement.

**Why nucleotide matching fails where it fails.** A human-codon-optimised GFP and EGFP are 100% identical at the protein level and can fall to ~70–75% nucleotide identity — far below any 96% threshold, below practical BLASTN detection for a 700 bp feature. The same holds for humanised Cas9, mammalian-optimised PuroR/HygR/BsdR, and most reporters used outside *E. coli*. Nucleotide matching does not degrade gracefully here; it fails silently and completely.

**Why nucleotide matching nevertheless works most of the time.** Bacterial resistance markers are direct descendants of Tn3/Tn5/Tn9 and are usually *not* re-coded. The Barrick prevalence data confirms it empirically: median real-world variant identity to reference is 99.60%. So tier 1 handles the common case fast and exactly.

**Why translated matching is the *only* sane option for tags.** FLAG is `DYKDDDDK` — one 8-residue string. At the nucleotide level it has dozens of synonymous encodings, and His6 alone appears as `CATCACCATCACCATCAC`, `CACCACCACCACCACCAC` and mixtures. One protein pattern replaces the entire synonymous family. False-positive math is comfortable: 8 residues over a 20-letter alphabet against ~10,000 residue positions (6 frames × 5 kb) gives ~10⁻⁶ per plasmid — **provided short features require exact protein match with no substitutions.** Rule: features under ~15 aa are matched exactly, never by scored alignment.

**The misconception to kill: translated hits give exact nucleotide coordinates.** An amino-acid interval in a known frame maps back as `nt_start = frame_offset + 3×(aa_start−1)`, extended to the upstream ATG and through the stop codon. Precision is lost only at the *ends* — whether to include the signal peptide, the RBS, the promoter — and that is a convention question that has to be an explicit field regardless of which tier found the hit.

### Concrete architecture

```
Tier 1 — nucleotide, all feature classes
  Index every reference_nt (k-mer seed + banded Smith-Waterman).
  Circularity: concatenate query + query[0:k_max] before searching, then mod coordinates back into 1..L.
  Exact-to-near-exact hits only. Fast, exact boundaries, and the ONLY tier that can find Class B features
  (a promoter has no protein).

Tier 2 — translated, Class A rows only
  6-frame translate the QUERY (not the reference), search against reference_aa, restricted to
  regions tier 1 did not already cover.
  A few hundred protein references → plain 6-frame + banded SW is fast enough in-process.
  No external dependency, no BLAST, no DIAMOND.
  Map hits back to nt coordinates as above; snap 5' end to the nearest in-frame ATG, 3' to the stop.

Output records which tier made each call.
```

That last line is a product differentiator, not a footnote. SnapGene and pLannotate return a hit; we return *a hit plus how we found it plus the boundary rule we applied*. That is the entire value proposition of "provenance-tracked."

### Schema consequences

```
id                 PLF:0001                    # ours, never a SnapGene sseqid, never a _(N) suffix
name               ours
aliases[]          {alias, source, source_accession}
type               INSDC feature key           # NOT an SO term in v0.1 — see Risk 4
class              CDS | regulatory | origin | repeat | synthetic_part | misc
reference_nt       sequence                    # required on class A and on every class whose
                                               # boundary is a claim about bases; MAY BE EMPTY on
                                               # a synthetic_part that carries a peptide instead
reference_aa       sequence                    # class A and class C. On A it is what makes
                                               # codon-optimised detection possible at all; on C
                                               # it is usually the whole record, because a tag IS
                                               # a peptide (see below). The invariant is AT LEAST
                                               # ONE reference, not "a nucleotide reference".
boundary_rule      ORF_ATG_to_STOP | ORF_mature_peptide | literature_defined
                   | consensus_of_INSDC | designed_sequence
boundary_evidence  accession.version + coords + strand, OR DOI + table/figure
exemplars[]        {accession.version, start, end, strand, retrieved}
provenance[]       PER FIELD: {source, licence, url, retrieved, sha256_of_archived_copy}
patent_flag        bool + note                 # see Risk 6
description        WRITTEN BY US, always
```

**Resolved 2026-07-28.** This section argued from the start that exact protein matching is the only sane option for tags, while the shipped schema said `reference_aa` was Class A only — the one place this document contradicted itself. The PI decided it: *"Yes — add these sequences, but make sure they are fused to an ORF, otherwise ignored."* Class C may now carry a peptide, with or without nucleotides, under three rules:

1. **A peptide-only row is matched exactly and wholly**, at zero edit distance, regardless of `Config::min_identity`. The false-positive arithmetic above holds only under exact matching, and `min_identity` is user-adjustable — at 0.80 an eight-residue tag gets an edit budget of one and the annotator starts reporting FLAG tags that are not there.
2. **A peptide hit must be fused to an ORF**, or it is ignored. The hit must lie in frame inside an open reading frame *of the query*, with at least 20 residues of that ORF outside the tag. The ORF is detected in the molecule, not taken from a tier-1 annotation: people tag their own protein, not AmpR, so a "must overlap an annotated CDS" rule would be invisible in exactly the case the rows exist for.
3. **The residue string is verified at build time**, against a freshly fetched wwPDB polymer entity in which it must occur exactly once — the same discipline the nucleotide rows get from `locate_unique`.

What rule 2 buys, measured rather than asserted: for an 8-residue tag under the shipped defaults it admits about 37% of the six-frame positions such a tag could start at on random 5 kb sequence — a factor of **2.7x** — and about **2.1x** on real vectors (pBR322 `J01749`, pUC19 `L09137`, pTrc99A `U13872`), which are denser in coding sequence than random DNA. An earlier version of this line said 4.7x, from an estimate rather than a run; `features/README.md` carries the full table and the floors either side of 20. **It is not the false-positive control** — exact matching is. It is the clause that makes the *claim* ("this is a tag on a protein") mean something. What it costs is stated in `features/README.md`: a tag on a partner shorter than 20 residues, a 5'-truncated fragment with no initiator, an empty tagging vector whose polylinker meets a stop within 20 codons, and a tag on a gene whose initiator the configured genetic code does not accept — all read as absent.

Keying the DB on **verified protein identity** for Class A also dissolves the naming problem for free: `bla`, `Bla`, `ampR`, `AmpR`, `beta-lactamase`, `ampicillin resistance protein` and the other six measured spellings collapse into one record with an alias table hanging off it. And alleles that differ by a few residues (the TEM family) become explicit variants of a parent group instead of silent corruption.

---

## 4. Honest gaps — what needs human curation, and what the 8 weeks must actually buy

Stages 1–4 automate roughly **60–120 distinct features** *(my estimate; depends on the unverified AMRFinderPlus content)*. Everything below is not automatable from any clean source.

**Gap 1 — Epitope tags, linkers, protease sites, 2A peptides.** FLAG/3xFLAG, HA, Myc, His6/8/10, V5, Strep-II, Twin-Strep, S-tag, AviTag, SBP, SUMO, TEV/3C/thrombin/Factor Xa sites, T2A/P2A/E2A/F2A, GS linkers. UniProt returns **0** for all of them. Each is a short designed sequence with one citable origin paper. Fast per item; ~30 items.

**Gap 2 — Engineered fluorescent proteins.** UniProt's entire GFP family is 13 wild-type entries. FPbase has 991 protein sequences and **no DNA field at all** — verified by enumerating every key across all 1,042 records. Route: FPbase as an uncopyrightable name→accession index (349 GenBank, 193 UniProt), sequence pulled from the primary source. That covers maybe a third; the rest need literature curation. Query by `name__icontains`, not `iexact` — sfGFP is stored as "Superfolder GFP".

**Gap 3 — Non-coding regulatory elements. This is the largest cost.** CMV, CAG, EF-1α, PGK, SV40, U6, H1, T7/T3/SP6, lac/tac/trc, pBAD, TRE/tetO, rhaBAD; CMV and SV40 enhancers; rrnB T1/T2, T7Te, SV40/bGH/hGH polyA; Kozak/Shine-Dalgarno; WPRE. GenBank annotates many but with inconsistent keys (retired `promoter` vs `regulatory` + `/regulatory_class`), inconsistent naming (name sits in `/standard_name` in one record, `/note` in another), and inconsistent boundaries. Rfam explicitly cannot help with terminators.

**Gap 4 — Origins of replication.** ColE1/pMB1/pUC vs pBR322 (they differ by copy-number mutations and *should be distinct records* — the Barrick data flags this specifically), p15A, pSC101, R6Kγ, f1/M13, SV40, oriT, oriV, 2μ, CEN/ARS, BAC oriS/repE/parABC. Plus the point-vs-interval schema decision.

**Gap 5 — Multiple cloning sites, and the commercial catalogue generally. This gap cannot be closed at all.** There is **no GenBank record** for pET-28a, pGEX-4T-1 or pMAL-c2 — `esearch` returns `Count=0` with `PhraseNotFound`. Novagen/Merck, NEB, Cytiva and Thermo do not deposit their catalogue vectors, and their published maps are copyrighted. GenBank covers pre-1995 academic backbones well and the modern commercial catalogue not at all. **v0.1 must not claim commercial-vector coverage.**

**Gap 6 — Eukaryotic selection markers and their codon-optimised forms.** PuroR (`pac`), HygR (`hph`), BsdR (`bsd`), ZeoR (`Sh ble`), NeoR. Possibly partly in AMRFinderPlus **[UNVERIFIED]**; the codon-optimised versions certainly not. Tier 2 solves *detection*; the reference sequence still needs sourcing.

**Gap 7 — Engineered nucleases and effectors.** SpCas9/dCas9/Cas12a, base editors, Cre, FLPe/FLPo, Tn5, PiggyBac, Sleeping Beauty, rtTA/tTA, Gal4/UAS, LexA. Wild-types exist in UniProt (14 reviewed Cas9, 1,098 recombinase); the humanised and engineered versions do not. Plus the heaviest patent exposure in the DB.

**Gap 8 — Viral backbone elements.** HIV-1 and MSCV LTRs, Ψ, RRE, cPPT/CTS, WPRE, AAV ITRs. Sequences exist in deposited viral genomes; the boundaries are conventional.

### How the 8 weeks must be spent

*These are my allocations, not verified estimates.* The critical point is that the automatable sources cover the **easy, well-defined** features and leave every **convention-defining** feature to humans. Do not budget curation time for things Stages 1–4 already do.

| Week | Work | ~Items |
|---|---|---|
| 1–2 | Promoter/enhancer boundary conventions. Per item: primary paper, published sequence or TSS-anchored definition, explicit `boundary_rule`, **≥2 independent GenBank exemplars** showing where depositors actually place it | ~35 |
| 3 | Terminators + polyA signals. Rfam confirmed unable to help | ~20 |
| 4 | Origins. Includes the point-vs-interval schema decision and pUC-vs-pBR322 ColE1 discrimination | ~12 |
| 5 | Tags, linkers, protease sites, 2A. **Cheapest week, highest value** — short sequences, one paper each, and they are what makes Tier 2 worth building | ~30 |
| 6 | Engineered FPs via the FPbase→GenBank index route, with per-item accession verification and a patent-status note | ~35 |
| 7 | Eukaryotic selection markers + codon-optimised variant modelling | ~15 base |
| 8 | **Alias table + writing every description from scratch + running the taint gate.** The probe measured 12 spellings for AmpR, 21 for ori, 191 distinct `/note` strings from 665 uses, 70 distinct `/gene` from 573, and called the alias table "the dominant engineering cost of using GenBank." Budget a full week; it is not padding | few hundred aliases |

Eight weeks is **not** enough for a comprehensive database. It is enough for a defensible ~250-row one. Say so out loud.

---

## 5. Risks to the central claim, ranked

The claim under defence: *a CC BY 4.0, provenance-tracked plasmid feature database that is not derived from SnapGene's proprietary list.*

**Risk 1 — Convergent-copy accusation.** *Bites if:* our names, boundaries and description prose resemble `snapgene.csv` at a rate not explained by both describing the same biology. Their `Description` column is human-written editorial prose — the tokens "et" and "al" each occur 392 times, i.e. inline literature citations. That is a copyrightable expression layer and it is the thing to stay away from. *Cheapest mitigation:* the Stage 0.4 gate + 0.5 naming firewall + writing every description ourselves + an independent `boundary_evidence` pointer per row. **~1 day of build, then discipline.**

> **The three limbs of Risk 1 are not equally defended, and this line is what says so.** *Description prose* is measured by §0.4 on every CI run. *Names* are defended only against their `sseqid` *shape* (§0.5): nothing compares our `name` column against their `Feature` column, anywhere. *Boundaries* cannot be compared at all — §0.6 sets out why, and what is enforced in place of a comparison. Anyone quoting this document on Risk 1 should quote §0.6 with it.

**Risk 2 — Attribution non-compliance.** *Bites if:* we ship without the NOTICE file and per-row source accessions. This is not hypothetical — **three separate per-copy/per-row obligations were missed by probers and found only on challenge**: UniProt's copyright statement reproduced with each copy, NLM's mandated phrase, and INSDC's per-*record* credit expectation (which sits two sentences from the quote one prober used to claim no attribution was required). A failure here does more than breach terms; it torpedoes the word "provenance-tracked," which is the product's entire differentiator. *Cheapest mitigation:* NOTICE file + `source_db`/`source_accession.version`/`retrieved` per row. **Half a day. The best value in this document.**

**Risk 3 — The NLM currency obligation cannot be expressed in CC BY 4.0.** Redistributors must maintain current data **or** conspicuously state they do not. CC BY has no mechanism to bind downstream licensees, so we would accept a duty we structurally cannot propagate. *Bites if:* someone forks a stale snapshot and NLM cares. Enforcement risk is realistically low, but this is a genuine licence-compatibility defect and the PI is signing the legal position. *Mitigation:* dated-snapshot banner in README, in the data file header, and in a machine-readable field; state plainly that we cannot bind downstream. **One hour. [Counsel question — do not resolve internally.]**

**Risk 4 — Sequence Ontology turns out to be BY-SA and we shipped the crosswalk.** *Bites if:* we ingest the INSDC↔SO crosswalk (143 feature + 91 qualifier synonyms) or SO definition prose, and the README's BY-SA statement governs. The evidence favours the restrictive reading: the BY-SA text was added by the SO team in 2020 and left standing through three 2025 README edits, while the CC BY 4.0 LICENSE was machine-emitted by an outside contributor's bulk script from the OBO Foundry badge. *Mitigation:* **use INSDC feature keys as the `type` column; define a `so_term` field and leave it empty in v0.1; file an issue at The-Sequence-Ontology/SO-Ontologies asking which document governs.** Near-zero cost — and INSDC keys are what the GenBank round-trip needs anyway. **[UNVERIFIED]** whether the INSDC feature-table *specification document* itself carries a licence; individual keys like "CDS" are short standard terms and the set is a de facto interoperability standard, but I have no verified authority on the document.

**Risk 5 — Addgene noncommercial taint on the Barrick variant sequences.** *Bites if:* we ship `variant_sequence` in a commercially-redistributable CC BY 4.0 artifact and Addgene asserts. The paper's Methods state the sequences were extracted from 51,384 Addgene plasmids; Addgene's ToU grants "informational, noncommercial use only" and bars reproduction for any commercial purpose. There is a real counter-argument (the ToU is a site contract binding the accessor, not a downstream CC BY recipient; raw DNA sequence has thin copyright) — **but it is the identical argument that would excuse SnapGene, and the project cannot deploy it selectively without destroying its own premise.** *Mitigation:* keep S1 out of polylinker-features entirely; bench-only; flag before bench ships publicly. **Zero cost** — the additive matching benefit was 6.5% of rows.

**Risk 6 — Patents on precisely the sequences we most want.** CC BY 4.0's legalcode states plainly that patent and trademark rights are not licensed; UniProt and FPbase both say outright they cannot give unrestricted permission because some data may be patented. Cas nucleases and fluorescent proteins are among the most heavily patented sequences in molecular biology. *Bites if:* distributing a reference sequence for annotation purposes is itself an infringing act — which is a different question from making or using the DNA, and one I have **[UNVERIFIED]** authority on in either direction. *Mitigation:* `patent_flag` field on affected rows + an explicit statement that the DB grants no patent licence. Hours. **[Counsel question.]**

**Risk 7 — Third-party contamination inside sources we called clean.** KEGG ("not a public database," commercial licence required) rides in UniProt xrefs; DrugBank's terms are unverifiable (403); IMGT ("All rights reserved," 79 SO definitions) and Wikipedia (26 SO definitions, plus all Rfam summaries) ride inside SO and Rfam; miRBase (no licence whatsoever) is 37.8% of Rfam. *Mitigation:* **field-level allow-lists at ingest — parse only named fields, never "everything the API returns."** Free if adopted now, expensive to retrofit.

**Risk 8 — Coverage overreach.** *Bites if:* we position a ~250-row DB as a replacement for a 1,367-row one and users hit the gaps (no commercial MCSs, thin FP coverage, no codon-optimised marker references). The project trades on credibility; one overclaim spends it. *Mitigation:* a quantified coverage statement and a published known-gaps list. Costs only the willingness to write §6.

**Risk 9 — The evidence package does not self-evidence.** `www.uniprot.org/help/license` returns HTTP 200 / 6,473 bytes with **zero** occurrences of "Creative Commons"; `ebi.ac.uk/ena/browser/about/policies` returns 12,665 bytes of Angular shell with zero licence text. If our legal position cites a URL that serves an empty shell, it fails at exactly the moment it is tested. *Mitigation:* archive REST JSON and FTP files, with hashes, in `legal/`. **An afternoon.**

**Risk 10 — Copyleft contamination via tooling.** pLannotate is GPL-3.0; PlasMapper carries Apache-2.0 in one subtree, GPL-3.0 in another, and nothing at root — any licence scanner reports "Apache-2.0" and someone ships it. *Mitigation:* clean-room, no fork, the CI gate, and a licence scan on every vendored dependency. Note that PlasMapper's `scrapeFeatures.py` proves its FeatureDB is Addgene-scraped, so it is doubly out.

---

## 6. Recommended v0.1 scope

**~240 feature records + ~25 exemplar located-feature rows.** Small, fully provenanced, every row defensible line by line.

| Block | Source route | Count *(target)* |
|---|---|---|
| Resistance / selection markers | AMRFinderPlus + UniProt→ENA, translation-verified | ~40 |
| Natural protein features | UniProt→ENA allow-list, exact translation match | ~25 |
| Structured RNA elements | Rfam seed, allow-listed families, U→T | ~20 |
| Tags, linkers, protease sites, 2A | Human curation, one citation each | ~35 |
| Promoters / enhancers / terminators / polyA | Human curation + ≥2 GenBank exemplars each | ~40 |
| Origins | Human curation, explicit point-vs-interval | ~15 |
| Engineered fluorescent proteins | FPbase index → GenBank/UniProt sequence, patent-flagged | ~40 |
| Aliases | Hand-built adjudication table | few hundred |
| **Exemplar located features** | 8–12 classic backbones, re-anchored by alignment | ~25 *(bench, not the annotation DB)* |

Every row carries: our name, our description, an INSDC feature key, a reference sequence with a stated `boundary_rule`, ≥1 `boundary_evidence` pointer, and per-field provenance with licence.

**The honest coverage claim, verbatim-ready:**

> polylinker-features v0.1 contains ~240 feature records covering classic academic *E. coli* cloning backbones, standard antibiotic-resistance and selection markers, standard epitope tags and linkers, common mammalian expression elements, and selected structured RNA elements. Every record carries an independently derived boundary, an explicit boundary rule, and a per-field provenance chain with licence. It is released under CC BY 4.0, with attribution notices for UniProt, the U.S. National Library of Medicine, INSDC submitters, EMBL-EBI and Rfam as required by those sources.

### What the release notes must NOT say

- **Not** "a drop-in replacement for pLannotate or SnapGene Common Features." It is ~18% the row count and covers partly different ground.
- **Not** "complete," "comprehensive," or "covers all common plasmid features."
- **Not** any coverage claim for commercial catalogue vectors — pET-28a, pGEX-4T-1 and pMAL-c2 return `Count=0` from GenBank and we have no clean source for their maps or MCSs.
- **Not** "public domain" or "unencumbered." Three upstream sources expressly decline to grant unrestricted permission: NCBI ("no rights to transfer"), UniProt and FPbase (both: "cannot provide unrestricted permission").
- **Not** "cleared of patent claims" or anything implying it. CC BY 4.0 excludes patents by its own terms.
- **Not** "legally reviewed" unless counsel has actually reviewed it.
- **Not** an accuracy benchmark against SnapGene or pLannotate — unless the benchmark ran without ingesting their data, and the CI gate output is published as proof.
- **Not** "CC0-equivalent" or "attribution-free" for any part of it, including the Rfam-derived rows.
- **Not** a bare "contains no SnapGene data." Disclose the CI taint gate instead: it transiently fetches `snapgene.csv` at a pinned commit for comparison only, and no byte of it is committed or redistributed. That disclosure is an asset, not a liability — it is the concrete evidence behind the project's entire premise.
- **Not** "the CI gate proves this database is not derived from SnapGene." It proves that of **descriptions**, and the artifact it is pinned to holds no sequence and no coordinate, so it can prove nothing else. The boundary route is §0.6: declared by every stage, checked structurally, and not measurable against their data by anyone. Describing the gate as covering the database rather than the description column is the precise overclaim this project exists not to make.

### Immediate next actions

1. Enumerate the AMRFinderPlus `latest/` directory and confirm whether `hph`, `pac`, `bsd` and `Sh ble` are present. **This single check moves ~15 features between Stage 1 and Week 7.**
2. File the SO governance issue asking which of `README` §License and `LICENSE` governs.
3. Build the NOTICE file and the per-field provenance schema before a single byte is ingested (Risk 2, cheapest fix available).
4. Put four questions to counsel, together: the NLM currency pass-through (Risk 3), the Addgene chain on `variant_sequence` (Risk 5), patent exposure on distributing reference sequences (Risk 6), and whether EU sui generis database right changes the Feist-style "coordinates are facts" defence.