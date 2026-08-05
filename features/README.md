# polylinker-features

An openly licensed, provenance-tracked database of common plasmid features.

**Licence: CC BY 4.0** for the data. See [`NOTICE`](NOTICE) for the attributions
this carries, and [`SOURCING.md`](SOURCING.md) for how each source was cleared
and by what evidence.

> **Status: v0.1 pre-release, 89 records. All 89 carry a curator sign-off dated
> 2026-07-28, so none are left at `proposed`.**
> `Db::reviewed()` ships only the rows [`SIGNOFF.tsv`](SIGNOFF.tsv) names with a
> content digest that still matches. A sign-off lapses automatically the moment
> the row it approves changes — including a change to its prose, because
> `description` and `notes` are both in `SIGNED_COLUMNS`. That is the intended
> state, not an unfinished one; see *Rule 6* below.
>
> **This is a dated snapshot** (sources retrieved 2026-07-27 and 2026-07-28) and
> does not reflect the most current data available from NLM, UniProt, EMBL-EBI,
> Rfam or the wwPDB. Per-field retrieval dates and source hashes are in
> `provenance.tsv`.

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
| `SIGNOFF.tsv` | One line per **signed** record: who, when, what they checked, and a sha256 of the row's content at the time. A missing, stale or malformed sign-off can only remove trust, never add it. The build reads it and never writes it, and CI proves that. |
| `SOURCING.md` | Which sources were cleared, with quoted licence evidence. |
| `NOTICE` | Attributions required by the sources in use. |
| `build/build.py` | The harness: id allocation, validation, the id-stability audit, both writers. §8.3 rule 5: *publish the build script, not just the output.* |
| `build/lib_columns.py` | The schema, in one place. Pinned to `crates/pl-features/src/lib.rs` through the header of the file it writes — the Rust loader compares that header against its own `FEATURE_COLUMNS` and refuses the file if they differ. |
| `build/stage_uniprot.py` | Stage 2. UniProt → ENA, one pinned cross-reference per entry, exact translation match. |
| `build/stage_rfam.py` | Stage 3. Rfam seed alignments, with the miRBase and Wikipedia exclusions enforced at parse time. |
| `build/stage_curated.py` | Stage 5. Hand-curated designed parts, one citation each, and two routes: codons sliced out of a natural parent, or a peptide verified against a wwPDB polymer entity. Six of 28 are still held; see *Honest coverage*. |
| `build/check_signoff.py` | Proves no row asserts more than a human signed — and proves the check itself can fail, in both directions. |

Rebuild, then verify:

```bash
PLF_BUILD_DATE=2026-07-28 python features/build/build.py
python features/build/check_signoff.py
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
| `PLF:3000`–`PLF:3999` | Hand-curated designed parts | 27 of 28 declared |

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
   on a network error. Its measured result over all 89 descriptions: no shared
   five-token run anywhere, no row above 60% containment, and two rows above the
   30% warning line — PLF:0002 and PLF:0003, longest shared runs of one and two
   tokens, which is the shared vocabulary of two aminoglycoside
   phosphotransferases rather than shared phrasing. That is the written
   justification the threshold asks for. Disclosing the gate is an asset: it is
   the concrete evidence behind the project's premise.

   **It fired for real on this release.** PLF:3012, the calmodulin-binding
   peptide, is one of the fourteen rows decision 1 issued, so its description
   faced the gate for the first time — and failed it, sharing the eight-token run
   *"skeletal muscle myosin light chain kinase binds calmodulin"*. Nothing was
   copied; that is the vocabulary of the subject arriving in the obvious order,
   and a sentence break vanished when stopwords were removed. The rule is
   mechanical on purpose, so the answer was to rewrite the row from the cited
   paper rather than to argue with the measurement. `stage_curated.py` records
   why that description is phrased the way it is, so nobody "tidies" it back.
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

**polylinker-features v0.1 contains 89 feature records: 24 antibiotic-resistance
and selection markers, 14 natural regulatory and enzyme proteins, 24 structured
RNA elements, and 27 designed parts (epitope tags, protease sites, 2A peptides
and linkers). Every record carries an explicit boundary rule, at least one
`boundary_evidence` pointer, and a per-field provenance chain with licence. It is
released under CC BY 4.0, with attribution notices for the U.S. National Library
of Medicine, the UniProt Consortium, EMBL-EBI (ENA and Rfam), Rfam's per-family
primary sources, the Worldwide Protein Data Bank, and INSDC submitters as
required by those sources.**

Composition, measured from the shipped file:

| Block | Records | Class | Reference | Boundary rule |
|---|---|---|---|---|
| AMRFinderPlus markers | 24 | `cds` | nt + protein | `orf_atg_to_stop`, translation-verified |
| UniProt → ENA proteins | 14 | `cds` | nt + protein | `orf_atg_to_stop`, translation-verified |
| Rfam structured RNA | 24 | `regulatory` (19), `misc` (5) | nt | `consensus_of_insdc` |
| Curated designed parts | 8 | `synthetic_part` | nt | codons from a natural parent |
| Curated designed parts | 19 | `synthetic_part` | **peptide only** | `designed_sequence` (13), `literature_defined` (6); across all 27, 13 and 14 |

38 of 89 rows are coding and carry a protein reference verified by exact
translation from their own nucleotides. **19 rows carry a peptide and no
nucleotides at all** — the shape decision 1 created — and each was verified by
locating its residue string, exactly once, in a sequence fetched at build time:
a wwPDB polymer entity for 18 of them, and the UniProt canonical of its own
declared parent for the nineteenth (enterokinase, whose five residues are below
`MIN_NT` and so cannot take codons from that parent even though it has one).
17 rows carry `patent_flag = 1`. **Five** licences are in play across
1,008 provenance rows: our own work (598), INSDC-free (156), CC0-1.0 (114),
`unresolved-see-SOURCING-Risk-4` (89) and CC BY 4.0 (51); by source, polylinker
598, Rfam 96, the INSDC feature-table specification 89, ENA 84, AMRFinderPlus 72,
UniProt 51 and the wwPDB 18.

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
at 89 rows against their 1,367 it is about 7% of the row count, and it covers
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
  `AprR`, `HygR`, `lacI` or lambda `int` (five of this database's own 38 CDS
  rows, all beginning `GTG`) was dropped with no output of any kind. C-terminal
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
