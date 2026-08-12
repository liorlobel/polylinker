# PROPOSED.md -- the curator worklist for the 23 unsigned rows

`features/SIGNOFF.tsv` states the rule this database exists to enforce:
**AI may propose, never assert.** These 23 rows are the proposal. Every one
**carries** `review_status = proposed` with an empty curator, none appears in
`SIGNOFF.tsv`, and `Db::reviewed()` ships none of them. Nothing here is in
the product until a named human signs it. Read the two verbs in that sentence
as the different events they are: a row **reaches the table** when a stage
emits it, and a row **ships** when a curator's signature puts it inside
`Db::reviewed()`. Nothing on this page has done the second.

This file is the reading list for that human. **It was a list of open
questions; as of 2026-08-11 it is a list of decisions with the evidence
assembled.** Every row below now carries the primary source that settles it,
what that source settles, and a recommendation -- *sign*, *withdraw*, or
*your call*, with the options and their consequences spelled out for the last.
Where the evidence does not settle a question the row says so and says what
would settle it.

**One of those decisions has been taken. `PLF:4006`, the CMV enhancer, was
WITHDRAWN by the curator on 2026-08-11** and is no longer in the table; the
withdrawal took the worklist from 21 rows to 20. What that cost, how it was
done without moving another id, and the record of the reasons are below under
*`PLF:4006`* and *If you withdraw a row*.

**Three rows were ADDED on 2026-08-11**, after the elements they describe were
re-measured and the reasons they had been held for did not survive it:
`PLF:4012` the T3 promoter, `PLF:4013` the araBAD promoter and `PLF:4014` the
human EF-1alpha promoter. All three were APPENDED, so no id in this worklist
moved and the 89 signatures and the other 20 proposed rows are byte-identical
either side of the change. They are the last three entries in the table below.

| | |
|---|---|
| Table | 112 rows, of which 89 signed and 23 proposed |
| This worklist | 23 rows: 14 selection markers (Stage 2), 9 Class B conventions (Stage 5) |
| Recommended to sign | 20, of which 3 only after reading a specific paragraph. Two of the three rows added on 2026-08-11 (`PLF:4012`, `PLF:4014`) are *your call* and say why; the third, `PLF:4013`, is **SIGN** |
| WITHDRAWN | 1 (`PLF:4006`), decided by the curator 2026-08-11; the id is retired, not freed |
| Needs a decision that no evidence can make | 3 naming/scope questions and 3 patent flags, listed below |
| Refused, not proposed | 5 Class B elements that were built and then failed the extent-corroboration rule; see *Refused on the evidence* |
| Signatures on file | 89, all still valid, `SIGNOFF.tsv` byte-identical to `main` |

**No digests are printed in this file, on purpose.** `SIGNOFF.tsv` says signing
a digest nobody has read is not an attestation; a worklist that let you copy 23
hashes out of it without opening a single row would be a machine for producing
exactly that. The digests also change the moment any prose in a row changes, so
a copy here would go stale silently. Get them from `--show`, one row at a time,
after reading the row.

Everything in the *Claims*, *Sources*, *Anchor*, *Witnesses* and *Places it at
our extent* lines below is read out of `features/features.tsv` rather than
retyped, so those facts cannot drift from the table. The research, the
citations and the recommendations are prose and are not.

---

## What changed in the tree with this worklist, and why

Nine of these rows carried a sentence in `description` or `notes` that the
research contradicted, could not reach, or pointed at a row that is not in the
table. Those are defects: `SIGNOFF.tsv` defines a signature as a human who
"wrote or checked its description from the primary source", so a description
written from nothing in particular is the thing a signature is supposed to
catch. They are fixed here, **before** any signature, which is the order
`SIGNOFF.tsv` requires because `description` and `notes` are inside the digest.

| Row | What was written | What the evidence says |
|---|---|---|
| `PLF:4006` CMV enhancer | notes sent the reader to "the promoter row above" | that row is `PLF:4005` and it is **not in the table**; it was refused on 2026-08-10 |
| `PLF:4006` | "a 378 nt convention ... a record annotating it was checked here" | the record is not named and not retained; **not re-derivable**, so the claim is removed |
| `PLF:4000` T7 promoter | "a 20 nt convention ... is measured against this row in the witness offsets above" | all four offsets above are `5'+0/3'+0`. **No 20 nt form exists** in anything checked; the real rivals are 19 nt and 21 nt and sit in other rows' records |
| `PLF:4000` | description: "The 17 bp class III promoter" | the row is the −17..−1 part with +1 excluded, which its own caveat says and its description did not |
| `PLF:4007` T7 terminator | "two rival 48 nt forms ... neither is wrong" | the 3' edge **is** primary (Macdonald 1994); only the 5' edge is a convention, and the two rivals are not equally defensible |
| `PLF:4007` | "at least one deposit labels 'T7 terminator' a sequence from a different part of the T7 genome" | no record checked shows this and none was ever named; **removed** |
| `PLF:4008` rrnB T1 | "nothing in the primary source says 'T1'" | **false.** Brosius 1984 and Orosz 1991 both name T1 and T2 and show each terminates alone; Orosz is in `J01695`'s own reference list |
| `PLF:4008` | "rival extents ... run from 43 to 98 nt" | **no rival longer than 44 nt exists** in anything checked; the lone 43 nt feature is one record disagreeing with itself |
| `PLF:1016` bsr | "the description deliberately names NO organism" | resolved from the paper: ***Bacillus cereus* K55-S1**. Written in |
| `PLF:1014` pac | nothing | `M25346.1` is flagged `UNVERIFIED_ORGANISM` by the archive and the row said nothing about it |
| `PLF:1019` LEU2 | only leu2-d was named as a hazard | **every** pRS vector in INSDC carries a LEU2 that differs from this row, and the series does not agree with itself |
| this file | "dropping one does not renumber anything after it" | **not true of an item deleted from a stage's `ITEMS`.** See *If you reject a row* |

Two additions, neither correcting an error: `PLF:4009` and `PLF:1015` now cite
the primary papers behind their names and extents, which they did not before.

---

## One more correction, 2026-08-12: a superlative in `PLF:4009` that was never true

The table above is the 2026-08-11 pass. This one is separate because it is not a
sentence the research overtook — it is a sentence that was **false the day it was
written**. It entered in the commit that created this worklist's rows
(`e918bd6`, 2026-08-10), was not touched by the 2026-08-11 pass, and shipped
unchanged in **v0.6.0, v0.7.0 and v0.8.0**.

| Row | What was written | What the table says |
|---|---|---|
| `PLF:4009` rrnB T2 | notes: "it is the shortest and most sharply bounded of the twelve" | **it is not the shortest and never was.** The twelve were `PLF:4000`–`PLF:4011`; `PLF:4000` and `PLF:4001` are 17 nt each and both are declared *above* this row. `PLF:4012`, appended since, is 19 nt. Measure `reference_nt`: at 28 nt this is the **fourth shortest** of the nine Class B rows in the table |

The bounding half is kept but no longer as a superlative, because it is not one:
every witness of `PLF:4009` that annotates anything over these bases draws
`5'+0/3'+0`, and `PLF:4000` and `PLF:4001` do exactly the same. What actually
separates this row from those two is in the *notes*, not in the offsets — theirs
name rival extents found elsewhere (19 nt and 21 nt for T7, a 19 nt consensus
running through +1 for SP6) and this row's names none. The row now says all of
that, and says that the claim it replaces was wrong from the start rather than
quietly dropping it.

**Nothing else moved.** `PLF:4009` is `proposed` and has no line in
`SIGNOFF.tsv`, so no signature was in a position to lapse; the 89 that exist
still verify, and the only other differences in the rebuilt tables are the build
clock — `date_added` in `features.tsv` and `retrieved` in `provenance.tsv`, both
outside the content digest by construction.

---

## Decide these. No amount of further research will.

Three naming and scope questions, and three patent flags. Each is a judgement
about what this database is for, and the data cannot make it. A fourth,
the CMV question, is **decided** and is kept here as the record of it.

**1. `PLF:4006` -- the CMV question. DECIDED 2026-08-11: WITHDRAWN.**
The choice was: withdraw the enhancer, restore the promoter row, or re-cut both
into one 584 nt row. The curator withdrew it. Behind it sat the posture
question -- **does this project accept SnapGene-shaped corroboration for a Class
B extent at all?** -- and note what withdrawing one row does and does not do to
that: every submission that draws the 380/204 split is SnapGene-shaped and every
submission that is not annotates one element and calls it a promoter, and both
of those sentences are as true today as they were before. **The posture question
is not answered by this decision and remains open.** Full case, and what the
withdrawal did and did not settle, in the row below.

**2. `PLF:1022` TK -- two decisions, both yours.**
(a) *The lookup gap.* The name is UniProt's gene symbol and here that is two
letters. `HSV-TK`, `HSVtk` and `UL23` -- the spellings on real maps -- are not
aliases and will not resolve. Adding them means writing names **ourselves**
into a column sourced entirely from UniProt under CC BY.
(b) *The strain.* A second reviewed 376 aa HSV thymidine kinase exists
(`Q9QNF7`) and differs at four residues: N23S, K36E, R89Q, A265T, measured. A
construct built from it is not corrupt.

**3. The vernacular `DHFR` resolves to neither `PLF:1017` nor `PLF:1023`.**
Measured, and it is not an alias collision: UniProt calls one "Dihydrofolate
reductase type 1" and the other "Dihydrofolate reductase", so each string
resolves to exactly one record and the bare word resolves to none. Do we write
a disambiguating alias ourselves, or leave the gap and let a map labelled
`DHFR` match nothing?

**4. `PLF:1027` rpsL -- `strA` names two genes with opposite phenotypes.**
UniProt lists `strA` as a synonym of `rpsL`, which confers streptomycin
**sensitivity**. `StrA` is also the *name* of `PLF:0023`, a signed row for an
aminoglycoside phosphotransferase conferring **resistance**. Both usages are
real. A caller resolving the alias gets one of two opposite genes.

**Patent: flagged, not adjudicated.** No patent database was searched
(SOURCING.md Risk 6). `patent_flag = 1` on **`PLF:1022` TK**, **`PLF:1025`
bar**, **`PLF:1026` pat**.

---

## The 23 rows at a glance

The withdrawn row is listed first and struck from the count: it is not one of
the 23, it is not in the table, and it cannot be signed. It stays on the page
because a decision with no record of itself is how a database forgets why it
looks the way it does.

| Row | | Recommendation | The one line that decides it |
|---|---|---|---|
| ~~`PLF:4006`~~ | CMV enhancer | **WITHDRAWN 2026-08-11** -- not in the table, not signable | Its own note forbade the state the table is in; the split is drawn only by SnapGene-shaped records; Boshart's −524..−118 straddles it |
| `PLF:4000` | T7 promoter | **SIGN** | 7 of 7 re-derived, and all 17 annotated T7 promoters agree from −17 inward and not before it |
| `PLF:4001` | SP6 promoter | **SIGN** | Not an analogy after all: the anchor's own paper publishes `KAWTTARGKGACACTATAG`, whose −17..−1 is this row exactly |
| `PLF:4007` | T7 terminator | **SIGN** | The 3' base is where Macdonald 1994 says termination happens and where the anchor annotates Tphi |
| `PLF:4008` | rrnB T1 | **SIGN** | Five records place it at exactly 44 nt and no rival extent exists; T1 is named by two primary papers |
| `PLF:4009` | rrnB T2 | **SIGN** | No competing extent, confirmed; the 253 nt "rival" is a synthetic composite |
| `PLF:4010` | bGH poly(A) | **SIGN** | The anchor locates the cleavage site at 2439; the element Goodwin & Rottman require sits inside this row with 84 nt to spare |
| `PLF:1014` | pac | **SIGN, read the flag** | The pinned record is `UNVERIFIED_ORGANISM`; the organism and the 600 nt ORF both come from the 1989 paper instead |
| `PLF:1015` | bsd | **SIGN** | Kimura 1994 reports "an open reading frame of 393 bp, encoding a polypeptide of 130 amino acids" -- this row |
| `PLF:1016` | bsr | **SIGN -- blocker cleared** | The paper says the gene is from *Bacillus cereus* K55-S1 and TK121 is the *E. coli* host |
| `PLF:1017` | dhfrI | **SIGN** (see decision 3) | A second INSDC record, `X17477`, is nucleotide-identical to the pin |
| `PLF:1018` | URA3 | **SIGN** | Confirmed multi-allele trap: A160S in 4 of 9 cross-references and in all five pRS URA3 vectors |
| `PLF:1019` | LEU2 | **SIGN, read the new paragraph** | Not uncontested: every pRS LEU2 differs from this row, and the series disagrees with itself |
| `PLF:1020` | HIS3 | **SIGN** | The 219 aa classic clone is a 3 nt insertion away, located exactly; all four pRS HIS3 vectors carry the 219 aa form |
| `PLF:1021` | TRP1 | **SIGN** | The one pRS marker whose vector copies match this row exactly, all four of them |
| `PLF:1022` | TK | **your call ×2** | See decision 2; patent flagged |
| `PLF:1023` | Dhfr | **SIGN** (see decision 3) | cDNA not genomic join, and the protein cannot tell them apart |
| `PLF:1024` | gpt | **SIGN** | Two different genes are written `gpt`; this is the bacterial one and the row says so |
| `PLF:1025` | bar | **SIGN** | The one-base boundary is confirmed: the native-locus record differs at position 1 only, A→G, M1V |
| `PLF:1026` | pat | **SIGN** | A second INSDC record, `M22827`, is nucleotide-identical to the pin |
| `PLF:1027` | rpsL | **SIGN** (see decision 4) | Not a resistance gene, and the row says the alias says the opposite |
| `PLF:4012` | T3 promoter | **your call** | Five independent addresses draw these 19 edges; the 17 nt extent that would match `PLF:4000`'s −17..−1 rule has one, so this row and its two siblings use different conventions on purpose |
| `PLF:4013` | araBAD promoter | **SIGN** | Five independent addresses place it edge for edge, verbatim in `J01641.1`, the 1978 araBAD regulatory-region record -- and 284/285 against K-12 MG1655, which the row states |
| `PLF:4014` | EF-1alpha promoter (human) | **your call** | At the floor with two placements; the 1179 nt *vector* form has three and is a different sequence -- the row explains which it took and what that costs |

---

## How to sign

From `SIGNOFF.tsv`'s own HOW TO SIGN section. Step 1 is *read the row*, and
there is a command for that -- it prints every column the signature covers,
unescaped, plus the provenance quads and the resulting digest, and it writes
nothing:

```
python features/build/build.py --show PLF:4000,PLF:4001,PLF:4007,PLF:4008,PLF:4009,PLF:4010,PLF:4012,PLF:4013,PLF:4014
python features/build/build.py --show PLF:1014,PLF:1015,PLF:1016,PLF:1017,PLF:1018,PLF:1019,PLF:1020,PLF:1021,PLF:1022,PLF:1023,PLF:1024,PLF:1025,PLF:1026,PLF:1027
```

Then take the 64 hex characters from that output -- or run
`python features/build/build.py --print-digests` for the whole table -- and add
one line per row to `SIGNOFF.tsv`: record id, `reviewed` or `verified`, your
name, the date, the digest, and a note saying what you actually checked.

Note the order of work `SIGNOFF.tsv` records: **change prose first, then sign.**
`description` and `notes` are inside the digest, so rewriting them after signing
lapses the signature. Every prose change this worklist recommends has already
been made; anything you change further re-runs that rule.

---

## What has been checked mechanically, and what that does not prove

Re-derived by checkers sharing no code with the build -- their own fetches,
their own coordinate arithmetic, their own codon tables, their own EMBL parser:

- **Every row re-derived exactly.** Every Class B slice was re-cut from a window
  padded either side and required to land at exactly that offset, so the
  coordinates were tested rather than handed to the server. Every marker CDS was
  re-translated and matched its shipped protein residue for residue.
- **All 14 marker proteins are the enzyme the row names**, confirmed against the
  UniProt canonical sequence and its recommended name.
- **The T7 boundary argument holds and is stronger than the row claimed**: all 7
  copies of the 17-mer end one base before an annotated promoter point and the
  next base is G in all 7; and across all 17 T7 promoters the anchor annotates,
  every column from −17 to −3 agrees in 14 of 17 records or better while no
  column from −24 to −18 exceeds 12 of 17.
- **The CMV split is arithmetic**: 204 + 380 = 584, contiguous, nothing over.
  That says the two intervals abut. It does not say the split is where the
  community puts it -- and the evidence assembled below says it is not.

None of that is a substitute for signing. It proves the bases are the bases the
accession holds. It says nothing about whether the *boundary* is the one the
community means by the name, which is the whole of what Class B is.

**Literature.** Every paper cited below was checked against PubMed for title,
journal, year and, where PubMed indexes one, the sentence quoted from its
abstract; the SP6 consensus was read from the open-access full text in PMC.
Two papers carry no PubMed abstract and are marked where they are used:
Dunn & Studier 1983 (which is therefore **not** relied on for any claim here)
and Kobayashi et al. 1991 (read from the publisher's full text, which is the
only route to it).

---

# The 9 Class B rows (Stage 5, `features/build/stage_classb.py`)

*Ten sections follow, because `PLF:4006` is kept below as the record of a
decision. Nine of them are rows you can sign; the first is not in the table.
None of the nine is signed today, so none of the nine is searched by anything.*

SOURCING.md section 3 classes promoter, terminator and poly(A) boundaries as
**conventions, not facts** -- there is no database that says where 'the CMV
promoter' ends -- and section 6 prescribes human curation plus at least two
independent GenBank exemplars each, and section 4 says what for: "showing where
depositors actually place it". Each row is a coordinate slice of one anchor
record, re-fetched and re-sliced every build, with at least two further records
**from different submitting addresses** carrying the exact bases, and at least
two independent submissions annotating a feature at **exactly** the shipped
extent.

Three things to know while reading these:

- **Witness counts are floors.** Submitting addresses are merged fuzzily when
  they look like one lab writing its address two ways, and the stage counts only
  the records a row names as exemplars.
- **SnapGene-annotated deposits are refused as witnesses**, because INSDC
  carries them and the CI taint gate structurally cannot see one -- it compares
  descriptions, never coordinates. That screen catches only the deposits that
  kept the `label:` tell; `SOURCING.md` §0.6 records that no detector can do
  better, and what is enforced instead. **`PLF:4006` is what that blind spot
  looks like when it matters** -- and its withdrawal on 2026-08-11 removed that
  instance and left the blind spot exactly as wide as it was.
- **Holding the bases and drawing the same edges are different claims**, and the
  second is the one `consensus_of_insdc` rests on.

---

## `PLF:4006` -- CMV enhancer  ·  **WITHDRAWN 2026-08-11 by Lior Lobel**

**This row is no longer in the table.** The recommendation below was *withdraw,
and it is your call*; the curator made that call on 2026-08-11, for the reasons
recorded in `stage_classb.ITEMS` beside the declaration itself. Everything under
this heading is kept as the record of the decision and is written in the tense it
was written in -- it is what was read, not what is shipped.

Two things the withdrawal did **not** do. It did not free the id: `PLF:4006` is
retired, its declaration stays in `stage_classb.ITEMS` at index 6, and
`stage_classb.self_test()` pins the five ids that a deletion would have moved.
And it did not answer the posture question about SnapGene-shaped corroboration,
which is still open and still applies to every Class B row this project builds.

`python features/build/build.py --show PLF:4006` now prints *no such row in this
build*, which is the correct answer and the reason the invocation at the foot of
this section is no longer offered.

- **Claims**: The immediate-early enhancer of human cytomegalovirus: a tandem array of repeated binding sites for host transcription factors, directly upstream of and contiguous with the immediate-early promoter. It is the part that supplies the strength, and it works in most mammalian cell types, which is why it travels with the promoter into vectors.
- **Anchor**: `X17403.1:173949-174328:-`  (380 nt, `enhancer`, `consensus_of_insdc`)
- **Check against**: LC897329.1, OP697991.1  ·  EXCLUDED as SnapGene-annotated: MH325107.1
- **Witnesses**: 3 independent submitting addresses over 3 records
- **Places it at our extent**: 2 of 3 -- LC897329.1, OP697991.1
- **Anchor's own annotation within 80 nt**: none

### Why this is a bug and not a question

The row's `notes` sent the reader to "the promoter row above" for why the block
is split in two and why the two ship together. That row is `PLF:4005`, and it is
not in the table -- the extent-corroboration rule refused it on 2026-08-10. Its
caveat, still in `stage_classb.ITEMS`, ends: *"Ship this row and the enhancer
together or not at all; shipping one alone would silently pick a convention."*
**The table is in the state that sentence forbids.**

The hazard is live, not theoretical. A pcDNA3-type CMV region is one 584 nt
block. With only the enhancer, the upstream 380 nt light up and the 204 nt
carrying the TATA box -- the part most maps call the CMV promoter -- stay dark.
`Db::absent_common_kinds` does not rescue it: that probe looks for the literal
key `promoter`, and `PLF:4000` and `PLF:4001` supply it, so the app will not say
"no promoter is in this database yet". Half-populated is worse than empty here.

### The evidence says something stronger than the note did

Every record this stage has fetched that contains the 380 bases, with what it
annotates over them:

| record | submitting address | annotates |
|---|---|---|
| LC897329.1 | NCNP, Japan | 380 nt "CMV enhancer" **+** 204 nt "CMV promoter" |
| OP697991.1 | U. Delaware | 380 nt "CMV enhancer" **+** 204 nt "CMV promoter" |
| MH325107.1 | MIT -- carries the `label:` tell | the same split |
| OR659033.1 | CNRS Montpellier | **one 584 nt feature, "CMV promoter"** |
| MN224159.1 | MPI Brain Research | **one 655 nt feature, "CMV promoter"** |
| MW987522.1 | Baylor | **one 623 nt feature, "CMV-tetO2 promoter"** |
| X17403.1 | the anchor | no regulatory feature within 80 nt |

The three that draw the split are exactly the three the project has already
identified as SnapGene-shaped. `MH325107.1` is flagged by the screen.
`LC897329.1` is SnapGene Common Feature naming from top to bottom with no tell,
and is named as such in *Refused on the evidence* below. `OP697991.1` is the
sharper case and SOURCING.md §0.6 uses it as its worked example of a
demonstrated false negative: over this very interval its `/note` reads
`human cytomegalovirus immediate early enhancer; CMV enhancer` where
`MH325107.1`'s reads `...; label: CMV enhancer` -- the two differ by the token
`label: ` and by nothing else. Reproduced here on 2026-08-11.

**Every submission in the project's own evidence that is not SnapGene-shaped
annotates this region as one element, and calls it a promoter. None of them
annotates an enhancer.**

### The primary literature does not draw this edge either

According to PubMed, Boshart M, Weber F, Jahn G, Dorsch-Häsler K, Fleckenstein
B, Schaffner W (1985) *A very strong enhancer is located upstream of an
immediate early gene of human cytomegalovirus*, Cell 41:521-530, PMID 2985280,
[DOI](https://doi.org/10.1016/s0092-8674(85)80025-8), locates the enhancer
"between nucleotides -118 and -524" of the major immediate-early transcription
start.

Placed on the anchor's own numbering -- `X17403.1` annotates
`exon complement(173610..173730)`, so the record puts +1 at 173730 -- this row
is −219..−598 and the refused promoter row was −15..−218. On the other common
convention (+1 taken as the base after `…TATATAAGCAGAGCT`, 173744) they are
−205..−584 and −1..−204. **Either way Boshart's −118 falls inside what this
database calls the promoter and −524 falls inside what it calls the enhancer.**
Neither of this row's edges is a Boshart edge. The split is a fragment boundary.

Supporting, on the repeat structure the description invokes: Thomsen DR,
Stenberg RM, Goins WF, Stinski MF (1984) PNAS 81:659-663, PMID 6322160,
[DOI](https://doi.org/10.1073/pnas.81.3.659).

### Your three options

**(A) Restore `PLF:4005` and ship the pair.** `OP697991.1` annotates
`regulatory 425..628` = exactly the 204 nt promoter, from an address independent
of `LC897329.1`'s, and it is *already* in `provenance.tsv` for this row -- so
adding it to `PLF:4005`'s exemplars meets `MIN_PLACEMENTS` and the row returns on
the next build. This is not searching until the check goes green.
*Cost:* both of the promoter's corroborating submissions would then be
SnapGene-shaped, one of them SOURCING.md §0.6's own worked example. It passes in
the letter and defeats the purpose in the spirit, and `notes` would have to say
so in those words.

**(B) Re-cut to one 584 nt row**, `genbank_key = promoter` so
`absent_common_kinds` and the app behave. This is what the non-SnapGene-shaped
depositors annotate and what maps mean. **Blocked today at one exact placement
(`OR659033.1`); it needs a second independent submission annotating 584 nt edge
for edge.** That is a bounded ENA survey and it was the only new fetching any of
the seven Class B rows needed. It is still the survey option B waits on, and
withdrawing this row did not perform it.

**(C) Withdraw.** What this file recommended, and **what the curator chose on
2026-08-11**. See *If you withdraw a row* below for the mechanism -- which is
**not** what this file used to say it was, and which is now code with a test
rather than a paragraph. Note one correction to the option as it was written:
withdrawal is not a move to `HELD`. `HELD` is for elements that were never
issued an id; a published row keeps its declaration in `ITEMS`, and its id, and
gains a `withdrawn` reason.

The decision turned on the posture question the data cannot answer: whether
SnapGene-shaped corroboration counts for a Class B extent. Option A said yes and
said so out loud. Option B owed nothing to SnapGene and cost one survey.
Option C shipped nothing false — and it is worth being exact about what choosing
it means: **it removes the instance, not the question.** A and B are still the
two ways to put a CMV region back into this database, and neither has been done.

---

## `PLF:4000` -- T7 promoter  ·  **RECOMMENDATION: SIGN**

- **Claims**: The 17 bp recognition element of the class III promoter of bacteriophage T7: the seventeen bases immediately upstream of a T7 transcription start, with the +1 base excluded. T7 RNA polymerase reads it as a single subunit with no sigma factor and no accessory protein, and the host polymerase cannot read it at all. That mutual blindness is what a T7 expression system is built on.
- **Anchor**: `V01146.1:22887-22903:+`  (17 nt, `promoter`, `consensus_of_insdc`)
- **Check against**: AF053733.1, KJ641600.1, PV764404.1, GQ421427.1
- **Witnesses**: 5 independent submitting addresses over 5 records
- **Places it at our extent**: 4 of 5
- **Anchor's own annotation within 80 nt**: regulatory as a POINT at 22904, 1 base 3' of this interval

**What settles the 3' edge.** The anchor does, mechanically, without anyone
asserting anything: the seventeen bases occur seven times in the T7 genome,
every one of the seven ends exactly one base before a coordinate the record
annotates as a promoter, and in all seven the next base is G. Re-derived
independently on 2026-08-11: **7 of 7**.

**What settles the 5' edge, and is new.** The anchor annotates **seventeen** T7
promoters, not seven. Aligned on their annotated points, every column from −17
to −3 agrees in 14 of 17 records or better -- nine of those columns in 17 of 17
-- and **no column from −24 to −18 exceeds 12 of 17**. The conserved block and
this row have the same 5' edge. This is a better argument than the row used to
make and it is now in the row.

**The defect that was fixed.** The note claimed a 20 nt convention "is measured
against this row in the witness offsets above". All four offsets above are
`5'+0/3'+0`; there was no rival in them. Re-measured, the rivals that exist are
**19 nt** (−17..+2, `MH325107.1`, which the screen flags) and **21 nt**
(−17..+4 in `DQ250998.1` and `FJ457001.1`; −18..+3 in `AY640625.1`) -- **no 20
nt form anywhere.** All of them run into the transcript. Those records are
exemplars of *other* rows, so this row's offsets structurally cannot show them,
and the note now names them instead of gesturing at them.

**What is still open, and it is prose not sequence.** The primary description of
the class III promoter is Dunn JJ, Studier FW (1983) J Mol Biol 166:477-535,
PMID 6864790, [DOI](https://doi.org/10.1016/s0022-2836(83)80282-4) -- and
according to PubMed that record carries no abstract. The element is commonly
given as 23 bp, −17..+6, in secondary sources; **that was not read from the
paper here and nothing in this row depends on it.** The description now says the
row is the 17 bp recognition element with +1 excluded, which is true whatever
the paper's figure turns out to be. If you want the row to *state* the 23 bp
consensus, read the paper first -- that is the one claim in this worklist that
could not be closed from a record or an abstract.

```
python features/build/build.py --show PLF:4000
```

---

## `PLF:4001` -- SP6 promoter  ·  **RECOMMENDATION: SIGN**

- **Claims**: The 17 bp promoter of Salmonella phage SP6, read by SP6 RNA polymerase and by no host enzyme. Bounded on the same rule as the T7 row, and by SP6's own published promoter consensus: the seventeen bases upstream of the transcription start. Paired with a T7 or T3 promoter at the other end of a polylinker it is what lets a vector transcribe either strand of an insert in vitro.
- **Anchor**: `AY288927.2:12542-12558:+`  (17 nt, `promoter`, `consensus_of_insdc`)
- **Check against**: DQ250998.1, FJ457001.1, KC800697.1
- **Witnesses**: 3 independent submitting addresses over 4 records
- **Places it at our extent**: 2 of 3 -- DQ250998.1 + FJ457001.1 (one address), KC800697.1
- **Anchor's own annotation within 80 nt**: none

**This row was described as bounded by analogy to T7. It is not, and the source
that settles it is the anchor record's own publication.** According to PubMed,
Dobbins AT, George M Jr, Basham DA, Ford ME, Houtz JM, Pedulla ML, Lawrence JG,
Hatfull GF, Hendrix RW (2004) *Complete genomic sequence of the virulent
Salmonella bacteriophage SP6*, J Bacteriol 186:1933-1944, PMID 15028677,
[DOI](https://doi.org/10.1128/JB.186.7.1933-1944.2004) -- which is `AY288927`'s
own reference -- reports that "Sequence analysis identified 10 putative
promoters for the SP6-encoded RNA polymerase". Its open-access full text
(PMC374404) gives the degenerate consensus **`KAWTTARGKGACACTATAG`**, 19 nt,
i.e. −18..+1.

Resolved on this genome (K=T, W=T, R=G) that string is `TATTTAGGTGACACTATAG`,
and **its −17..−1 window is `ATTTAGGTGACACTATA` -- this row, base for base.**
Checked on 2026-08-11. The one base further 5' that the consensus carries is the
degenerate `K`, and the three exact copies of this row in the anchor itself
carry **G, T and G** there, so −18 is the first column that varies. All three
are followed by **G** at +1.

The register is fixed independently by Brown JE, Klement JF, McAllister WT
(1986) *Sequences of three promoters for the bacteriophage SP6 RNA polymerase*,
Nucleic Acids Res 14:3521-3526, PMID 3010240,
[DOI](https://doi.org/10.1093/nar/14.8.3521): the core these phage promoters
share, `CACTA`, runs "from -7 to -3". Read as −17..−1, this row puts `CACTA` at
exactly −7..−3.

So the boundary rests on the same articulated rule the T7 row uses -- the
conserved window lying entirely upstream of +1 -- applied to primary evidence
about SP6. **The honest residue:** both primary consensuses extend through +1,
so 17 is still a choice. It is the same choice, made the same way, as T7.

**One free strengthening if you want it:** `LT726933.1` places `SP6 promoter` at
exactly 17 nt and is not among this row's exemplars. Adding it would take the
count to 3 independent submissions placing it exactly. That is a prose-and-
exemplar change, so it goes before the signature, not after.

```
python features/build/build.py --show PLF:4001
```

---

## `PLF:4007` -- T7 terminator  ·  **RECOMMENDATION: SIGN**

- **Claims**: The Tphi transcription terminator of bacteriophage T7: a GC-rich hairpin followed by a run of thymines, which together stall T7 RNA polymerase. Placed downstream of the insert in T7 expression vectors so that transcription stops instead of running on round the plasmid.
- **Anchor**: `V01146.1:24164-24210:+`  (47 nt, `terminator`, `consensus_of_insdc`)
- **Check against**: AF525444.1, PV764404.1, KJ641600.1
- **Witnesses**: 4 independent submitting addresses over 4 records
- **Places it at our extent**: 2 of 4 -- AF525444.1, PV764404.1
- **Anchor's own annotation within 80 nt**: regulatory as a POINT at 24210, inside this interval

**The primary source picks an edge, and it is this row's.** According to PubMed,
Macdonald LE, Durbin RK, Dunn JJ, McAllister WT (1994) *Characterization of two
types of termination signal for bacteriophage T7 RNA polymerase*, J Mol Biol
238:145-158, PMID 8158645, [DOI](https://doi.org/10.1006/jmbi.1994.1277):
"termination occurs at a 3' G residue just downstream of the U run". Measured in
the anchor, 24201-24215 reads `GGGTTTTTTGCTGAA` -- six T's, then the G at
**24210**, which is this row's last base and the single coordinate the record
annotates as "T7 transcription terminator Tphi". The primary definition and the
depositor's annotation land on the same base.

The same paper says the 5' side is *not* delimited: "sequences upstream from the
terminator have marked effects on the position and efficiency of termination".
**That is the one arbitrary edge here, and the row now says so.**

**The rivals, re-measured, are not symmetric** -- which is what the old note's
"neither is wrong" got wrong:

| form | in `V01146` terms | 5' edge | 3' edge | deposit |
|---|---|---|---|---|
| **this row, 47 nt** | 24164-24210 | convention | **the Tphi termination G** | AF525444.1, PV764404.1 (2 independent, exact) |
| rival A, 48 nt | 24163-24210 | **first base after the gene 10 stop** (`TAA` at 24160-24162, measured) | the same G | GQ421427.1 |
| rival B, 48 nt | 24162-24209 | the *last base of* the stop codon | **one base short of the termination site** | AY303670.1 |

A has a principled 5' edge and the primary 3' edge; B has neither. Also present:
`KJ641600.1` at 62 nt, same 3' edge; and `KM261834.1`'s 253 nt "terminator",
which is not a rival extent but a **synthetic composite** -- measured here, it
contains this row verbatim at its offset 2 and a one-base variant of the rrnB T1
row at its offset 60, and it does **not** contain rrnB T2.

**If you prefer 48 nt** on the "start at the base after the CDS" rule, that is
defensible and articulable -- and it has exactly one exact placement today,
below this stage's floor of two. That is an argument for keeping 47.

**Removed:** "at least one deposit labels 'T7 terminator' a sequence from an
entirely different part of the T7 genome." No record checked shows it and none
was ever named.

```
python features/build/build.py --show PLF:4007
```

---

## `PLF:4008` -- rrnB T1 terminator  ·  **RECOMMENDATION: SIGN**

- **Claims**: The first of the two tandem rho-independent terminators at the end of the Escherichia coli rrnB ribosomal RNA operon: a GC-rich stem-loop followed by a thymine run. Used downstream of a cloned gene to stop transcription, and upstream of a promoter to insulate it from read-through from the vector.
- **Anchor**: `J01695.2:6369-6412:+`  (44 nt, `terminator`, `consensus_of_insdc`)
- **Check against**: DQ115377.1, EF216319.1, U13872.1
- **Witnesses**: 3 independent submitting addresses over 4 records
- **Places it at our extent**: 2 of 3 -- DQ115377.1 + EF216319.1 (one address), U13872.1
- **Anchor's own annotation within 80 nt**: none

**"Nothing primary says T1" was false, and the truth is better for the row.**
According to PubMed, Brosius J (1984) *Toxicity of an overproduced foreign gene
product in Escherichia coli and its use in plasmid vectors for the selection of
transcription terminators*, Gene 27:161-172, PMID 6202587,
[DOI](https://doi.org/10.1016/0378-1119(84)90137-9) -- by the author of this
operon's sequence and of the pKK vectors these extents come from -- reports that
"the putative rrnB terminators, T1 and T2, each function separately in vivo".
And Orosz A, Boros I, Venetianer P (1991) *Analysis of the complex transcription
termination region of the Escherichia coli rrnB gene*, Eur J Biochem
201:653-659, PMID 1718749,
[DOI](https://doi.org/10.1111/j.1432-1033.1991.tb16326.x) subcloned "the
terminators T1 and T2 ... individually" and concluded "T1 and T2 are both
efficient terminators in isolated forms".

**Orosz et al. is in `J01695`'s own reference list**, verified in the record
itself. So the primary source that supplies the coordinates also cites the paper
that names the element and shows it terminates alone. That is direct support for
shipping T1 and T2 as two rows.

The narrower true claim, which the row now makes: `J01695`'s **feature table**
annotates no terminator -- the only feature between 5900 and 7200 is
`rRNA 6246..6365`. Rfam still cannot help; SOURCING.md records as a confirmed
negative that Rfam does not model standalone rho-independent terminators.

**The "43 to 98 nt" rivals do not survive re-measurement.** Across every record
this stage fetches that contains these bases:

| record | address | places it |
|---|---|---|
| DQ115377.1 | U. Cape Town | **44 nt exact** ("rrnBR1") |
| EF216319.1 | U. Cape Town (same lab) | **44 nt exact** ("rrnBT1") |
| U13872.1 | Pharmacia Biotech | **44 nt exact** ("rrnB") |
| U13859.1 | Pharmacia Biotech | **44 nt exact** twice, **and 43 nt once** |
| LT727425.1 | BCCM/LMBP Gent | **44 nt exact** ("T1T") |
| J01695.2 | the anchor | nothing |

**No rival longer than 44 nt exists at all**, and the 98 is not re-derivable
from anything in the repository. The only 43 nt feature is `U13859.1`
disagreeing with itself: it carries these bases three times and annotates all
three `rrnB T1`, twice at 44 nt and once at 43, the 43 being this row without
its leading `A`. That is a slip inside one submission, not a convention.

**One free strengthening:** `LT727425.1` is an exact placement from an address
this row does not cite. Adding it raises the corroboration count honestly.

```
python features/build/build.py --show PLF:4008
```

---

## `PLF:4009` -- rrnB T2 terminator  ·  **RECOMMENDATION: SIGN**

- **Claims**: The second of the two tandem terminators of the Escherichia coli rrnB operon, downstream of T1. Vectors that carry 'rrnB T1T2' carry both with the natural spacer between them; this row is T2 alone.
- **Anchor**: `J01695.2:6544-6571:+`  (28 nt, `terminator`, `consensus_of_insdc`)
- **Check against**: LT739213.1, U13859.1, U13872.1
- **Witnesses**: 3 independent submitting addresses over 4 records
- **Places it at our extent**: 2 of 3 -- LT739213.1, U13859.1 + U13872.1 (one address)
- **Anchor's own annotation within 80 nt**: none

The cleanest row in the stage, and re-measurement agrees with it: every record
that annotates T2 separately encloses exactly these 28 bases -- `LT727425.1`
("T2T"), `LT739213.1` ("T2 terminator"), `U13859.1` ("rrnB T2"), `U13872.1`
("rrnB"). **No competing extent, and none of the intra-record inconsistency T1
has.**

The one overlapping rival is `KM261834.1`'s 253 nt `regulatory` at 5481-5733,
and it is exactly the third element the caveat predicts: measured here it opens
with the `PLF:4007` T7 Tphi row verbatim, carries a one-base variant of the
`PLF:4008` T1 row, and then **stops nine bases into these 28** rather than
enclosing them. A synthetic tandem terminator, not a rival T2 extent.

**Added, not corrected:** the row now cites Brosius 1984 and Orosz 1991 (above),
so the *name* rests on the primary literature and only the *extent* rests on the
vector records. Before 2026-08-11 both rested on the vector records.

**Corrected 2026-08-12, and it is the one thing on this page to read before
signing this row:** the note used to end "it is the shortest and most sharply
bounded of the twelve". It is not the shortest of the twelve, of the nine in the
table, or of the fifteen this stage declares — `PLF:4000` and `PLF:4001` are
17 nt against this row's 28 and were both declared before it, so the claim was
false when written, not overtaken. The note now says so, states
the bounding as the measurement it is rather than as a ranking, and points at
`reference_nt` so the count can be re-derived. `notes` is inside the digest, so
this changed this row's digest — which cost nothing, because the row is
`proposed` and unsigned.

```
python features/build/build.py --show PLF:4009
```

---

## `PLF:4010` -- bGH poly(A) signal  ·  **RECOMMENDATION: SIGN**

- **Claims**: The polyadenylation signal of the bovine growth hormone gene: the AATAAA hexamer together with enough flanking sequence to include the downstream GT-rich element, which cleavage and polyadenylation need as much as the hexamer itself. The standard 3' element of mammalian expression vectors.
- **Anchor**: `M57764.1:2326-2550:+`  (225 nt, `polyA_signal`, `consensus_of_insdc`)
- **Check against**: LC897329.1, MN224159.1, OR659033.1  ·  EXCLUDED as SnapGene-annotated: MN811118.1
- **Witnesses**: 4 independent submitting addresses over 4 records
- **Places it at our extent**: 3 of 4
- **Anchor's own annotation within 80 nt**: none

The best-supported row in the stage, and the primary evidence now backs the
*length* rather than the row's judgement standing alone.

**The anchor locates the cleavage site, which turns "enough flanking sequence"
into a measurement.** `AATAAA` occurs exactly once in these 225 bases, at row
position 91 = `M57764.1` 2416-2421. The record's own `exon 2138..2439` puts the
cleavage and polyadenylation site at **2439 -- eighteen bases after the hexamer
ends**, the textbook spacing. This row runs 111 further bases past it. All
re-derived here.

According to PubMed, Goodwin EC, Rottman FM (1992) *The 3'-flanking sequence of
the bovine growth hormone gene contains novel elements required for efficient
and accurate polyadenylation*, J Biol Chem 267:16330-16334, PMID 1644817 (no DOI
indexed), report that "a region from 18 to 27 nucleotides downstream of the
cleavage site contains sequences required for correctly positioning the cleavage
site" -- that is 2457-2466, **inside this row with 84 bases to spare** -- and
that the data "are consistent with a diffuse efficiency element in the bGH
polyadenylation signal rather than a discrete element". A diffuse element is
exactly why there is no sharp 3' edge to find, and why a short row would be
actively wrong.

The anchor's own publication, which the row did not cite before 2026-08-11, is
Gordon DF, Quick DP, Erwin CR, Donelson JE, Maurer RA (1983) *Nucleotide
sequence of the bovine growth hormone chromosomal gene*, Mol Cell Endocrinol
33:81-95, PMID 6357899,
[DOI](https://doi.org/10.1016/0303-7207(83)90058-8).

**One free strengthening:** `LT726933.1` places it at exactly 225 nt and is not
among this row's exemplars; that is a fourth independent address.

```
python features/build/build.py --show PLF:4010
```

---

# The 14 selection markers (Stage 2, `features/build/stage_uniprot.py`)

Every row is the CDS the depositor annotated, initiator codon through stop codon
inclusive, accepted only because translating the nucleotides reproduces the
UniProt reference protein exactly. Every row therefore **excludes the promoter**,
which is the cassette-vs-ORF trap named in each row it applies to.

---

## `PLF:4012` -- T3 promoter  ·  **RECOMMENDATION: YOUR CALL.** The evidence is overwhelming; what needs deciding is whether three sibling rows may use two conventions

- **Claims**: The 19 bp T3 promoter convention -- the seventeen bases upstream of a T3 transcription start, plus the first two bases of the transcript.
- **Anchor**: `AJ318471.1:20733-20751:+`  (19 nt, `promoter`, `consensus_of_insdc`) -- the T3 genome, and this is the phi10 copy, immediately upstream of the gene 10A major-capsid CDS, exactly as `PLF:4000` takes T7's phi10.
- **Check against**: PV959484.1, PQ463640.1, OK413188.1, PP475160.1, LC795782.1
- **Places it at our extent**: 5 of 6 -- every exemplar; the anchor draws 23 nt (`5'+0/3'+4`)

**Why it was held, and why that was wrong.** The old reason said the two leading
conventions were 17 nt and 19 nt, offset rather than nested, one submission each.
Read as a comparison of `AATTAACCCTCACTAAA` with the *shifted*
`ATTAACCCTCACTAAAGGG`, that is exactly right. It was the wrong pair. On a corpus
large enough to see it the two leading conventions are this row's 19 nt (18
independent submissions in the 2026-08-11 survey; 5 in this row's own witness
list) and the 20 nt nested *around* it -- this row plus one G (14 in the survey;
4 verified here -- Montreal, Wisconsin-Madison, CNB-CSIC, Stratagene). Both share
this row's 5' edge. Nested rivals are a case this table already handles.

**The decision that is actually yours.** The extent that would make this row read
like `PLF:4000` and `PLF:4001` -- 17 nt, −17..−1, +1 excluded -- is deposited and
has **one** submitting address behind it: BCCM/LMBP Gent, checked on three of its
records. It fails `MIN_PLACEMENTS`. So either this row goes into the table at 19 nt and the
database holds two conventions for three phage promoters, or T3 stays out
because the extent that matches its siblings cannot clear the floor. The row
takes the first and says so at length; the primary literature (Bailey, Klement &
McAllister 1983, PMID 6574450) is what makes the disagreement measurable rather
than vague -- their conserved 16 bp block runs −12..+4, so this row's last two
bases are transcribed.

```
python features/build/build.py --show PLF:4012
```

---

## `PLF:4013` -- araBAD promoter  ·  **RECOMMENDATION: SIGN**

- **Claims**: The araC-araBAD intergenic regulatory region of *Escherichia coli* in the extent vectors carry as "the pBAD promoter".
- **Anchor**: `J01641.1:1004-1288:+`  (285 nt, `promoter`, `consensus_of_insdc`) -- "E.coli araBAD promoter region and araC gene", the 1978 record the old hold reason said did not exist.
- **Check against**: OR900359.1, PQ381271.1, PQ015303.1, PP457274.1, LC143902.1
- **Places it at our extent**: 5 of 6 -- every exemplar; the anchor annotates four "(approx)" regulatory sites inside the interval and draws none of its edges

**Why it was held, and why that was wrong.** "No Escherichia coli ara locus
record fetched to anchor any of them" -- `J01641` is that record, it is verbatim
over the whole 285 nt, and it carries five primary references including Smith &
Schleif 1978 (PMID 357433) and Greenfield, Boone & Wilcox 1978 (PMID 368797).
"Three extents from three submissions, two of them SnapGene-annotated" is also
retired: five independent, non-SnapGene addresses draw these edges in this row's
own witness list alone.

**Read before signing.** The row does NOT claim its 3' edge is −1 relative to the
araBAD transcription start; nothing fetched establishes that, and the caveat says
so. And the 285 bases are **not** in K-12 MG1655: against `U00096.3` they match
284/285, the difference at genome coordinate 70289, inside the annotated AraC
inducer site. The row cites `J01641` because that is the record the bases are
verbatim in, and states which strain the fragment descends from as unknown.

```
python features/build/build.py --show PLF:4013
```

---

## `PLF:4014` -- EF-1alpha promoter (human)  ·  **RECOMMENDATION: YOUR CALL.** Two forms clear the rule and they are different molecules

- **Claims**: The promoter of human `EEF1A1` with its first exon and entire first intron, ending nine bases into exon 2.
- **Anchor**: `J04617.1:373-1560:+`  (1188 nt, `promoter`, `consensus_of_insdc`) -- Uetsuki, Naito, Nagata & Kaziro 1989, J Biol Chem 264:5791-5798, PMID 2564392.
- **Check against**: LC884827.1, LC904051.1
- **Places it at our extent**: 2 of 3, against a floor of 2 -- no margin

**Why it was held, and what survives.** The old reason said the element "is not a
verbatim slice of anything: a reference taken from the gene will not match real
vectors, and one taken from a vector cannot be cited to the gene by coordinates."
The second half is true and irrelevant -- a vector form does not need to be cited
to the gene, and the tac and trc rows already anchor on construct records. The
first half was answered by measurement: at the default `min_identity` of 0.96 a
four-base deletion over 1188 nt is 99.7% identity, so this row *does* annotate
real pEF vectors. The figures in that reason, 1144 and 1148 nt, are not
reproducible on any record fetched; the two real conventions are 1179 and 1188.
That last point stopped being an arithmetic argument on review: `pl annotate
--include-proposed` was run against all three of the 1179 nt deposits below,
fetched from ENA as FASTA, and each returns this row at **99.7% identity, 100%
coverage** over `1..1179`. The row annotates real pEF vectors; that is now a
measurement on the shipped binary and not a calculation about one.

**The decision that is yours.** The 1179 nt vector form -- this row minus its
first five bases and minus `GCCC` at `J04617.1:686-689` -- has **three**
independent placements (UCSD, Mahidol, Tsinghua) against this row's two. It was
not taken because all three of those sit at base 1 of their own record, so they
corroborate a 3' edge and not a 5' one, while both of this row's placements are
interior annotations where the depositor chose both edges. If you would rather
ship what most pEF vectors actually carry, that is a defensible call and the
caveat gives the exact recipe. Note also that one of the two placements is a
`misc_feature`, and that the four deleted bases are present in the human genome
(`AL603910.6`), so the deletion is the vectors' and not the record's.

```
python features/build/build.py --show PLF:4014
```

---

## `PLF:1016` -- bsr  ·  **RECOMMENDATION: SIGN. The blocker is cleared.**

- **Claims**: Blasticidin S deaminase of the bsr type. Inactivates blasticidin S by the same hydrolytic deamination as the fungal bsd enzyme, from a different protein family. Its origin is bacterial: the gene was cloned out of pBSR8, a plasmid carried by the soil organism Bacillus cereus K55-S1. Widely used as a blasticidin selection marker in mammalian and insect cells.
- **Sources**: UniProt `P33967` -> ENA CDS `S81409.1:182..604`  (423 nt, 140 aa, initiator ATG)

**The conflict was: UniProt `P33967` says *Bacillus cereus*; ENA `S81409` says
`/organism="Escherichia coli" /strain="TK121"`. Both are right about different
things, and the paper says so.** Kobayashi K, Kamakura T, Tanaka T, Yamaguchi I,
Endo T (1991) *Nucleotide sequence of the bsr gene and N-terminal amino acid
sequence of blasticidin S deaminase from blasticidin S resistant Escherichia
coli TK121*, Agric Biol Chem 55(12):3155-3157, PMID 1368770. According to
PubMed this record carries **no abstract**, which is presumably why the question
stayed open; the publisher's full text was read for this worklist. In its own
words:

- p. 3155: the authors "have isolated a blasticidin S (BS) resistant *Bacillus
  cereus* K55-S1" and from it "obtained a plasmid pBSR8 (10.5 kb) that encodes
  BS-deaminase".
- p. 3155: cloning "located the bsr gene in the 1.5-kb EcoRI to BamHI fragment
  of pTK17", whose figure caption says the two line styles show "the DNA region
  derived from pUC19 and pBSR8, respectively".
- p. 3156: "*E. coli* TK121 carrying pTK17 was grown in nutrient broth
  containing 200 mcg/ml of BS".
- its own article footnote: "Inactivation of Blasticidin S by *Bacillus
  cereus*. **Part IV**."

**TK121 is an *E. coli* laboratory transformant carrying pTK17, a pUC19
derivative bearing a fragment of the *B. cereus* K55-S1 plasmid pBSR8.**
`S81409`'s `/organism` records the strain the sequenced DNA was sitting in --
consistent with the record being one NLM staff created from the article, which
its own reference block states.

**Four corroborations, each re-derived from the record itself on 2026-08-11:**

1. **The record's coordinates are numbered in a *Bacillus* paper.** According to
   PubMed, Nawa K, Tanaka T, Kamakura T, Yamaguchi I, Endo T (1998)
   *Inactivation of blasticidin S by Bacillus cereus. VI. Structure and
   comparison of the bsr gene from a blasticidin S-resistant Bacillus cereus*,
   Biol Pharm Bull 21:893-898, PMID 10607416,
   [DOI](https://doi.org/10.1248/bpb.21.893), reports the transcription start as
   "the A located 7 bases downstream from the putative sigmaA promoter (91TTGATC
   and 113TAAAAT)". `S81409` positions 91-96, 113-118 and 125 are `TTGATC`,
   `TAAAAT` and `A`. **All three exact.**
2. **The promoter is a *Bacillus* promoter.** That paper calls them σ^A and σ^B
   promoters; σ^B is the *Bacillus* general-stress sigma factor and *E. coli*
   has none. The 1991 paper's own Shine-Dalgarno citation is to Moran et al.,
   Mol Gen Genet 186:339 (1982) -- the *B. subtilis* promoter/RBS paper.
3. **Base composition.** Measured: this CDS is **37.4 % GC with 25.5 % GC at
   third positions**, and the 181 bases upstream are 24.3 % GC. *E. coli* K-12
   is ~50 % GC; *B. cereus* is ~35 %. This is not an *E. coli* gene.
4. **The record is the paper's figure.** The paper sequenced "the NdeI-HincII
   fragment"; `S81409` bases 1-6 are `CATATG` (NdeI) and 670-675 are `GTTGAC`
   (HincII), and the ORF it reports is "420 base pairs" = 140 codons, this row
   without its stop.

Supporting, all from the same group and all naming *B. cereus*, per PubMed:
Endo et al. 1988 J Antibiot 41:271, PMID 2833485,
[DOI](https://doi.org/10.7164/antibiotics.41.271) (isolation of pBSR8 from
*B. cereus*); Kamakura et al. 1990 Mol Gen Genet 223:332, PMID 2250657,
[DOI](https://doi.org/10.1007/BF00265072) ("discovered in the BS resistant
strain, *Bacillus cereus* K55-S1, and the structural gene, bsr ... has been
cloned"); Izumi et al. 1991 Exp Cell Res 197:229, PMID 1720391,
[DOI](https://doi.org/10.1016/0014-4827(91)90427-v) ("bsr, isolated from
*Bacillus cereus* K55-S1 strain"); Nawa et al. 1995 Biol Pharm Bull 18:350,
PMID 7742811, [DOI](https://doi.org/10.1248/bpb.18.350) (purification of the
enzyme "mediated by a plasmid from blasticidin S resistant *Bacillus cereus*
K55-S1"); and the bsd paper itself, Kimura et al. 1994 Mol Gen Genet 242:121,
PMID 8159161, [DOI](https://doi.org/10.1007/BF00391004) ("bsr, the BS deaminase
gene isolated from *Bacillus cereus*").

**Written into the row**, phrased around the taint gate: the organism is in a
sentence of its own, separated from the enzyme name, because the obvious opening
is a five-token run of exactly the shape SOURCING.md §0.4 hard-fails -- the same
trap `PLF:1015` was already rewritten around. **Do not tidy the two back
together.**

**Not resolved by any of this, and left for you:** `S81409` remains an
S-prefixed record created from the article rather than a depositor submission,
and `P33967` has exactly **one** EMBL cross-reference, so there is no second
INSDC record for this sequence to fall back on. A search for a
*Bacillus*-attributed bsr found none.

```
python features/build/build.py --show PLF:1016
```

---

## `PLF:1014` -- pac  ·  **RECOMMENDATION: SIGN, after reading the flag**

- **Claims**: Puromycin N-acetyltransferase of Streptomyces alboniger. Transfers an acetyl group from acetyl-CoA onto the free amino group of the tyrosinyl moiety of puromycin. Puromycin is an aminonucleoside that mimics the 3' end of an aminoacyl-tRNA and terminates the growing peptide chain; acetylation abolishes the mimicry. The standard dominant selection marker for mammalian cell culture, where killing is fast and selection is usually complete within a few days.
- **Sources**: UniProt `P13249` -> ENA CDS `M25346.1:254..853`  (600 nt, 199 aa, initiator ATG)

**The pinned record is flagged by the archive, and until 2026-08-11 nothing in
the row said so.** `M25346.1` carries, verbatim:

```
DE   UNVERIFIED: Streptomyces alboniger puromycin N-acetyltransferase (pac)
DE   gene, complete cds.
KW   puromycin N-acetyltransferase; UNVERIFIED_ORGANISM.
CC   GenBank staff is unable to verify source organism and sequence
CC   and/or annotation provided by the submitter.
```

This is the same *class* of problem as `PLF:1016` -- an organism attribution
resting on a record that will not carry it -- except that here the archive said
so explicitly and the row was silent. `P13249` has exactly **one** EMBL
cross-reference, so there is no alternative INSDC record inside UniProt.

**It is not the same severity, because the flag is discharged by the primary
literature and by the row's own construction.** According to PubMed, Lacalle RA,
Pulido D, Vara J, Zalacaín M, Jiménez A (1989) *Molecular analysis of the pac
gene encoding a puromycin N-acetyl transferase from Streptomyces alboniger*,
Gene 79(2):375-380, PMID 2676728,
[DOI](https://doi.org/10.1016/0378-1119(89)90220-5):

- the **organism** is in the title, and the record carries `/strain="ATCC
  12461"` with `/culture_collection="ATCC:12461"`, both verified in the record;
- the **extent** is corroborated by the paper independently of the record: "The
  pac gene contains a 600-nt open reading frame, starting with an ATG codon" --
  this row is 600 nt, ATG through stop;
- the **sequence** is answered by this stage's forced translation, which a
  corrupted record would not survive.

The row now says all of this. **A signature that does not mention the flag is a
signature saying a curator read the record and did not notice its first line.**

```
python features/build/build.py --show PLF:1014
```

---

## `PLF:1015` -- bsd  ·  **RECOMMENDATION: SIGN**

- **Claims**: An enzyme of the fungus Aspergillus terreus that inactivates the nucleoside antibiotic blasticidin S. It hydrolyses the amino group off the drug's cytosine ring, and the deaminohydroxy product no longer blocks peptide-bond formation at the ribosome. Two entirely unrelated deaminases are sold under the name 'blasticidin resistance'; this is the fungal one.
- **Sources**: UniProt `P0C2P0` -> ENA CDS `D83710.1:50..442`  (393 nt, 130 aa, initiator ATG)

Nothing was overturned. **What was added is a primary source for the extent,
which the row did not have.** According to PubMed, Kimura M, Kamakura T, Tao QZ,
Kaneko I, Yamaguchi I (1994) *Cloning of the blasticidin S deaminase gene (BSD)
from Aspergillus terreus and its use as a selectable marker for
Schizosaccharomyces pombe and Pyricularia oryzae*, Mol Gen Genet 242:121-129,
PMID 8159161, [DOI](https://doi.org/10.1007/BF00391004), is the paper that
isolated this cDNA and reports it contains "an open reading frame of 393 bp,
encoding a polypeptide of 130 amino acids" -- **this row's extent to the base.**
The same paper names the other enzyme "bsr, the BS deaminase gene isolated from
*Bacillus cereus*" and reports "no homology and a large difference in codon
usage" between them, which is the two-deaminases warning with a citation behind
it.

The description's unusual opening is deliberate and load-bearing: the obvious
one is a five-token run that SOURCING.md §0.4 hard-fails. Do not tidy it.

```
python features/build/build.py --show PLF:1015
```

---

## `PLF:1017` -- dhfrI  ·  **RECOMMENDATION: SIGN** (naming decision 3 stands separately)

- **Claims**: Type I dihydrofolate reductase, the trimethoprim-insensitive enzyme carried on the Tn7 dfrA1 cassette. It reduces dihydrofolate to tetrahydrofolate exactly as the chromosomal enzyme does, but is bound by trimethoprim far more weakly, so one-carbon metabolism continues while the host enzyme is inhibited. Trimethoprim selection is useful where beta-lactam and aminoglycoside markers are already spent, and the cassette travels in integrons and in broad-host-range backbones.
- **Sources**: UniProt `P00382` -> ENA CDS `X00926.1:236..709`  (474 nt, 157 aa, initiator **GTG**, read as Met)

Measured for this worklist, and it strengthens the pin: `P00382` has three EMBL
cross-references and both of the others were compared to the row.
**`X17477`/`CAA35509` is nucleotide-identical to this row** -- 474 nt, same GTG
start. `X17478`/`CAA35512` differs at exactly one nucleotide, `T223G`, giving
`L75V`. So the pinned sequence is corroborated by a second independent INSDC
record base for base, and the only variant in the entry is a single conservative
substitution.

The claimed `DHFR`/`dhfr` alias collision with `PLF:1023` was measured and does
not exist. The real gap -- that the vernacular `DHFR` resolves to neither row --
is decision 3 above and is a naming choice, not a defect in this row.

Non-ATG start: **GTG**, read as Met. That is a property of the record, not of
this row's arithmetic.

```
python features/build/build.py --show PLF:1017
```

---

## `PLF:1018` -- URA3  ·  **RECOMMENDATION: SIGN**

- **Claims**: Orotidine 5'-phosphate decarboxylase of Saccharomyces cerevisiae, the final step of de novo pyrimidine biosynthesis. Complements a ura3 auxotroph, and is the standard yeast counter-selectable marker as well: cells that carry it convert 5-fluoroorotic acid into a toxic product, so growth on 5-FOA selects for having lost the gene.
- **Sources**: UniProt `P03962` -> ENA CDS `U18530.1:26573..27376`  (804 nt, 267 aa, initiator ATG)

The multi-allele trap the row describes is **confirmed and is worse than the row
says**. Of `P03962`'s nine EMBL cross-references, four carry `A160S` and three of
those four are records whose own titles say *cloning vector*: `U89671` (pLacZi),
`U89927` (pHISi), `U63018` (pRAY-1); the fourth is `K02206`, a genomic record.

**New, and worth adding to the row before you sign it:** all five pRS URA3
vectors deposited in INSDC -- `U03438` (pRS306), `U03442` (pRS316), `U03446`
(pRS406), `U03450` (pRS416), `U03451` (pRS426) -- carry the same two nucleotide
differences, `G411A` and `G478T`, giving `A160S`. So the deviating allele is not
merely "what a construct is likely to carry"; it is what the standard yeast
shuttle-vector series carries, uniformly. The pin on the primary chromosome V
record is the right call and this makes the reason concrete.

```
python features/build/build.py --show PLF:1018
```

---

## `PLF:1019` -- LEU2  ·  **RECOMMENDATION: SIGN, after reading the new paragraph**

- **Claims**: 3-isopropylmalate dehydrogenase of Saccharomyces cerevisiae, the third enzyme of leucine biosynthesis. Complements a leu2 auxotroph, and is one of the four markers the pRS shuttle-vector series is built on.
- **Sources**: UniProt `P04173` -> ENA CDS `X59720.2:91323..92417`  (1095 nt, 364 aa, initiator ATG)

**This row was presented as uncontested and it is not.** It had the same problem
as URA3 and HIS3 and nothing said so. Measured:

| record | vector | vs this row (1095 nt / 364 aa) |
|---|---|---|
| U03437 / U03441 / U03449 | pRS305, pRS315, pRS415 | 4 nt -> **A69V, N300D** |
| U03445 / U03452 | pRS405, pRS425 | 6 nt -> **A69V, G78A, V195L, N300D** |

Read the second row again. **Those five records are one submitter** -- D. J.
Stillman, University of Utah -- **deposited on 10 and 11 November 1993 as one
series**, and the series does not agree with itself about what LEU2 is. No
curator can pick between them from the records.

Part of it is allelic rather than error: `A69V` is also carried by `X03840`
(`CAA27459`), a natural yeast genomic record and one of `P04173`'s own four
cross-references.

The leu2-d point stands unchanged and is separate: that allele differs in how
much upstream sequence it retains, i.e. in a promoter boundary, which SOURCING.md
classes as a convention. It would be a separate row and it is not in this
database.

```
python features/build/build.py --show PLF:1019
```

---

## `PLF:1020` -- HIS3  ·  **RECOMMENDATION: SIGN**

- **Claims**: Imidazoleglycerol-phosphate dehydratase of Saccharomyces cerevisiae, the sixth step of histidine biosynthesis. Complements a his3 auxotroph. The enzyme is competitively inhibited by 3-aminotriazole, which is what makes HIS3 a tunable reporter in two-hybrid work: raising the inhibitor raises the expression threshold a colony has to clear before it grows.
- **Sources**: UniProt `P06633` -> ENA CDS `Z75110.1:238..900`  (663 nt, 220 aa, initiator ATG)

The row's hand alignment was re-derived and is exactly right. Against
`X03245`/`CAA27003`, the classic 219 aa clone: the two are **identical for the
first 324 nt and for the last 331 nt**; the proteins are identical for the first
108 residues and the last 109; the difference is a **3 nt insertion plus a
substitution in one window at residue 109** (this row reads `...KEAL GAV RGVK...`
where the clone reads `...KEAL LA RGVK...`). Not a start-codon convention.

**New:** all four pRS HIS3 vectors -- `U03435` (pRS303), `U03439` (pRS313),
`U03443` (pRS403), `U03447` (pRS413) -- carry **660 nt / 219 aa**, i.e. the
classic clone and not this row. So an exact nucleotide match against a pRS HIS3
construct will fail, for a reason that has nothing to do with the construct. The
row already warns of this in words; the four accessions make it checkable.

The automated cross-reference survey structurally cannot report any of it: for
unequal lengths it reports only that positions are not comparable, which is
correct behaviour -- aligning from residue 1 would dress a frame offset up as
point mutations.

```
python features/build/build.py --show PLF:1020
```

---

## `PLF:1021` -- TRP1  ·  **RECOMMENDATION: SIGN**

- **Claims**: N-(5'-phosphoribosyl)anthranilate isomerase of Saccharomyces cerevisiae, the third step of tryptophan biosynthesis. Complements a trp1 auxotroph.
- **Sources**: UniProt `P00912` -> ENA CDS `V01341.1:103..777`  (675 nt, 224 aa, initiator ATG)

**The one pRS marker that behaves.** All four pRS TRP1 vectors -- `U03436`
(pRS304), `U03440` (pRS314), `U03444` (pRS404), `U03448` (pRS414) -- contain
this row's 675 nucleotides **exactly**, measured. That is the useful contrast
with URA3, LEU2 and HIS3 above and it is worth knowing when a match fails
elsewhere.

The entry's 26 cross-references do carry natural polymorphism -- `S212F` recurs
across the `AJ5856xx` strain-survey haplotypes, with `S172R` and `I48V` in
others -- but these are strain haplotypes of *S. cerevisiae*, deposited as such,
and none of them is a vector allele.

The row's real caveat is unchanged and is a scope point, not an evidence point:
**'TRP1' on a map usually means TRP1-ARS1.** In YRp7 and its descendants the
label covers this gene together with the adjacent ARS, which is what makes the
plasmid replicate. ARS1 is a separate element, its boundary is a convention, and
it is not in this database.

```
python features/build/build.py --show PLF:1021
```

---

## `PLF:1022` -- TK  ·  **RECOMMENDATION: YOUR CALL, twice.** Patent flagged

- **Claims**: Thymidine kinase of herpes simplex virus type 1, gene UL23. Much less selective than the cellular enzyme, it phosphorylates nucleoside analogues such as ganciclovir and aciclovir, which are then extended to triphosphates that poison DNA synthesis. That promiscuity is the point: the gene is the classic negative-selection and suicide marker, killing the cells that carry it as soon as the prodrug is supplied.
- **Sources**: UniProt `P0DTH5` -> ENA CDS `complement(X14112.1:46672..47802)`  (1131 nt, 376 aa, initiator ATG)
- **`patent_flag = 1`, not adjudicated.**

**Decision (a), the lookup gap.** `TK` is UniProt's gene symbol and here that is
two letters. `HSV-TK`, `HSVtk` and `UL23` -- the spellings on real maps -- are
not aliases of this row and will not resolve. Adding them means writing names
ourselves into a column sourced entirely from UniProt under CC BY. That is a
sourcing-posture decision, not a research question.

**Decision (b), the strain.** Measured: the second reviewed 376 aa HSV thymidine
kinase, `Q9QNF7`, differs from this row's protein at four positions -- **N23S,
K36E, R89Q, A265T**. A construct built from it is not corrupt and would fail an
exact protein match against this row.

**Evidence that bears on (b) and is new.** Two further records were compared:

- `V00470`/`CAA23742` (McKnight's HSV-1 TK) is 1131 nt and 376 aa but differs
  from this row's protein at **seven** positions and from `Q9QNF7` at five, so it
  is a third sequence and not a tie-breaker.
- `L19900` ("Cloning vector cosmid svPHEP DNA sequence encoding beta-lactamase
  and HSV thymidine kinase genes") carries a TK whose protein is **identical to
  this row**, with only two nucleotide differences (`A102G`, `T969C`). So real
  vector-borne HSV-TK does match this row's *protein* exactly while differing in
  nucleotides -- which is an argument for the pin as it stands, and a reason the
  translated tier matters more than the nucleotide tier for this row.

Nothing here forces either decision. Both are yours.

```
python features/build/build.py --show PLF:1022
```

---

## `PLF:1023` -- Dhfr  ·  **RECOMMENDATION: SIGN** (naming decision 3 stands separately)

- **Claims**: Mouse dihydrofolate reductase. Reduces dihydrofolate to tetrahydrofolate, the one-carbon donor for thymidylate and purine synthesis. Used as a selection marker in DHFR-negative CHO lines, and as an AMPLIFICATION marker: stepping methotrexate up selects for cells that have amplified the locus, and a linked transgene is amplified with it.
- **Sources**: UniProt `P00375` -> ENA CDS `BC005796.1:49..612`  (564 nt, 187 aa, initiator ATG)

Checked and nothing overturned: the row's 187 aa is `P00375`'s canonical
sequence and the entry carries 15 EMBL cross-references, a mix of mRNA and
genomic. The row's argument for pinning the cDNA rather than the six-exon
genomic join -- that the two give the identical protein and differ at exactly
one synonymous nucleotide, so the protein cannot tell them apart and a
nucleotide match can, and a vector carries the cDNA -- **is the row's own
measurement and was not re-derived in this pass.** If you want it independently
confirmed before signing, that is one comparison of two cached CDS records; it
is the only marker claim in this worklist that is neither re-derived nor
sourced to a paper.

```
python features/build/build.py --show PLF:1023
```

---

## `PLF:1024` -- gpt  ·  **RECOMMENDATION: SIGN**

- **Claims**: Xanthine-guanine phosphoribosyltransferase of Escherichia coli. Salvages guanine, xanthine and hypoxanthine into their nucleotides. Mammalian cells cannot use xanthine this way, so in medium containing mycophenolic acid, which blocks de novo GMP synthesis, together with xanthine, only cells expressing this enzyme make GMP and survive.
- **Sources**: UniProt `P0A9M5` -> ENA CDS `U00096.3:255977..256435`  (459 nt, 152 aa, initiator ATG)

Checked and nothing overturned. The pin is the K-12 reference genome, the
protein is `P0A9M5`'s canonical 152 aa, and the entry's nine EMBL
cross-references are all *E. coli*. The row's caveat is a name warning and it is
correct: **two different genes are written `gpt`**, and this is the bacterial
xanthine-guanine enzyme (often written `Ecogpt` in mammalian work), not the
mammalian HPRT.

```
python features/build/build.py --show PLF:1024
```

---

## `PLF:1025` -- bar  ·  **RECOMMENDATION: SIGN.** Patent flagged

- **Claims**: Phosphinothricin N-acetyltransferase from the bialaphos biosynthesis cluster of Streptomyces hygroscopicus. Acetylates the free amino group of phosphinothricin, the glutamine-synthetase inhibitor released from bialaphos and sold as the herbicide glufosinate. The standard herbicide-resistance selection marker for plant transformation.
- **Sources**: UniProt `P16426` -> ENA CDS `X17220.1:31..582`  (552 nt, 183 aa, initiator ATG)
- **`patent_flag = 1`, not adjudicated.**

**The boundary is one base and it is base 1, and that re-derives exactly.**
Measured against the other cross-reference: `X05822`/`CAA29262`, the native-locus
record, is 552 nt beginning **GTG**, differs from this row at exactly one
nucleotide (`A1G`), and gives the identical 183 aa protein with a single
`M1V`-shaped difference at residue 1. The pinned record, from the
plant-transformation cassette, begins **ATG**.

A curator wanting the native gene pins the other record and must expect a
position-1 mismatch against every construct. That is the whole of the choice and
the row states it.

```
python features/build/build.py --show PLF:1025
```

---

## `PLF:1026` -- pat  ·  **RECOMMENDATION: SIGN.** Patent flagged

- **Claims**: Phosphinothricin N-acetyltransferase of Streptomyces viridochromogenes. The same reaction and the same glufosinate selection as the bar gene of the row above, from a different producer strain. The two are used interchangeably in plant transformation and are distinct sequences, so a construct carrying one does not match the other at the nucleotide level.
- **Sources**: UniProt `Q57146` -> ENA CDS `X65195.2:29930..30481`  (552 nt, 183 aa, initiator **GTG**, read as Met)
- **`patent_flag = 1`, not adjudicated.**

Measured, and it corroborates the pin: the entry's other cross-reference,
`M22827`/`AAA72709`, is **nucleotide-identical to this row** -- 552 nt, same GTG
start. Two independent INSDC records agreeing base for base is stronger than
this row currently claims.

The scope warning stands: `bar` and `pat` are used interchangeably in the
literature and are two genes. Whether a map's `BlpR` or `PPT-AT` label means
this row or `PLF:1025` cannot be settled from the label, only from the sequence.

```
python features/build/build.py --show PLF:1026
```

---

## `PLF:1027` -- rpsL  ·  **RECOMMENDATION: SIGN** (naming decision 4 stands separately)

- **Claims**: 30S ribosomal protein S12 of Escherichia coli, part of the decoding centre of the small subunit. The wild-type allele is DOMINANT SENSITIVE to streptomycin: a streptomycin-resistant host carries a mutant rpsL, and supplying the wild-type protein in trans restores sensitivity. That inversion is what makes the gene a counter-selectable marker -- an rpsL-neo cassette is selected onto a target with kanamycin and selected off it again with streptomycin.
- **Sources**: UniProt `P0A7S3` -> ENA CDS `complement(U00096.3:3474178..3474552)`  (375 nt, 124 aa, initiator ATG)

Checked and nothing overturned. The pin is the K-12 reference genome and the
protein is `P0A7S3`'s canonical 124 aa; UniProt's recommended name is "Small
ribosomal subunit protein uS12", which is the name the row carries in its
description.

The row's own point is the one that matters and it is documented rather than
resolved: **this is not a resistance gene and its alias says the opposite.**
That is decision 4 above.

```
python features/build/build.py --show PLF:1027
```

---

## Refused on the evidence: five Class B elements that are NOT in the table

Added 2026-08-10. `stage_classb.MIN_PLACEMENTS` requires **two independent
submissions to annotate a feature at exactly the shipped extent**, edge for
edge, before a row may carry `boundary_rule = consensus_of_insdc`. Until that
build the stage measured where each depositor drew the edges, wrote it into
`notes`, and tested nothing -- so "consensus" could rest on one lab. These five
rested on one lab, and the build drops each of them with its numbers printed:

| Row | Element | Submissions holding the bases | Placing it at our extent |
|---|---|---|---|
| `PLF:4002` | lac promoter | 4 | 1 (HM126493.1) |
| `PLF:4003` | tac promoter | 3 | 1 (MH488909.1 + MH488911.1, one address) |
| `PLF:4004` | trc promoter | 2 | 1 (U13872.1, which is the anchor itself) |
| `PLF:4005` | CMV promoter | 3 | 1 (LC897329.1) |
| `PLF:4011` | SV40 early poly(A) | 3 | 1 (LT009443.1) |

**These are not deletions and they are not `HELD`.** They stay in
`stage_classb.ITEMS`, keep their ids **and their index**, and are re-measured on
every build, so a row returns by itself the moment its evidence does. That is by
design and it has now been exercised, so the qualification it needs is here:

> **A ROW MAY ALSO RETURN BECAUSE THE IMPLEMENTATION CHANGED, AND THAT IS NOT
> THE SAME THING.** On 2026-08-11 `verify()` was fixed: it had scored a record's
> FIRST copy of an element and no other, so a depositor who carries the element
> twice and draws our edges over the SECOND copy was measured as having drawn
> nothing. Since that widens what counts as corroboration for **every** Class B
> row at once, all fourteen were measured under both loops before the fix
> landed. **NONE OF THE FIVE ABOVE RETURNED. NO ROW MOVED AT ALL** -- the two
> numbers in the table above are the same under either loop, all nine rows in
> the table keep their extents and their `notes` byte for byte, and `features.tsv`
> and `provenance.tsv` rebuild identical. The reason is that only four witness
> records in `ITEMS` carry their element more than once -- `V01146` (T7, 7x),
> `AY288927` (SP6, 3x), `U13859` (rrnB T2, 2x) and `AJ318471` (T3, 3x) -- and
> every one of them annotates its copies alike, so scoring the first was already
> scoring all of them. Eleven of the held extents below were measured the same
> way and exactly one moves: the mouse PGK promoter, from one exact placement to
> two, and it is held rather than issued for the reason its entry gives. **If a
> refused row is ever seen to return, this is the first question to ask of it:
> did the evidence change, or did the code?**

Two ways to rescue one, both a curator's and neither a program's:

1. **Cite more evidence.** Find further independent submissions that draw the
   same edges and add them to the item's `exemplars`. Note what this is not:
   searching until the check goes green is the failure mode, and for at least
   `PLF:4005` and `PLF:4011` a wider survey says it will not work -- across 481
   records, roughly one independent submission in nine uses our CMV-promoter
   extent, and the SV40 poly(A) figure is one in forty-six.
2. **Re-cut the extent** to one the evidence already corroborates, and rewrite
   the row's basis to match. For `PLF:4002` that means confronting the 84 nt
   convention the anchor's own annotation gestures at; for `PLF:4005` it means
   deciding whether the CMV promoter is separable from the enhancer at all --
   the question set out under `PLF:4006` above. Withdrawing that row on
   2026-08-11 removed the half-block from the table; it did not answer this,
   and `PLF:4005` is still refused on the same evidence it was refused on.

`PLF:4005` is worth reading twice. **Its only exact corroboration was
`LC897329`**, whose feature table is SnapGene's Common Feature naming from top to
bottom -- `CMV enhancer`, `CMV promoter`, `bovine growth hormone (bGH)
polyadenylation signal` -- with no `label:` tell anywhere in it, so the stage's
SnapGene screen passes it and counts it. That is the blind spot `SOURCING.md`
§0.6 describes, in a record this database relies on. It was not a taint check
that caught the row; it was the extent rule, which names no vendor at all.

---

## Worked up and deliberately NOT issued as rows

SOURCING.md section 6 budgets about 40 Class B items. Ten survived the rules and
nine of those are in the table, the tenth having been withdrawn; five more were
built and refused on the extent evidence (above). The rest are recorded in
`stage_classb.HELD` with the reason, so that nobody re-does the work and
concludes it was never done.

**Rewritten 2026-08-11 against fresh measurement, and this list is not what it
was.** Three entries left it for the table (`PLF:4012`, `PLF:4013`, `PLF:4014`).
Two entries were one name over several unrelated elements and are now separate
entries with separate statuses -- the CAG line became three, the dropped
`tetO / TRE / Ptet` line became the three-way split it asked for. Of the rest,
every reason changed: two were **backwards** about which of the two rules they
failed, one had been checking the wrong accession, and one turned out to fail
only because `verify()` scored a record's first copy of an element and not its
second -- **that defect was fixed on 2026-08-11 and PGK's entry now records a
measurement instead of a complaint.** Where an entry now says an extent CLEARS the corroboration floor and is
still not a row, the missing thing is named.

- **SV40 early promoter** -- The 330 nt convention is a contiguous circular interval that WRAPS the numbering origin of the SV40 record, which this schema's accession:lo-hi:strand boundary_evidence cannot express, and a rival 283 nt form does not place as a single interval at all. The region also carries a tandem repeat, so the two forms may differ in repeat copy number -- that was NOT counted and is not offered as a finding. RE-CHECKED 2026-08-11 ON A 419 nt FORM OF THE SAME CONVENTION, and the wrap is not one record's numbering quirk but every record's: the interval is 5171..5589 of a 5243 bp genome, i.e. it runs 346 bases past the origin, and the assembled 419 bases occur CONTIGUOUSLY in none of J02400.1, AF316139.1 or EF579804.1 -- all three 5243 bp, all three sharing that origin. The obstacle is the schema and not the evidence, which is a different thing from the other holds in this list and would be cleared by a boundary_evidence form that can express a circular join, not by more fetching.
- **U6 promoter (human)** -- STILL HELD, and the reason it stood under until 2026-08-11 was backwards. It said only ONE independent submission witnesses the 249 nt extent and that it 'fails on witnesses, not on evidence'. Both halves are the wrong way round. The bases are witnessed abundantly: four independent addresses in a single afternoon's fetching hold them (Allen Institute for Brain Science PZ036121.1, George Washington University JN255690.1, AIST LC414435.1, UCSD MK318530.1) and a 2026-08-11 survey of 479 records put it at eleven. What exactly ONE submission does is DRAW those edges: OP099837.1, OP099840.1 and OP099843.1, all Drug Discovery Sciences, Boehringer Ingelheim Pharma, one address. So it clears MIN_SUBMISSIONS several times over and fails MIN_PLACEMENTS at one, which is the opposite failure and has the opposite remedy. THE EXTENT IS THE ARTICULABLE ONE, WHICH IS WHY IT IS WORTH SAYING SO. M14486.1 (human U6 gene, clone pGEM/U6, Kunkel, Texas A&M; Kunkel, Maser, Calvet & Pederson 1986, Proc Natl Acad Sci USA 83:8575-8579, PMID 3464970) annotates the PSE at 263..282, the TATA box at 298..306 and prim_transcript 329..435, so +1 is 329 and the 249 nt convention is exactly M14486.1:80-328 = -249..-1, the same rule PLF:4000 and PLF:4001 use. (That prim_transcript is invisible to this stage's own parser -- see the EF-1alpha row's caveat on fifteen-character feature keys -- and was read by hand.) TWO NESTED ALTERNATIVES DO CLEAR THE FLOOR AND ARE DELIBERATELY NOT OFFERED AS ROWS. 241 nt = M14486.1:80-320 is annotated exactly by the Allen Institute (PZ036121.1, PZ036141.1 -- one address), PX139666.1 and MK318530.1, three independent submissions; it stops nine bases short of +1 for no reason anyone has articulated. 264 nt = M14486.1:65-328 is annotated exactly by George Washington University (JN255690.1, JN255691.1 -- one address) and AIST (LC414435.1), two submissions, at the floor, and it ends at -1 like the 249. Adopting either BECAUSE the 249 failed is re-cutting an extent until two records agree with it, which is the move this stage exists to refuse; the module docstring already says such a re-cut 'is a curator's decision and not this program's'. They are recorded here so that decision is cheap, not taken. One further measurement, because it is the trap: three submissions DO draw a 249 nt extent whose sequence is not the primary record's -- one substitution at M14486.1:146 -- and MG550105.1 is one of them. Counting those would ship a sequence that is in no primary record, which is the failure the EF-1alpha and PGK entries are about.
- **H1 promoter (human)** -- STILL HELD, and both halves of the old reason were wrong; the corrected reason is stronger and it fails on two legs rather than one. THE PROVENANCE GAP IS CLOSED, so 'no genomic record carrying the upstream promoter could be located' is retired. The record is X16612.1, 'Human gene for H1 RNA', 1057 bp, clone pMBH1, deposited by S. Altman at Yale (Baer, Nilsen, Costigan & Altman). It annotates the TATA box at 345..348 and precursor_RNA 375..715, and X16612.1:152-366 -- 215 nt -- is held verbatim by FOUR independent addresses: AL355075.6:176331-176545 (Genoscope, chromosome 14 clone), AF479321.1:1615-1829 (Genome Sciences, University of Washington), Neurology at the University of Goettingen (AY640625.1:2317-2531 and AY640626.1:2373-2587, one address between them) and the anchor itself. This page and `stage_classb.HELD` both said THREE until 2026-08-11; re-measured under the every-copy rule, Goettingen holds the 215 verbatim and draws it 5'+0/3'+1, which is the whole 215-versus-216 question in one number. The old reason was checking X15624, 'Human H1 RNA', which is a 340 bp transcript-level record and contains none of the promoter. THE CONSENSUS WAS NEVER THERE, which is the leg that now kills it. 'Three independent submissions agree on 216 nt' counted address STRINGS, which is precisely the error same_submitter() exists to prevent (finding 2 above). Every record found holding those 216 bases is one department: AY640625.1 and AY640626.1, Neurology, University of Goettingen, Waldweg 33 -- one address, one submission. FJ687158.1, DQ465352.1, MH749464.1, LT727092.1, AL355075.6 and AF479321.1 were each checked and hold no copy of the 216 nt at all. Corroborating submissions: ONE. AND THE 216 nt IS STILL NOT A SLICE OF ANYTHING. It is X16612.1:152-366 plus one further base, and that base is A where X16612.1:367 is T -- so it is 215 verbatim bases with a 216th that disagrees with the gene, and boundary_evidence would have nothing to point at. The genomic 215 nt form, which does slice cleanly, is annotated by NOBODY: four submissions hold it and zero draw it, so it fails MIN_PLACEMENTS at nought -- and it fails there identically under the every-copy rule, because no witness of this element carries it more than once. WHAT WOULD RESCUE IT: two genuinely independent submissions annotating exactly 215 nt (today none) or exactly 216 nt (today one, and it would still not be a verbatim slice). Nothing in the schema and nothing about the anchor is now the obstacle.
- **PGK promoter (mouse)** -- HELD, AND IT IS THE ONE ENTRY THIS PAGE'S 2026-08-11 REWRITE GOT WRONG IN THE OTHER DIRECTION. 'NOT A VERBATIM SLICE OF ANYTHING' IS RETIRED: the 508 nt element is exact at BX469914.4:13192-13699 on the plus strand -- 'Mouse DNA sequence from clone RP23-217J7 on chromosome X', Wellcome Trust Sanger Institute -- so a citable modern anchor exists. It is not in M18735.1 (Adra, Boer & McBurney 1987, Gene 60:65-74), the 1987 Pgk-1 exon 1 record the older reason measured against: the longest exact prefix present there is 64 nt and the record then diverges, which is a 1987 record differing from the genome and NOT the engineered internal deletion the EF-1alpha entry describes. (This page said 'Adra, Boer & Klip' until 2026-08-11; the third author is McBurney, and `stage_classb.HELD` had it right.) 'ONE SUBMISSION' WAS WRONG BY AN ORDER OF MAGNITUDE -- a survey put nine independent addresses on the exact bases. **IT DIED ON MIN_PLACEMENTS AT ONE BECAUSE OF A CODE DEFECT, NOT BECAUSE OF THE EVIDENCE, AND THE DEFECT IS NOW FIXED.** CR293496.1 (Sanger Centre) annotates regulatory 4137..4644 = exactly these 508 bases. AB242435.1 (Central Institute for Experimental Animals, Kawasaki) carries the element TWICE and annotates the SECOND copy at exactly 508 nt (regulatory 2089..2596), while the first copy, at 374-881, carries a 516 nt feature (regulatory 366..881, 5'+8/3'+0). `verify()` scored `occurrences()[0]` and no other copy, so it saw the 516 and never the 508. `place_in_record()` now scores every copy, and re-measured under it this element has **two independent submissions and two exact placements** -- BX469914 and CR293496 are both Hinxton addresses and `same_submitter()` merges them, so the anchor adds no third opinion -- and it would clear both floors. **WHAT IS STILL OWED IS THE ONE DECISION NO PROGRAM MAY TAKE: issuing the row.** It needs a curated name and description, and an id APPENDED after `PLF:4014` so that none of `PLF:4000`..`PLF:4014` moves. A stage that promoted an element the moment its own code stopped under-counting it would be adjusting its own membership, which is the move every refusal in this file exists to refuse. The measurement is done; the judgement is yours. And note what the previous version of this bullet predicted -- that fixing `verify()` 'will alter the measured notes of rows already signed'. It was checked: no Class B row is signed, `SIGNOFF.tsv` carries no `PLF:40*` line at all, and `features.tsv` and `provenance.tsv` rebuild byte for byte either side of the fix.
- **AG promoter of pCAGGS, 1342 nt** -- HELD AT ONE PLACEMENT, and the old CAG entry's central claim is confirmed harder than it was stated. These 1342 bases -- LT727518.1:3457-4798, the pCAGGS record deposited by BCCM/LMBP Gent -- share ZERO 20-mers with X17403.1, the human cytomegalovirus genome, on either strand. There is no CMV sequence in the element that most maps call a CAG promoter's front half; it is chicken beta-actin running into rabbit beta-globin. WHAT KILLS IT IS NOT THAT: it is that 'widely deposited' was false. The extent appears in dozens of records and ONE submission -- the BCCM/LMBP bulk plasmid-collection deposit -- which is finding 2 of the module docstring in its purest form. One placement, floor of two. A second, independent AG-type element of 1347 nt exists (EF186083-EF186088, LUMC Leiden), also with no CMV sequence and also with one submission, so the family is not one depositor's quirk and is still not a consensus.
- **CAG promoter, the CMV-enhancer-containing forms** -- HELD, AND THE NUMBER IS THE FINDING: fifteen distinct extents were measured and not one of them reaches two independent submissions. The 935 nt form (JN898959.1/JN898962.1, KIST, one submission) really does begin inside the cytomegalovirus enhancer -- 210 20-mers shared with X17403.1, against zero for the AG element above -- and so do the 1699, 1722, 1740, 1733, 1728, 1721, 1720, 1710, 1677, 1673, 1667 and 1647 nt forms, each with exactly one submission behind it. Fifteen single opinions is not a convention, and the reason to keep this entry separate from the AG one is that a single row named 'CAG promoter' spanning both would merge an element that contains CMV with an element that provably does not.
- **chicken beta-actin promoter** -- HELD, AND THIS IS THE ONE TO READ TWICE, because the two legs of the rule are satisfied by two INCOMPATIBLE extents and the near-miss is exactly the shape PLF:4006 was withdrawn over. 276 nt = X00182.1:268-543 has the better boundary argument in this whole file: the record annotates a CAAT signal at 455..459 and a TATA box at 517..524, and the extent is -276..-1 against the transcription start, +1 excluded -- the PLF:4000 rule arrived at independently. It has TWO submissions annotating it exactly, and the second one must not be counted: OK413188.1 (OHSU/ONPRC) is an honest witness, and OP697986.1 is the same submitting address as OP697991.1 -- same_submitter() returns True -- which SOURCING.md section 0.6 names as a DEMONSTRATED false negative of record_is_snapgene_annotated, and which section deliberately declined to widen the screen for. The mechanical screen passes it; a curator may not. ONE honest placement. 278 nt has two honest submissions (Oxford Protein Production Facility, EF372394.1 and EU733644.1, deposited 2007 and 2008; Witten/Herdecke, PQ540283.1, 2024) and no anchor: it is the 276 nt with TWO extra G in a G-homopolymer -- measured, the run is 14 G in X00182.1 and 16 G in the vector form -- so it is a verbatim slice of no primary record and reference_nt taken through that tract will silently miss whichever half of the wild population has the other length. WHAT WOULD RESCUE IT: one more independent submission drawing 276 nt exactly. Nothing else is missing.
- **PLtetO-1 (bacterial tet-regulated promoter)** -- STILL NOT A ROW, BUT NO LONGER FOR THE OLD REASON. The old 'tetO / TRE / Ptet' entry was DROPPED on the ground that the name covers at least four unrelated elements and 'must be split into separately named rows before any part of it can be sourced at all'. This entry and the two below are that split, and the sourcing now exists for parts of it. A 74 nt extent -- KX682238.1:4307-4380, the lambda PL promoter with two tet operators -- is annotated edge for edge by FOUR independent addresses checked on 2026-08-11: Pacific Northwest National Laboratory (KX682238.1), Biomedical Engineering, Boston University (KM521209.1), Biological Engineering, MIT (KT893256.1, KX264176.1 -- one address) and Biology, University of Texas at Tyler (MK753225.1). None carries the SnapGene tell. It clears both legs. WHAT IT DOES NOT YET HAVE, and why no row is offered: a curated name and a description written from the primary source (Lutz & Bujard 1997, Nucleic Acids Res 25:1203-1210, which was NOT read here), and a decision between this 74 nt and the 54 nt form nested inside it -- JX155235.1:1-54, whose placements checked here (JX155235.1, JX155240.1, JX155247.1) are all ONE address, UC Berkeley EECS. This is a designed hybrid with no natural locus, so it would be anchored on a construct record exactly as PLF:4003 and PLF:4004 are. It is the strongest unclaimed candidate in this list.
- **tetO7 / TRE (mammalian tet-response element)** -- HELD, WITH THE CORROBORATION MEASURED AND A HAZARD THAT IS NOT YET ANSWERED. A 271 nt heptamer array -- MG883664.1:1376-1646 -- is annotated edge for edge by three independent addresses checked on 2026-08-11: Southwest University (MG883664.1, protein_bind), University of York (PQ260749.1) and Human Genetics, University of Michigan (PQ360726.1). A 291 nt form was reported by the same survey with three more. Both clear the floor on their face. THE HAZARD IS THAT THIS ELEMENT IS A TANDEM REPEAT, so two extents differing by 20 nt may be the same convention counted over a different number of operator copies rather than two rival boundaries -- and this stage has no way to tell those apart, because occurrences() matches a whole string and says nothing about periodicity. That is the same hazard the SV40 early promoter entry flags and does not count. Until somebody counts the repeat units in each deposit and says which extents are the same element, a row here would be asserting a boundary it has not distinguished from an artefact.
- **CMV-tetO2, PTight and the remaining tet hybrids** -- HELD, AND STILL PARTLY UNSPLIT. The third and fourth elements the dropped 'tetO / TRE / Ptet' entry named -- a CMV-tetO2 hybrid and a bacterial pTet -- plus a PTight/TRE-Tight form of 315 nt that the 2026-08-11 survey turned up, have not been separated into individually named elements here and none of their extents was measured against this stage's own rule. A CMV-tetO2 element additionally OVERLAPS PLF:4005's refused interval, so it cannot be worked up independently of the decision the CMV promoter row is still waiting on. Recorded so the split is complete on paper even where the evidence is not.

The recurring reasons are worth naming, and they are not the ones this list gave
before: **holding the bases is not drawing the edges** (U6, PGK, the AG promoter
of pCAGGS -- all three have witnesses in abundance and one placement), **the
element is not a verbatim slice of anything** (H1's 216 nt form, the 278 nt
chicken beta-actin form), **the schema cannot express the interval** (SV40 early
promoter, which wraps the numbering origin in every genome record checked), and
**the name covers more than one element** (CAG, tetO/TRE -- now split rather than
merely deplored). One new reason has appeared and is the most important of them:
**an extent can pass the mechanical screen and still not be honestly
corroborated.** The 276 nt chicken beta-actin promoter has two submissions
drawing its edges, and the second is the record `SOURCING.md` section 0.6 already
names as a demonstrated false negative of the SnapGene screen. The screen passes
it. A curator may not. That is the same question `PLF:4006` was withdrawn over.

---

## If you withdraw a row

**This section was wrong until 2026-08-11**, and it was wrong about the thing
that mattered most that day, because withdrawing `PLF:4006` was the live
question. It has since been corrected twice: first to describe the hazard, and
then -- when the withdrawal was actually carried out -- to describe the mechanism
that now exists. Read to the end; the last part is the part that is code.

It used to say: remove the item from `stage_classb.ITEMS` or
`stage_uniprot.ITEMS`, add it to the stage's `HELD` tuple with the reason, and
rebuild -- "IDs are allocated from where a row is *declared*, never from how many
survived, so dropping one does not renumber anything after it".

The second half is true of a row **dropped by a check** -- which is exactly what
the five refused Class B elements are: they stay in `ITEMS`, keep their index,
and are re-measured every build. **It is not true of an item deleted from the
source.** Both stages allocate `PLF:{ID_BASE + i}` from the item's *index in the
tuple*, so deleting one renumbers every item after it. Measured on 2026-08-11 by
enumerating the tuple with and without the item:

| if the CMV enhancer item were deleted from `stage_classb.ITEMS` | |
|---|---|
| `PLF:4006` | would come to mean **T7 terminator** |
| `PLF:4007` | would come to mean **rrnB T1 terminator** |
| `PLF:4008` | would come to mean **rrnB T2 terminator** |
| `PLF:4009` | would come to mean **bGH poly(A) signal** |
| `PLF:4010` | would come to mean **SV40 early poly(A) signal** |

`build.py` catches this -- it re-reads the previous `features.tsv` and refuses to
write when a published id changes meaning -- so the failure is loud rather than
silent. It is still a failure, and the rebuild does not complete.

**So: to withdraw a published row, keep its declaration in `ITEMS` at its index
and stop it at the gate.** Until 2026-08-11 the only gates that stopped a row
were the verification checks themselves and there was no "withdrawn" marker.
There is one now, and this is how to use it.

### The mechanism, as built on 2026-08-11

1. **Set `withdrawn` on the item, in place.** `stage_classb.Convention` and
   `stage_uniprot.Natural` both carry the field -- both, so the mechanism is not
   a special case for whichever stage happened to need it first. It takes the
   **reason**, not a bool: an id is permanent, so a withdrawal is permanent, and
   `withdrawn = True` would record that somebody decided without recording what
   they decided.
2. **Do not touch anything else.** Leave the item where it is. The id comes from
   its index, so its place in the tuple is what keeps the number spoken for; the
   row leaves the table and the id is *retired*, never reissued.
3. **Rebuild.** `build()` drops the row and prints `WITHDRAWN` with the reason
   against the id, beside the ordinary `DROP` lines but distinct from them,
   because a decision is not a check failing. `build.py`'s id-stability audit
   then reports the absence as a withdrawal instead of refusing to write --
   **only** because `withdrawn_ids()` explains that exact id. Any other
   disappearance is still fatal, and `--allow-id-drift` is still the only way
   past a genuine repointing.
4. **Fix the counts.** The README headline counts are test-asserted from the
   live tables by `pl-features`' `the_readmes_state_the_signoff_count_the_
   database_has`, so a withdrawal that leaves them alone is a red build.

The check that makes any of this trustworthy is `stage_classb.self_test()` item
8, which runs on every build: it pins `PLF:4006`..`PLF:4010` to the elements
they were published as, asserts that marking one withdrawn moves none of them,
and asserts against the same fixture with the item **deleted** that the pin
really does catch the five reassignments in the table above. Driven at HEAD by
deleting the CMV enhancer declaration for real, it failed on all five ids and
the build wrote nothing.

`stage_classb.ITEMS` carries a comment recording all of this at the point where
somebody would reach for the delete key.

`PLF:4006` was withdrawn under this mechanism on 2026-08-11. The table went from
110 rows to 109; the 89 signatures and the other 20 proposed rows are
byte-identical either side of it, and `SIGNOFF.tsv` was not touched.
