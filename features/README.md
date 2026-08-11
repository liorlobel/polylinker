# polylinker-features

An openly licensed, provenance-tracked database of common plasmid features.

**Licence: CC BY 4.0** for the data. See [`NOTICE`](NOTICE) for the attributions
this carries, and [`SOURCING.md`](SOURCING.md) for how each source was cleared
and by what evidence.

> **Status: v0.1 pre-release, 112 records. 89 carry a curator sign-off, and 23 are `proposed`.**
>
> The 89 were signed on 2026-07-28. Twenty-one rows were added on 2026-08-10; on
> 2026-08-11 the curator **withdrew one of them**, `PLF:4006`, the CMV enhancer,
> leaving 20, and three more Class B rows were added the same day — `PLF:4012`
> the T3 promoter, `PLF:4013` the araBAD promoter and `PLF:4014` the human
> EF-1alpha promoter, each appended so that no published id moved. That is 23.
> No human has read those 23, and **`Db::reviewed()` does not ship
> them** — what a user of the tool searches is still 89 rows until a curator
> signs each one.
> `Db::reviewed()` ships only the rows [`SIGNOFF.tsv`](SIGNOFF.tsv) names with a
> content digest that still matches. A sign-off lapses automatically the moment
> the row it approves changes — including a change to its prose, because
> `description` and `notes` are both in `SIGNED_COLUMNS`. The gap between 112
> and 89 is the intended state of a table a machine is allowed to add to; see
> *Rule 6* below, and *What is proposed and not yet signed* for what the 23 are.
> The curator's reading list for them, row by row and contested cases first, is
> [`PROPOSED.md`](PROPOSED.md).
>
> **`PLF:4006` is retired, not recycled.** A withdrawn row leaves the table and
> its id stays spoken for: the declaration remains in `stage_classb.ITEMS` at its
> index, carrying the reason it was withdrawn, so no future element can be issued
> under that number. Deleting it instead would have shifted the T7 terminator
> into `PLF:4006` and moved four more published ids; `stage_classb.self_test()`
> pins those five ids by name, and `build.py`'s id audit refuses any absence that
> is not a declared withdrawal.
>
> **This is a dated snapshot** (sources retrieved 2026-07-27, 2026-07-28 and
> 2026-08-10) and does not reflect the most current data available from NLM,
> UniProt, EMBL-EBI, Rfam or the wwPDB. Per-field retrieval dates and source
> hashes are in `provenance.tsv`; the three dates are three ingest passes, and
> no existing row was re-fetched underneath its signature.

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
| `SIGNOFF.tsv` | One line per **signed** record: who, when, what they checked, and a sha256 of the row's content at the time. A missing, stale or malformed sign-off can only remove trust, never add it. The build reads it and never writes it, and `build/check_writer.py` proves both halves on every push — offline, so the proof does not depend on any upstream server being reachable. |
| `SOURCING.md` | Which sources were cleared, with quoted licence evidence. |
| `NOTICE` | Attributions required by the sources in use. |
| `build/build.py` | The harness: id allocation, validation, the id-stability audit, both writers. §8.3 rule 5: *publish the build script, not just the output.* |
| `build/lib_columns.py` | The schema, in one place. Pinned to `crates/pl-features/src/lib.rs` through the header of the file it writes — the Rust loader compares that header against its own `FEATURE_COLUMNS` and refuses the file if they differ. |
| `build/stage_uniprot.py` | Stage 2. UniProt → ENA, one pinned cross-reference per entry, exact translation match. |
| `build/stage_rfam.py` | Stage 3. Rfam seed alignments, with the miRBase and Wikipedia exclusions enforced at parse time. |
| `build/stage_curated.py` | Stage 4. Hand-curated designed parts, one citation each, and two routes: codons sliced out of a natural parent, or a peptide verified against a wwPDB polymer entity. Six of 28 are still held; see *Honest coverage*. (This row said "Stage 5" from before there was one, which stopped being merely wrong the day a real Stage 5 landed underneath it.) |
| `build/stage_classb.py` | Stage 5. INSDC-anchored Class B conventions — promoters, terminators, poly(A) signals. One anchor record per row, re-sliced every build, plus ≥2 witnesses from *different submitting addresses* and ≥2 of those placing the feature at *exactly* the shipped extent. Reads no `/note`, `/label`, `/gene`, `/product` or `/standard_name`, and refuses a SnapGene-annotated record as a witness. Ten held elements carry their reasons in `HELD` and five more rows are refused on the extent rule; see *Class B*. |
| `PROPOSED.md` | The curator worklist for every row that is `proposed`: what it claims, which accessions to check it against, the boundary chosen and on what basis, **the primary source that settles it**, and a recommendation — sign, withdraw, or a decision the evidence cannot make — with the decisions first and the arithmetic afterwards. Its *Claims*, *Anchor*, *Sources* and witness lines are read out of `features.tsv` rather than retyped, so they cannot drift from the table. Carries no digests, deliberately: `SIGNOFF.tsv` says signing a digest nobody has read is not an attestation. |
| `build/insdc_posture.py` | The stage-posture gate. Every stage in `build.STAGES` must declare `INSDC_POSTURE` — what it does about a boundary convention arriving from a depositor's INSDC feature table — and this refuses a stage that declares nothing, then checks the mechanical half of whatever was declared. Runs inside `taint_gate.py` before the fetch, and in `tools/ci.ps1` offline. Not a taint check, and its docstring says why one cannot be built here. |
| `build/check_signoff.py` | Proves no row asserts more than a human signed — and proves the check itself can fail, in both directions. |
| `build/check_writer.py` | Proves the build's writer *reads* `SIGNOFF.tsv` and never writes it, over the real shipped rows and with no network. Plants five misbehaving writers and requires itself to catch each, then requires itself to pass a clean one. |

Rebuild, then verify:

```bash
PLF_BUILD_DATE=2026-07-28 python features/build/build.py
python features/build/check_signoff.py
python features/build/check_writer.py
python features/build/taint_gate.py          # runs insdc_posture.py first
python features/build/insdc_posture.py       # ...or on its own, offline
python features/build/archive_legal.py --check
cargo test -p pl-features
```

`PLF_BUILD_DATE` is what makes the output reproducible. Without it the builder
uses today's date, which is written into `#!version`, into every row's
`date_added` and into every own-work provenance `retrieved` — so the same
sources rebuilt on a different calendar day produce a different file. Pin it to
the release date to reproduce a release byte for byte.

`PLF_OFFLINE=1` uses the cache and never the network. Cached files are still
verified against their recorded sha256, so this is "do not fetch", not "trust
whatever is lying around": a stage whose sources are absent reports its source
unavailable and contributes no rows, and the build exits 3. With a warm cache it
reproduces the full table with no network at all, which is how CI audits the
writer without any upstream server's uptime deciding whether the gate is green.

The id-stability audit defaults to auditing against the file it is about to
overwrite, which is the *published* table only on a clean checkout. After one
local build it compares the output with itself and cannot fail. Pass
`--baseline <path-to-the-released-features.tsv>` when that distinction matters,
which is any time a row is deliberately re-pinned.

`build.py` exits non-zero if any row was rejected or any stage failed. The
`features.tsv` it writes is always loadable — rejected rows are reported and left
out — so a non-zero exit means the build is incomplete, not that the output is
broken.

| Exit | What it means |
|---|---|
| 0 | Complete. |
| 1 | A row was rejected or a stage raised. Incomplete, and it is this repository's problem. |
| 2 | A published id would change meaning. **Nothing is written.** |
| 3 | A stage could not reach its upstream host. Incomplete, and it is *not* this repository's problem — re-run when the host is back. |

Exit 3 is separated from exit 1 for the same reason `taint_gate.py` separates
`taint-gate-unavailable` from a finding: *could not check* and *checked and found
wanting* are different answers, and a build that reports someone else's outage as
its own defect sends whoever reads it looking for a bug that is not there.

### How ids are allocated

Each stage owns a permanently reserved block of the `PLF:` space, and a row's id
comes from **where it is declared**, never from where it landed in the output:

| Block | Stage | Issued | Signed |
|---|---|---|---|
| `PLF:0001`–`PLF:0999` | AMRFinderPlus resistance and selection markers | 24 | 24 |
| `PLF:1000`–`PLF:1999` | UniProt → ENA natural proteins | 28 | 14 |
| `PLF:2000`–`PLF:2999` | Rfam structured RNA | 24 | 24 |
| `PLF:3000`–`PLF:3999` | Hand-curated designed parts | 27 of 28 declared | 27 |
| `PLF:4000`–`PLF:4999` | INSDC-anchored Class B conventions | 9 of 25 worked up, 5 more refused, 1 withdrawn | 0 |

**The block follows the stage, not the topic**, and the 2026-08-10 additions
make that visible for the first time: fourteen new *selection markers* landed in
the `PLF:1000` block, beside lacI and GFP, because the route that verified them
is the UniProt → ENA chain and that is what Stage 2 is. They are not next to the
`PLF:0001` resistance markers and they never will be. An id says which stage
built a row and therefore what was checked about it; it does not say what the
row is for. Use `class`, `boundary_rule` and the name for that.

A candidate that fails verification leaves its number unissued rather than
pulling every later id down by one. That mechanism has now been exercised for
real: `PLF:3000` was reserved for FLAG and sat empty for a release, and when the
schema learned to carry a peptide, FLAG took **that** id and nothing already
published moved. One number is still reserved and empty — `PLF:3019`, factor Xa
— held by its occurrence record rather than by sourcing or by length; the five
that used to sit beside it (`PLF:3004`, `3015`, `3016`, `3018`, `3020`) were
issued on 2026-07-28 and signed the same day, at `SIGNOFF.tsv` lines 149-153, on
the short-peptide basis recorded there. The build re-reads the previous
`features.tsv` and refuses to
write if any published id has come to mean a different sequence.

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
   on a network error. Its measured result over all 112 descriptions: **no shared
   five-token run anywhere, and no row above 60% containment.** Five rows sit
   above the 30% warning line, and this is the written justification the
   threshold asks for:

   | Row | Containment | Longest shared run | Why |
   |---|---|---|---|
   | PLF:0002, PLF:0003 | 54.5%, 58.3% | 1 and 2 tokens | The shared vocabulary of two aminoglycoside phosphotransferases. A one-token run is not phrasing. |
   | PLF:1018 URA3 | 36.1% | 4 tokens | *orotidine 5-phosphate decarboxylase* is the enzyme's name. There is no second way to write it, and 13 shared tokens over a short description is what a description of a named enzyme looks like. |
   | PLF:1019 LEU2 | 35.0% | 3 tokens | *3-isopropylmalate dehydrogenase*, same argument. |
   | PLF:1021 TRP1 | 35.7% | 3 tokens | *phosphoribosyl anthranilate isomerase*, same argument. |

   Disclosing the gate is an asset: it is the concrete evidence behind the
   project's premise — **for descriptions, which is all it measures.** It
   compares prose against prose, and pLannotate's `snapgene.csv` carries no
   sequence and no coordinate, so nothing in it could support a comparison of
   *boundaries*. That route — a depositor who annotated a plasmid in SnapGene,
   whose `/label` ENA folds into the `/note` of a record this project cleared —
   is answered by the posture gate below, and by `SOURCING.md` §0.6, which sets
   out why it cannot be answered by a measurement at all.

   **It has now fired for real twice, on two different releases.** First
   PLF:3012, the calmodulin-binding peptide, sharing the eight-token run
   *"skeletal muscle myosin light chain kinase binds calmodulin"*. Then, on
   2026-08-10, PLF:1015 — the fungal blasticidin S deaminase — whose first
   draft opened with the enzyme's name followed by its organism and thereby
   produced a five-token run that occurs verbatim in their file. Neither was
   copied; both are the vocabulary of the subject arriving in the only order
   anyone writes it, and in both cases a sentence boundary vanished when
   stopwords were stripped. The rule is mechanical on purpose, so both times the
   answer was to rewrite the row rather than to argue with the measurement, and
   both times the stage file records why the description is phrased the way it
   is so that nobody "tidies" it back. The gate cost two rewrites and bought the
   only evidence this project's central claim can actually be defended with.

   **The coordinate route, and why it gets a declaration rather than a check.**
   ENA folds a submitter's SnapGene `/label` into the `/note`, so a depositor who
   annotated their plasmid in SnapGene publishes SnapGene's *boundary convention*
   inside an INSDC record — 15 of 481 records surveyed for Stage 5 carry the
   fingerprint (6 by the strong `label:` tell, 9 by prose alone), and
   `SOURCING.md` lists all fifteen so the number can be re-derived. The gate above cannot see it: it compares English strings, and
   the file it is pinned to has no coordinate in it to compare against. Neither
   can anything else here, and `SOURCING.md` §0.6 gives the measurements —
   chiefly that a rule keyed on extent agreement fires on 84% of the distinct
   extents in that survey, which is a check that gets switched off.

   So `features/build/insdc_posture.py` requires every stage to **declare** what
   it does about that route, in a closed vocabulary of four postures, and refuses
   a stage that declares nothing. It then checks the mechanical part of each
   declaration: that a stage claiming to fetch no INSDC record names no INSDC
   host, that one claiming to read no feature table names no flat-file endpoint,
   that the translation test `stage_uniprot` says forces its extents really does
   refuse a CDS one residue out, and that `stage_classb`'s SnapGene screen still
   sees the tell and still does not fire on a clean record. It runs inside the
   taint gate before the fetch, so it reports on a day the pin is unreachable,
   and separately in `tools/ci.ps1`. **It does not show that no boundary here
   agrees with SnapGene's. It shows that nobody reached this table without
   answering the question.**
4. **Per-row provenance** means a single challenged row can be dropped without
   rebuilding, and a licence question can be answered feature by feature.
5. **Publish the build script, not just the output.**
6. **AI may propose, never assert.** Machine extraction may triage candidates and
   draft text. Nothing ships without a named human curator and
   `review_status: reviewed`. This is enforced by the loader, not by discipline.

   Since 2026-07-28 the mechanism is [`SIGNOFF.tsv`](SIGNOFF.tsv): one committed
   line per signed record, carrying the curator, the date, what they checked, and
   a sha256 of the row's semantic content at the time. The rule did not change;
   what changed is that a signature now **lapses by itself** when the thing it
   approved changes — a base, a description, a note, a cited accession, a
   licence. Four controls keep it honest: the build only ever *reads* that file
   and CI runs the build and requires `git diff --exit-code` on it; `coerce_row`
   refuses any stage that emits a status or a curator; the compiled-in loader
   recomputes the digest, so a hand-edited `features.tsv` is refused inside an
   executable somebody already downloaded; and `check_signoff.py` plants each
   forbidden edit into the real table and requires itself to catch it. The
   digest **authenticates nobody** — the build computes it from the same content
   — and `SIGNOFF.tsv`'s own preamble says so rather than implying more.

## Honest coverage

**polylinker-features v0.1 contains 112 feature records — 89 signed off and
shipped, 23 `proposed` and not shipped. The 89: 24 antibiotic-resistance and
selection markers, 14 natural regulatory and enzyme proteins, 24 structured RNA
elements, and 27 designed parts (epitope tags, protease sites, 2A peptides and
linkers). The 23 awaiting a curator: 14 further selection markers and 9 Class B
regulatory elements (five promoters, three terminators and a poly(A) signal).
A tenth Class B row, the CMV enhancer, was withdrawn on 2026-08-11 rather than
signed; its id, `PLF:4006`, is retired and stays so.
Every record carries an explicit boundary rule, at least one
`boundary_evidence` pointer, and a per-field provenance chain with licence. It is
released under CC BY 4.0, with attribution notices for the U.S. National Library
of Medicine, the UniProt Consortium, EMBL-EBI (ENA and Rfam), Rfam's per-family
primary sources, the Worldwide Protein Data Bank, and INSDC submitters as
required by those sources.**

Composition, measured from the shipped file:

| Block | Records | Class | Reference | Boundary rule |
|---|---|---|---|---|
| AMRFinderPlus markers | 24 | `cds` | nt + protein | `orf_atg_to_stop`, translation-verified |
| UniProt → ENA proteins | 28 (14 signed) | `cds` | nt + protein | `orf_atg_to_stop`, translation-verified |
| Rfam structured RNA | 24 | `regulatory` (19), `misc` (5) | nt | `consensus_of_insdc` |
| Curated designed parts | 8 | `synthetic_part` | nt | codons from a natural parent |
| Curated designed parts | 19 | `synthetic_part` | **peptide only** | `designed_sequence` (13), `literature_defined` (6); across all 27, 13 and 14 |
| Class B conventions | 9 (0 signed; a 10th withdrawn) | `regulatory` | nt | `consensus_of_insdc`, corroborated by ≥2 independent placements |

52 of 112 rows are coding and carry a protein reference verified by exact
translation from their own nucleotides. **19 rows carry a peptide and no
nucleotides at all** — the shape decision 1 created — and each was verified by
locating its residue string, exactly once, in a sequence fetched at build time:
a wwPDB polymer entity for 18 of them, and the UniProt canonical of its own
declared parent for the nineteenth (enterokinase, whose five residues are below
`MIN_NT` and so cannot take codons from that parent even though it has one).
20 rows carry `patent_flag = 1`. **Five** licences are in play across
1,292 provenance rows: our own work (731), INSDC-free (242), CC0-1.0 (114),
`unresolved-see-SOURCING-Risk-4` (112) and CC BY 4.0 (93); by source, polylinker
731, ENA 170, the INSDC feature-table specification 112, Rfam 96, UniProt 93,
AMRFinderPlus 72 and the wwPDB 18.

ENA overtook Rfam as the second-largest source in that list on 2026-08-10, and
the reason is worth stating rather than leaving as a number that moved: a Class B
row cites ENA once for the bases it carries, once for the anchor record they were
sliced out of, and once more for **each independent record that witnesses
them** — because for a convention the witnesses *are* the evidence. Nine rows
contributed 58 ENA provenance rows between them, out of 170 in total.

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

### What is proposed and not yet signed

23 rows carry `review_status = proposed`, which means a program put them in the
table and no human has read them. `Db::reviewed()` excludes every one, so
nothing below is searched by `pl annotate`, by the desktop app, or by anything
else, until a curator signs it in `SIGNOFF.tsv`. **They are in the repository so
that a human can read them, which is the only thing the machine is allowed to
ask for.**

[`PROPOSED.md`](PROPOSED.md) is the worklist for that reading: every one of the
23, what it claims, the accessions to check it against, the boundary decision
and its basis, the exact `--show` invocation per row, and — since 2026-08-11 —
the primary source that settles the row plus a recommendation. It opens with the
handful of questions a curator has to *decide* rather than check, because no
further research can make them. The first of those was `PLF:4006`, and it is
**decided**: the curator withdrew it on 2026-08-11, which took the file from 21
rows to read down to 20; the three Class B rows added the same day took it to
**23**. The two that remain open are whether to write alias
spellings ourselves into a UniProt-sourced column, and the three unadjudicated
patent flags. The organism conflict that blocked `PLF:1016` is
resolved — the gene is *Bacillus cereus*, the record's `/organism` is the
expression host — and the resolution is in the row.

**14 further selection markers** (`PLF:1014`–`PLF:1027`), all through the Stage
2 chain, so all of them translation-verified exactly against a UniProt canonical
and all of them coordinate-cited in an INSDC record: `pac` (puromycin), `bsd`
and `bsr` (the two unrelated blasticidin deaminases), `dhfrI` (trimethoprim),
the four yeast markers `URA3`, `LEU2`, `HIS3` and `TRP1`, `TK` (HSV thymidine
kinase, for ganciclovir negative selection), mouse `Dhfr` (methotrexate), `gpt`
(mycophenolic acid), `bar` and `pat` (glufosinate, for plants), and `rpsL` (the
counter-selection half of an rpsL-neo cassette). They give the database its
first yeast markers of any kind, and they **narrow** `SOURCING.md` Gap 6 rather
than closing it: three of Gap 6's five markers were signed before this, the two
added here (`pac`, `bsd`) are `proposed` and therefore searched by nobody, and
Gap 6's codon-optimised half gets nothing from a native CDS. Gap 6's entry
carries the remainder.

Every one of them is the **ORF only**, initiator codon through stop codon, and
excludes the promoter. That is not a preference, it is what the chain derives,
and it is checkable from the row: `len(reference_nt) == 3 × (len(reference_aa) +
1)` on all fourteen. The trap it creates is stated in each row's `notes`: what a
vector map labels `PuroR` is a promoter-ORF-poly(A) *cassette*, `URA3` on a pRS
map is the gene with its own promoter and terminator, and `TRP1` in the YRp7
lineage means TRP1-ARS1. A match against those files covers the ORF and stops,
and that is correct.

**9 Class B regulatory elements**, from the fifteen that were built — see the
next section for the five that were refused and why. Ten reached the table,
seven on 2026-08-10 and three more on 2026-08-11; the curator withdrew
`PLF:4006` on 2026-08-11, so of the fifteen built, five were refused on the
evidence and one was withdrawn by a human, which are different things and are
counted separately here on purpose. None of the nine is signed, so none of them
is searched.

### Class B: boundaries that are conventions, and how they are evidenced

`SOURCING.md` §3 divides features by where their boundary comes from. Class A
boundaries are *facts* (the ORF), Class C boundaries are *stipulated by a paper*
(a designed part), and **Class B boundaries are *conventions*** — nothing says
where "the CMV promoter" ends. §6 prescribes the method: human curation plus at
least two independent GenBank exemplars each, and §4 says what those exemplars
are for — "showing where depositors actually place it". `build/stage_classb.py`
executes both halves rather than asserting them.

**Fifteen elements were built and nine are in the table.** The nine are the T7,
SP6, T3, araBAD and human EF-1α promoters, the T7 (Tφ), rrnB T1 and rrnB T2
terminators, and the bGH poly(A) signal; the last three promoters were appended
on 2026-08-11 as `PLF:4012`–`PLF:4014`, having been held until measurement
contradicted the reasons they were held for. A tenth, the CMV enhancer
(`PLF:4006`), reached the table on 2026-08-10 and was withdrawn by the curator
on 2026-08-11; its id is retired.
Each one claims exactly four things:

- **These bases are `accession:lo-hi` on this strand.** Re-fetched and re-sliced
  on every build, and cross-checked between the record's FASTA view and its flat
  file. A row whose coordinates stop holding its sequence is dropped, never
  corrected.
- **At least two INSDC records from different submitting addresses contain those
  exact bases**, and where each of those depositors put the edges relative to
  ours is *measured at build time* and written into `notes` — `5'+48/3'+6` means
  that depositor's feature starts 48 bases earlier and ends 6 later, in the
  element's own orientation.
- **At least two of those independent submissions annotate a feature at exactly
  this extent**, edge for edge, with no tolerance. This is the claim
  `consensus_of_insdc` actually rests on, and it is the one that was measured and
  never tested until 2026-08-10: holding the bases is a fact about the sequence,
  drawing the same edges is the only thing that makes the word *consensus* true.
  **Five rows failed it and never reached the table at all** — lac
  (`PLF:4002`), tac (`PLF:4003`), trc (`PLF:4004`), the CMV promoter
  (`PLF:4005`) and the SV40 early poly(A) signal (`PLF:4011`), each corroborated
  by exactly one submission out of the two to four that hold its bases. They
  keep their ids, stay in the stage's allow-list, and are re-measured on every
  build, so a row returns by itself the day a curator cites evidence that
  corroborates its extent — or re-cuts it to an extent the evidence already
  corroborates. That is a curator's judgement and not a program's, which is why
  they are refused rather than adjusted.
- **Nothing about the extent being right.** `boundary_rule = consensus_of_insdc`
  says it is a convention, and the rival conventions are named in `notes` with
  their offsets.

The corroboration rule **is not a taint check and must not be described as one.**
It names no vendor, reads no `/note`, and cannot show that an extent came from
anywhere. It answers the one question that is answerable from inside this
repository: did our own evidence force this extent, or is it one lab's opinion?

Three findings from building it are worth having in the open, because two of
them are about this repository's own controls:

1. **INSDC is contaminated with SnapGene, and the CI taint gate structurally
   cannot see it.** Ordinary submitters deposit records annotated in SnapGene;
   ENA folds SnapGene's `/label` into the `/note`, so a record reads
   `/note="promoter for the E. coli lac operon; label: lac promoter"`. The taint
   gate compares *our descriptions* against theirs — it has no way to notice a
   *coordinate* arriving this way. Two consequences, both mechanical:
   `stage_classb.py` never reads a `/note`, `/label`, `/gene`, `/product` or
   `/standard_name` at all, and a record carrying the `label:` tell is **not
   counted as an independent witness**. Counting two SnapGene-annotated deposits
   as "two exemplars" would manufacture exactly the convergence this project
   exists to disclaim. It fires on real data: five of the fifteen rows built —
   `PLF:4002`, `4005`, `4006`, `4010` and `4011` — have a witness excluded for
   this reason. Two of those five reached the table, `PLF:4006` and `PLF:4010`,
   and each names the exclusion in its own `notes`; `PLF:4006` has since been
   withdrawn, so `PLF:4010` is the only one of the five still in the table — and
   it is `proposed`, like every Class B row, so it does not ship either.

   **What that leaves, and what now covers it.** Excluding the records that carry
   the tell does nothing about a depositor who retyped the note, and no detector
   can: two independent tells for "annotated in SnapGene" — the `label:` string
   and their `Feature`-column naming — agree across 481 records at Cohen's
   κ ≤ 0.067. `PLF:4005` is the case in the open: the CMV promoter's only exact
   corroboration was `LC897329`, whose feature table is SnapGene's Common Feature
   naming throughout with no `label:` in it. It was the corroboration rule above,
   which names no vendor at all, that refused the row. **`PLF:4006` was the case
   that is NOT in the open**, and until 2026-08-11 it was in the table as
   `proposed`: *both* of its two
   corroborating submissions carry a fingerprint the tell cannot see —
   `LC897329` again, and `OP697991`, four of whose `/note`s have a descriptive
   half byte-identical to `MH325107`'s, differing by the `label: ` token the
   screen matches on and by nothing else (measured 2026-08-10). The row was still
   attested by two independent submissions, which is what its `notes` claimed;
   what it was not is two submissions this screen has cleared. **The curator
   withdrew that row on 2026-08-11**, and the reason he gave includes this — only
   the SnapGene-shaped submissions draw its 380/204 split, while every other
   deposit in the project's evidence annotates the region as one element and
   calls it a promoter. Read that as a case the disclosure was written for, not
   as the disclosure being retired: the blind spot is exactly as wide as it was,
   no detector closed it, and the next row to fall into it will not announce
   itself either. And every stage now
   declares what it does about this route in `INSDC_POSTURE`, with
   `features/build/insdc_posture.py` refusing one that declares nothing;
   `SOURCING.md` §0.6 is the adjudication.
2. **"Two exemplars" has to mean two *submissions*.** A quarter of the surveyed
   corpus is one bulk deposit from one culture collection; by record count some
   of these elements have dozens of witnesses and by submission they have three.
   Independence is decided from the address on each record's own submission
   reference, and addresses are merged when they look like one lab writing its
   address two different ways — a comparison biased towards merging, because
   over-merging can only make the gate harder to pass.
3. **Depositor strand annotation is not trustworthy.** Several records annotate
   a T7 or SP6 promoter without `complement()`, so the enclosed bases are the
   reverse complement of the promoter: the span is right and the strand is not.
   Every sequence is therefore located in every witness on *both* strands and
   the strand actually found is recorded, never inherited.

`genbank_key` on these rows is `promoter`, `terminator` or
`polyA_signal` — **all three of which INSDC has retired** in favour of
`regulatory` plus a `/regulatory_class` qualifier, and the anchor records
themselves have moved (V01146 writes `regulatory` + `/regulatory_class=
"promoter"`). The stage emitted the current spelling first, and it broke
something: `Db::absent_common_kinds` probes the table for the literal keys
`promoter`, `terminator` and `rep_origin`, and that probe is what makes the
desktop app and `pl methods annotate` say *"no promoter is in this database
yet"*. Under `regulatory` the Class B rows are invisible to it, so the app would
have kept saying "no promoter" after promoters were signed — a user-facing claim
made false by a schema decision nobody would think to connect to it. This schema
has no column for `/regulatory_class`, so the choice was between a current key
that says nothing and a retired key that says what the feature is. The retired
key won because something real depends on it. If they are ever changed again,
`absent_common_kinds`'s probe list is the thing that has to change with them.

**Ten elements are worked up and are not rows**, each for a stated reason
recorded in `stage_classb.py`'s `HELD` list, rewritten on 2026-08-11 against
fresh measurement: the SV40 early promoter (its interval wraps the record's
numbering origin in every genome record checked, which `boundary_evidence`
cannot express — the schema is the obstacle, not the evidence); U6 (many
submissions hold the bases, exactly one draws the edges); H1 (its 216 nt form is
one department's, and is not a verbatim slice of the gene record that has now
been found for it); PGK (one placement, and a second that `verify()` cannot see
because it is the record's *second* copy of the element); three separate
chicken-β-actin and CAG entries, replacing the one entry that used to cover
them, of which the best-evidenced fails only because its second witness is the
record `SOURCING.md` §0.6 names as a demonstrated false negative of the SnapGene
screen; and three tet entries, which are the split the old dropped `tetO / TRE /
Ptet` line asked for. §6 budgets about forty Class B rows; ten is what survived
the rules applied honestly, after five more were built and then refused by the
exact-extent corroboration rule described above and one was withdrawn — and
those numbers are the finding.

### Aliases that resolve to more than one record

`SOURCING.md` §3 makes the alias table the mechanism that collapses spellings on
a map onto one record, so a spelling that resolves to two records is worth
stating rather than leaving to be discovered. **Seventeen strings do**, counted
over names and aliases together, case-insensitively:

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
| `smR` | PLF:0004, PLF:0023, PLF:0024 | **Three, not two.** AadA is *named* `SmR`, and StrA and StrB both carry it as an alias. Three different enzymes inactivating one drug. |
| `Blasticidin-S deaminase` | PLF:1015, PLF:1016 | UniProt gives the fungal `bsd` and the bacterial `bsr` the identical recommended name. They are unrelated proteins of different lengths that catalyse the same reaction, and a vector map calls either one `BsdR`. |
| `Phosphinothricin N-acetyltransferase`, `PPT N-acetyltransferase`, `Phosphinothricin-resistance protein` | PLF:1025, PLF:1026 | The same three UniProt names for `bar` and `pat`. They are used interchangeably in the plant-transformation literature and are two genes. |
| `strA` | PLF:0023, **PLF:1027** | **The one collision here that inverts a phenotype**, and the only one not within a family. `StrA` is the plasmid aminoglycoside phosphotransferase APH(3'')-Ib, which confers streptomycin RESISTANCE. UniProt lists `strA` as a synonym of `rpsL`, from the early *E. coli* genetics in which resistance mapped to that locus — and wild-type `rpsL` confers streptomycin SENSITIVITY and is used as a counter-selectable marker for exactly that reason. Both usages are real and neither is a typo. |

A caller that resolves an alias to a single record will get one of a set; the
descriptions say which is which and the sequences are far enough apart that
sequence matching does not confuse them. `strA` is the one to be careful with,
and it is called out above rather than left in the list. What is *not* here any
more is `tetA`, which used to resolve to three records (PLF:0006, 0013, 0014) —
those are now `tetA(A)`, `tetA(B)` and `tetA(C)`. Drug names (`gentamicin`,
`phleomycin`, `thiamphenicol`) have been dropped as aliases: a drug is not a gene
name and selecting on it is a different concept from being it.

Two corrections to an earlier version of this section, both found by measuring
it rather than reading it: it said *twelve* strings when the count was over the
whole table and it listed `smR` as resolving to two records when it resolves to
three. The count is now taken from the shipped file.

### Boundary rules and alternative initiation codons

Nine rows carry `boundary_rule = orf_atg_to_stop` over a sequence beginning
`GTG` or `TTG` — PLF:0006, 0015, 0017, 0020, 0023, 1000, 1007, 1017 and 1026.
That is not a contradiction and the rows are correct: the rule means *start codon
through stop codon of the frame that translates to the verified reference
protein*, and `GTG`/`TTG` are real initiation codons read as formyl-Met when they
initiate. tet(A) genuinely begins `GTG`.

The two most recent, PLF:1017 (`dhfrI`) and PLF:1026 (`pat`), are also the
sharpest illustration of why the initiator is a per-row fact and not a family
one: PLF:1025 (`bar`) is the same enzyme as `pat`, the same length in
nucleotides and in residues, and begins `ATG`. The `bar` row's `notes` record
that the two INSDC records for that gene differ at exactly one base — position
1, `ATG` against `GTG` — and which one this database pinned.

The enum's string form is nonetheless narrower than its definition, and
`BoundaryRule::is_derived()` treats this rule as the strongest derivation claim
in the schema, so an auditor reading the label literally would think seven rows
misclaim it. The value is not being renamed — it is published — so the fact is
recorded here and in the enum's own doc comment instead, and every affected row
states its initiator codon in `notes`, measured rather than assumed.

### What this is not

It is **not** a drop-in replacement for pLannotate or SnapGene Common Features:
at 112 rows against their 1,367 it is about 8% of the row count — and 89 of the
112 are what the tool actually searches, which is about 7% — and it covers
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

- **One designed part is still held, and by a MEASUREMENT rather than by
  length.** 28 tags, linkers, protease sites and 2A peptides are declared in
  `build/stage_curated.py`; 27 are now rows. The one that is not is `PLF:3019`
  factor Xa (`IEGR`, 4 aa).

  The floor that used to hold six of them, `MIN_PEPTIDE_AA = 8`, is gone.
  It measured the wrong thing: every peptide in this table was counted against
  73 real plasmid and contig files, 17,061,931 residues of ORFs, counting only
  occurrences the shipped fusion gate would report — and `DDDDK` at five
  residues occurred **zero** times while `IEGR` at four occurred **154**. No
  value of a length floor separates those two, because length is not the
  property being tested. What replaced it is a per-part occurrence record and a
  three-clause gate: a measurement must exist, every occurrence must have been
  read by a human, and none may have turned out to be something else. Factor Xa
  fails the second clause — its 154 hits are unexamined, not shown to be noise —
  and it ships the day somebody reads them.

  The seeding half of the old argument was a real defect and was fixed in the
  code, which is where it belonged: `K_PROTEIN = 5` with `min_seeds = 3` needs
  seven residues to make three windows, and `Index::short()` reported only
  records that seeded *nothing*, so a six-residue peptide was seeded,
  unchainable, absent from every report and never found. `Index::unchainable()`
  now routes any record with fewer indexed words than the caller's `min_seeds`
  to an exact substring scan, so "too few words to chain" is a route rather than
  a hole — and it stays one as `min_seeds` rises, which no constant in the
  builder could have bought.

  **His6 was the most valuable row the floor was holding.** It occurred eight
  times in the corpus and all eight are real tags: C-terminal at exactly -0
  residues from the stop, behind a `GG` linker, in files whose names say the
  construct carries one. Zero measured false positives, eight true positives the
  shipped tool could not find. The `20⁻ᴸ` argument had ranked it the *most*
  suspect row in the table.
- **The 8 designed parts with a natural parent carry one natural encoding
  each**, taken from the gene the peptide belongs to and verified by
  translation. Vector versions of these elements are routinely re-coded, so
  nucleotide matching will miss most real occurrences. That limitation is
  unchanged; what changed is its cause. They *could* now carry a peptide
  reference as well, and deliberately do not — but the reason is now only that
  nobody has done the curation, not that it would be unsafe. The two annotator
  rules key on `Record::is_designed_peptide` (*a `synthetic_part` carrying
  residues*), not on the absence of nucleotides, so a row carrying both gets the
  exactness rule and the ORF-fusion rule on its translated route while its
  nucleotide route stays exactly as it was. Keyed the other way — on
  peptide-only — adding a peptide to one of these eight would silently have
  opened an ungated six-frame path for a nine-residue epitope: a hole that opens
  because of the *shape of a row* rather than because anyone decided anything,
  which is what `Db::audit`'s own comment means by "discipline is not a
  control".
- **Mammalian selection markers — mostly closed, and via the other stage.**
  `pac`/PuroR and `bsd`/BsdR are absent from AMRFinderPlus, confirmed by
  enumerating its drug-class field: zero PUROMYCIN and zero BLASTICIDIN entries.
  HygR and ZeoR *are* present but under catalogue symbols rather than vernacular
  ones (`aph(4)-Ia` and `ble-Sh`, not `hph` and `Sh ble`), which is why they read
  as missing until the field was enumerated properly; both now ship. The rest
  came through the UniProt → ENA chain instead on 2026-08-10 and are `proposed`,
  not shipped: `pac`, both blasticidin deaminases, HSV `TK`, mouse `Dhfr`, `gpt`,
  and the four yeast markers. **Codon-optimised eukaryotic versions of any of
  them are still absent from every source cleared so far**, and that half of the
  gap is the one that matters for detection: a mammalian construct usually
  carries a re-coded `pac`, which these nucleotides cannot match at all. What
  finds those is the translated tier, over the protein reference on the same row.
- **Engineered fluorescent proteins.** Swiss-Prot curates natural proteins; its
  entire GFP family is 13 wild-type entries. Wild-type avGFP and DsRed ship;
  EGFP, sfGFP and mCherry are simply not in any cleared source and need
  literature curation.
- **Terminators and polyA signals: Rfam contributes zero.** Re-measured rather
  than inherited. Rfam's type vocabulary has no terminator or attenuator class
  at all, and the word appears only inside free-text curator comments. By name:
  `rrnB`, `T7Te`, `tL3`, `SV40 polyA`, `BGH`, `CYC1` and `ADH1` all return zero.
  That confirmed negative is why `build/stage_classb.py` exists and anchors on
  primary records instead; `rrnB` T1 and T2, T7 Tφ, and the bGH and SV40 early
  poly(A) signals are `proposed` through it. `tL3`, `CYC1` and `ADH1` are still
  nowhere, and the terminator half of this gap is closed to about a quarter.
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

### The schema question, ANSWERED — 2026-07-28

This section used to pose a question. It records the answer instead.

**The question.** `SOURCING.md` §3 argued that exact protein matching is *the
only sane option for tags*, because a short designed peptide has dozens of
synonymous encodings. The shipped schema took the opposite position:
`reference_aa` was permitted only on class `cds`, and every row had to carry
`reference_nt`. Both could not be right, and 20 of the most-used features in
molecular biology sat declared, cited, verified and unissued as a result.

*(An earlier draft of this section said `Class::SyntheticPart`'s own
documentation asserted that tags are not translated-matchable. It did not. The
claim lived on `Class::Cds` — "this is the only class translated matching can
find" — and on `Record::reference_aa`. The misattribution is corrected here
rather than carried forward.)*

**The answer, from the PI, verbatim:** *"Yes — add these sequences, but make
sure they are fused to an ORF, otherwise ignored."*

So `synthetic_part` may carry `reference_aa`, with or without nucleotides. Four
loader clauses changed, no new column, no new `Class` variant, no new
`boundary_rule`:

| Rule | What it refuses |
|---|---|
| pair-rule | a row carrying **neither** a nucleotide nor a protein reference. The invariant is *at least one reference*, not *no reference required*. |
| `cds` needs bases | a protein-only CDS, whose `orf_atg_to_stop` boundary would be the schema's strongest claim made about a sequence the row does not carry. |
| allow-list of two | `reference_aa` on a `regulatory`, `origin`, `repeat` or `misc` row. A promoter has no protein; a tag is nothing but protein. |
| no derived boundary without bases | a peptide-only row claiming `orf_atg_to_stop` or `orf_mature_peptide`. |

**No new `Class` variant, deliberately.** FLAG's boundary is stipulated by Hopp
et al. 1988; that is `synthetic_part` + `designed_sequence` whether or not the
row happens to carry nucleotides. A `Class::Peptide` would answer a different
question — *which index can find this* — which the two reference columns already
answer exhaustively. It would also split the tag family on an accident of
sourcing: HA tag has a natural parent and ships with codons, FLAG does not, and
under a `Peptide` class those two would land in different classes for a reason
that has nothing to do with either. And `class` is a published string that
`Class::parse` rejects when unknown, so a new value means every already-released
reader **refuses** every new row.

**Two rules the annotator applies to a translated hit on a `synthetic_part`
carrying residues, and to nothing else.** Every shipped row of that shape is
peptide-only today, but the predicate is `Record::is_designed_peptide` rather
than `is_peptide_only`, so the rules follow the peptide rather than the absence
of bases. The nucleotide route is untouched by both.

1. **Exact and whole**, at zero edit distance, regardless of
   `Config::min_identity`. This used to fall out of the arithmetic — the edit
   budget is `floor((1 − min_identity) × len)`, which is 0 for short cores at
   the default 0.96 — but `min_identity` is user-adjustable, and it was never
   the rule, only a consequence of it. Measured before the rule existed: a
   36-of-37 match to the SBP tag was reported as an SBP tag *at the default
   threshold*, with `identity 0.973` and `coverage 1.0`.
2. **Fused to an ORF, or ignored.** The hit must lie in frame inside an open
   reading frame of the *query*, with at least 20 residues of that ORF outside
   the tag. The ORF is detected in the molecule by `pl_core::orf::find_orfs`,
   not taken from a tier-1 annotation — people tag *their own* protein, not
   AmpR, so a "must overlap an annotated CDS" rule would be blind in exactly the
   case these rows exist for. It would also make a tag's visibility depend on
   the curation state of an unrelated row, and would happily certify a fusion
   straight through a nonsense mutation the user's clone really has.

*Ignored* is meant literally: a hit that fails the predicate is dropped, with no
annotation and no fragment.

**What the fusion rule buys, measured rather than asserted.** The quantity is
the share of the positions at which an 8-residue tag could start in the six
translated frames that the predicate admits — which is exactly the share of
chance exact matches that get through — under the shipped defaults:

| substrate | partner floor 20 | 50 | 100 |
|---|---|---|---|
| random sequence, 20 × 5 kb | **2.7×** | 6.4× | 40× |
| pBR322 `J01749`, 4361 bp | **2.1×** | 3.0× | 5.2× |
| pUC19 `L09137`, 2686 bp | **2.3×** | 4.0× | 10.3× |
| pTrc99A `U13872`, 4176 bp | **2.1×** | 3.5× | 8.3× |

An earlier version of this section claimed 4.7× here and ~64× at 100, from an
estimate rather than a run; the numbers above come from `find_orfs` itself over
the three ENA records named. **Real vectors are what users annotate, and there
the gate is worth about 2.1×** — they are far denser in coding sequence than
random DNA, so more of them lies inside some ORF. The conclusion survives the
correction and strengthens: if a 100-residue floor buys 5–10× on a real vector
rather than 64×, paying for it with every bacterial small protein is a worse
bargain than the old number suggested.

**It is not the false-positive control** — exact matching is. It is the clause
that makes the *claim* ("this is a tag on a protein") mean something.

**What it costs, and these are silent:**

- a tag whose fusion partner is shorter than 20 residues — a tagged peptide
  antigen, a tagged peptide hormone, a 12-residue display construct;
- a 5′-truncated fragment: a sequencing read or a Gibson piece covering the
  middle of a tagged gene has no initiator, so no ORF, so no tag. The commonest
  real miss;
- **an empty tagging vector**, conditionally. A cassette not yet fused to an
  insert has no partner by definition, so whether it is reported depends
  entirely on where the first in-frame stop falls downstream: 20+ codons of
  polylinker and stuffer and the tag appears, a prompt stop and it does not.
  This is exactly what "fused to an ORF, otherwise ignored" instructs, and it
  will still look like a bug to someone who opens an empty tagging vector and
  sees nothing;
- a tag on an exon of a genomic knock-in donor, where the partner's exons are
  not one ORF;
- readthrough constructs — amber suppression, selenocysteine recoding, −1
  frameshift cassettes — where the ribosome makes a protein no ORF finder sees;
- **a tag on a gene whose initiator the chosen genetic code does not accept.**
  This one is new with the fusion rule and is worth stating on its own, because
  it is the only silent miss that depends on a *setting*. `find_orfs` runs with
  `require_start`, so which codons may initiate decides which ORFs exist at all.
  The default is now **table 11** — seven initiators, including `GTG` — and it
  was table 1 until 2026-07-28, under which an N-terminal tag on `TetA`,
  `AprR`, `HygR`, `lacI` or lambda `int` (five of this database's 52 CDS rows,
  all beginning `GTG`; there are nine such rows in all) was dropped with no
  output of any kind. C-terminal
  tags on the same genes were accidentally rescued by an internal downstream
  `ATG`, so the miss bit hardest at the N-terminus, which is where His, FLAG and
  Strep tags usually go. Under table 1 the miss is still real for `GTG`, `ATT`,
  `ATC` and `ATA` starts; `pl annotate --code <transl_table>` is how to choose,
  and a eukaryotic construct is a legitimate reason to want table 1's three.

**What it admits that is not a fusion:** a tag inside a long antisense ORF
(nothing about it is expressed, and no ORF rule can close this); a genuine
`HHHHHH` inside a real histidine-rich protein; and chance exact hits at the
residual rate.

Every peptide row's `notes` states these rules, so a user reading one row is
told how it is matched.
