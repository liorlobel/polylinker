# Changelog

This file exists because of the updater. Since v0.1.2 a copy of Polylinker can
tell you that a newer version exists, and until now there was nowhere to find
out what is in it before saying yes. A version number on its own is not
information, and "there is an update" without "here is what changed" is a
request to trust rather than to check.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions
are three numbers compared as numbers, not as text: `pl update` refuses anything
that is not numerically newer than the copy running it, so 0.1.10 is an upgrade
from 0.1.2 and never the reverse. That rule and the attack it exists to stop —
a signed rollback onto an older release with public vulnerabilities — are in
[`crates/pl-update/src/version.rs`](crates/pl-update/src/version.rs). This is
0.x, and no compatibility promise has been made yet.

**Nothing in any of these releases is code-signed**, and nothing above them is
going to be: code signing came off the roadmap on 2026-08-06, so this is a
settled property of the project rather than a run of versions waiting for a
certificate. Windows SmartScreen and macOS Gatekeeper do not recognise the
publisher and say so, on every version below. The signing described under 0.1.2
covers the release *manifest*, which is a different thing;
[`docs/RELEASING.md`](docs/RELEASING.md) is precise about which guarantee is
which.

## [Unreleased]

**Nothing here changes what Polylinker does to a sequence, and no row was
signed.** The 89 records `pl annotate` searches by default are unchanged, byte
for byte, with every signature still valid; `features/SIGNOFF.tsv` is
byte-identical to 0.6.0 and CI still proves the build never writes it. Of the 21
`proposed` rows, **one has been withdrawn by the curator**; the other 20 are
still `proposed` and byte-identical either side of the withdrawal. What else
changed is the prose those rows carry, and the worklist that asks a human to
read them.

### Removed — `PLF:4006`, the CMV enhancer, withdrawn by the curator

**The table is 109 rows; it was 110.** On 2026-08-11 Lior Lobel withdrew
`PLF:4006` rather than sign it. The reason is recorded beside the declaration in
`features/build/stage_classb.py` and printed by every build: its `notes`
referenced `PLF:4005`, which is not in the table, and asserted a shipping
condition — *ship this row and the enhancer together or not at all* — that the
table violates; of this project's own evidence only the SnapGene-shaped
submissions draw the 380/204 split, while every other deposit annotates the
region as a single element and calls it a promoter; and Boshart et al. 1985
(Cell 41:521–530, PMID 2985280) place the enhancer at −118..−524, which straddles
the split on either numbering, so neither of the row's edges is a literature
edge.

**The id is retired, not freed.** `PLF:4006` will never name anything else. The
declaration stays in `stage_classb.ITEMS` at its index, carrying the reason,
because that is what keeps the number spoken for — deleting it would have moved
the T7 terminator into `PLF:4006` and shifted four more published ids, which
0.6.0 measured and which the mechanism below now prevents.

Withdrawing this row **removes an instance and answers no question.** The posture
question underneath it — whether SnapGene-shaped corroboration counts for a Class
B extent — is exactly as open as it was, the SnapGene screen is unchanged, and
`PLF:4005` is still refused on the evidence that refused it.

**Six further counts moved with the row, in files no test reads.**
`features/NOTICE` said the taint gate ran over 110 descriptions and that seven
Class B rows carry the 2026-08-10 retrieval date; `README.md` said the table
holds 21 unread rows; `features/README.md` said 7 Class B elements; `PROPOSED.md` headed
its Class B section *The 7 Class B rows*; and two Rust doc comments described a
110-row table with 21 proposed and quoted a failure transcript reading *7 Class B
rows*. Only the two README headline claims are test-asserted, which is exactly
why the rest went stale — so each was recomputed from the tables rather than
decremented, and the taint gate was re-run to get the description count instead
of subtracting one from it: **1,367 of theirs against 109 of ours, no shared
five-token run, nothing above 60% containment**, and the same five rows above the
30% line with the same longest runs (1, 2, 4, 3, 3).

### Added — a row can be withdrawn, and a test proves the other ids do not move

`Convention` (Stage 5) and `Natural` (Stage 2) both gained a `withdrawn` field.
Both, deliberately: the two stages allocate ids the same way and carry the same
hazard, so a mechanism built for one would be a trap set for whoever first needed
it in the other. It takes the **reason**, not a bool — an id is permanent, so a
withdrawal is permanent, and a bare flag would record that somebody had decided
without recording what they decided. A marked item is dropped by `build()`, its
id is still consumed, and the drop is reported as `WITHDRAWN` with its reason
rather than as a check failing, because a decision is not a failure.

`build.py`'s id-stability audit learned the one absence that has an answer: a
published id that disappears is still fatal *unless* a stage declares it
withdrawn, and `--allow-id-drift` remains the only way past a genuine
repointing. Four cases in `build.self_test()` drive that hole, including a
withdrawal declared for the wrong id and a row that is present and repointed.

The check that matters is `stage_classb.self_test()` item 8: it pins
`PLF:4006`–`PLF:4010` to the elements they were published as, asserts that
marking one withdrawn moves none of them, and asserts against the same fixture
with the item **deleted** that the pin catches all five reassignments. Proven by
doing it: deleting the CMV enhancer declaration for real failed the pin on every
one of the five ids, and the build wrote nothing.

**And the failure now says so.** When the declaration is missing the check exits
early, and that early exit used to discard every label `must()` had recorded —
so reproducing the break printed one sentence about the enhancer and no evidence
at all that four further published ids had moved with it, while this entry and
`PROPOSED.md` both claimed the pin fails on all five. The five reassignments are
the entire subject of the check; a message that measures them and then throws
them away leaves its own claim unwitnessed. The exit now carries every pin that
failed, so the sentence above is something a reader reproduces rather than
something two files assert about each other.

`stage_classb.build()` now runs `self_test()` before it emits a row. That
function's docstring had said since it was written that its gates "run on every
build"; in fact they ran only under `python features/build/stage_classb.py`,
because `build.py` called `build()` and never `self_test()`. They need no
network, so there was never a reason for that.

### Changed — the curator worklist now carries the evidence, not just the questions

[`features/PROPOSED.md`](features/PROPOSED.md) was a list of open questions. It
is now a list of decisions: every one of the 21 rows it was written for carries
the primary source that settles it, what that source settles, and a
recommendation — *sign*, *withdraw*, or *your call* with the options and their
consequences spelled out. Nineteen are recommended for signature, three of those
only after reading a named paragraph; one was recommended for withdrawal and has
since been withdrawn, so the worklist is 20 rows. Three naming and scope
questions and three unadjudicated patent flags are collected at the top as the
things no further research can decide — there were four, and the CMV question is
the one that is now closed. Its
*Claims*, *Anchor*, *Sources* and witness lines are read out of `features.tsv`
rather than retyped, so they cannot drift from the table, and it still carries no
digests for the reason it always did.

**One row was recommended for withdrawal: `PLF:4006`, the CMV enhancer** — a
recommendation, because withdrawing a row is a curator's call too. *He took it;
see* Removed *above.* Its
`notes` sent the reader to "the promoter row above" for why the two halves of the
584 nt block ship together; that row is `PLF:4005`, which the
extent-corroboration rule refused in 0.6.0, so the table is in the state the note
forbids. A user annotating a pcDNA3-type CMV region sees the upstream 380 nt
light up and the promoter half stay dark, and `Db::absent_common_kinds` cannot
say so, because `PLF:4000` and `PLF:4001` supply the literal `promoter` key it
probes for. The evidence turned out to say something stronger than the note did:
of the six records this stage fetches that contain the 380 bases, the only three
that draw the 380/204 split are the three already identified as SnapGene-shaped,
and every submission that is *not* SnapGene-shaped annotates the region as one
element and calls it a promoter. Boshart et al. 1985 (PMID 2985280) put the
enhancer at −524..−118, which straddles the split on either numbering
convention, so no primary source draws this edge either. `PROPOSED.md` sets out
all three options — restore the promoter row, re-cut to one 584 nt row, or
withdraw — with what each costs.

### Fixed — nine rows carried a sentence the evidence does not support

`SIGNOFF.tsv` defines a signature as a human who "wrote or checked its
description from the primary source", so a description written from nothing in
particular is precisely what a signature is supposed to catch. These were caught
before anyone signed, which is the order that rule exists to produce. Each is a
`description` or `notes` change on an unsigned row; no signed row's prose moved.

- **`PLF:4008` rrnB T1 — "nothing in the primary source says 'T1'" is false.**
  Brosius 1984 (PMID 6202587), who sequenced this operon and built the pKK
  vectors these extents come from, reports that T1 and T2 "each function
  separately in vivo"; Orosz et al. 1991 (PMID 1718749) subcloned them
  individually and found "T1 and T2 are both efficient terminators in isolated
  forms" — and that paper is in `J01695`'s **own reference list**. The narrower
  true claim, that the record's *feature table* annotates no terminator, is what
  the row says now. Its "rival extents run 43 to 98 nt" also does not survive
  re-measurement: no rival longer than 44 nt exists in anything checked, and the
  lone 43 nt feature is `U13859` annotating the same bases `rrnB T1` three times
  at 44, 44 and 43 — one submission disagreeing with itself.
- **`PLF:4000` T7 promoter — a note that pointed at evidence which is not
  there.** It said a 20 nt convention "is measured against this row in the
  witness offsets above"; all four offsets above are `5'+0/3'+0`, and no 20 nt
  form exists anywhere checked. The rivals that do exist are 19 nt and 21 nt,
  sit in other rows' records, and are now named. The description also called the
  row "the 17 bp class III promoter" while its own caveat described it as the
  −17..−1 part with +1 excluded; the description now says which. Added: across
  all 17 T7 promoters the anchor annotates, every column from −17 to −3 agrees
  in 14 of 17 records or better while no column from −24 to −18 exceeds 12 of 17
  — a better 5'-edge argument than the row was making.
- **`PLF:4007` T7 terminator — "neither is wrong" understated the row.**
  Macdonald et al. 1994 (PMID 8158645) put termination "at a 3' G residue just
  downstream of the U run"; that G is 24210, this row's last base and the single
  coordinate the anchor annotates as Tphi. Only the 5' edge is a convention, and
  the two rival 48 nt forms are not equally defensible. A claim that some deposit
  gives the name to a different part of the T7 genome was removed: no record
  checked shows it and none was ever named.
- **`PLF:4006` CMV enhancer — a rival "378 nt convention" attributed to a record
  that was neither named nor retained.** Not re-derivable from anything in the
  repository, so it is gone rather than left standing.
- **`PLF:1016` bsr — the organism conflict is resolved and written in.** UniProt
  said *Bacillus cereus*, ENA `S81409` said `/organism="Escherichia coli"
  /strain="TK121"`, and the row named no organism at all. The paper the record
  was created from (Kobayashi et al. 1991, PMID 1368770 — no PubMed abstract, so
  the full text is the only route) says the authors isolated a blasticidin S
  resistant *Bacillus cereus* K55-S1, took the plasmid pBSR8 from it, subcloned
  the gene into pUC19 as pTK17, and grew pTK17 in *E. coli* TK121. **The
  `/organism` is the expression host.** Four corroborations re-derived from the
  record itself: the promoter coordinates a 1998 *Bacillus* paper reports
  (91TTGATC, 113TAAAAT, start at 125) are exact in `S81409`; those are σ^A/σ^B
  promoters and *E. coli* has no σ^B; the CDS is 37.4 % GC with 25.5 % at third
  positions; and the record's ends are the paper's NdeI and HincII sites. The
  organism is written into the description in a sentence of its own, because the
  obvious phrasing is a five-token run of exactly the shape SOURCING.md §0.4
  hard-fails — the same trap `PLF:1015` was already rewritten around.
- **`PLF:1014` pac — the pinned record is flagged `UNVERIFIED_ORGANISM` by the
  archive and the row said nothing about it.** `M25346.1`'s own first line reads
  "UNVERIFIED:", its comment says GenBank staff could not verify the source
  organism, the sequence and/or the annotation, and `P13249` has exactly one EMBL
  cross-reference. The flag is discharged — the organism from Lacalle et al. 1989
  (PMID 2676728) and the `ATCC:12461` culture collection, the extent from that
  same paper's "600-nt open reading frame, starting with an ATG codon", the
  sequence from the stage's forced translation — but a signature that does not
  mention the flag would be a signature saying a curator read the record and did
  not notice its first line.
- **`PLF:1019` LEU2 — presented as uncontested; it is not.** Every pRS vector in
  INSDC carries a LEU2 that differs from this row (`A69V`, `N300D`; plus `G78A`
  and `V195L` in pRS405 and pRS425), and those five records are **one submitter
  on two consecutive days in November 1993** — a single vector series that does
  not agree with itself. `A69V` is also in `X03840`, a genomic record, so part of
  it is allelic rather than error.
- **`PROPOSED.md`'s own instructions for rejecting a row were wrong**, and wrong
  about the thing that matters most right now. "Dropping one does not renumber
  anything after it" is true of a row *dropped by a check* — the five refused
  Class B elements keep their index and are re-measured every build — and false
  of an item *deleted from a stage's `ITEMS`*, because both stages allocate
  `PLF:{ID_BASE + i}` from the tuple index. Measured: deleting the CMV enhancer
  would make `PLF:4006` mean the T7 terminator and would shift four more ids.
  `build.py` catches it and refuses to write, so the failure is loud, but it is
  still a failure. `stage_classb.ITEMS` now carries a comment recording this at
  the point where somebody would reach for the delete key. That section said the
  gate it describes did not exist yet; it does now, and it is the `withdrawn`
  field and its test under *Added* above.

### Added — primary citations for rows that had none

- **`PLF:4009` rrnB T2** now cites Brosius 1984 and Orosz 1991 for the *name*, so
  only the extent rests on vector records. **`PLF:4010` bGH poly(A)** now cites
  its anchor's own publication (Gordon et al. 1983, PMID 6357899) and Goodwin &
  Rottman 1992 (PMID 1644817), and records that the anchor's `exon 2138..2439`
  places the cleavage site 18 bases after the hexamer — which turns "enough
  flanking sequence" from a judgement into a measurement, with the positioning
  element those authors require sitting inside the row with 84 bases to spare.
- **`PLF:4001` SP6 promoter** was described as bounded by analogy to T7. It is
  not: the anchor record's own publication (Dobbins et al. 2004, PMID 15028677)
  identifies ten SP6 promoters and publishes the consensus
  `KAWTTARGKGACACTATAG`, whose −17..−1 window resolves to this row base for
  base, and Brown et al. 1986 (PMID 3010240) fixes the register at `CACTA` =
  −7..−3, which this row satisfies. **`PLF:1015` bsd** now cites Kimura et al.
  1994 (PMID 8159161) for "an open reading frame of 393 bp, encoding a
  polypeptide of 130 amino acids" — this row's extent to the base.

Literature was checked against PubMed and Europe PMC; no source on
`SOURCING.md`'s NO-GO list was consulted. Two papers carry no PubMed abstract
and are marked as such wherever they are used: Dunn & Studier 1983, which
nothing here relies on, and Kobayashi et al. 1991, read from the publisher's
full text.

## [0.6.0] - 2026-08-10

**Nothing in this release changes what Polylinker does to a sequence**, and the
89 feature records `pl annotate` searches by default are unchanged, byte for
byte, with every signature still valid. If you upgrade and change nothing else,
nothing you get out of the tool moves. The database grew from 89 rows to 110;
the 21 new rows are `proposed`, which means a program put them there and no
human has read them, so they are searched only if you ask for them by name with
`--include-proposed` or the equivalent tick-box in the app.

Those 21 are the story, and it has two halves that arrived four hours apart.

**The database got its first promoters, terminators and yeast markers** — 14
selection markers and 12 Class B regulatory elements, the first elements of
those classes it has ever held.

**Then a rule applied honestly took five of the twelve back.** `SOURCING.md` §4
has always required "≥2 independent GenBank exemplars *showing where depositors
actually place it*", and only the first half of that sentence was ever executed:
the build checked that two independent submissions held the *bases*, measured
where each drew the *edges*, wrote the answer into a note, and tested nothing. A
row could therefore ship `boundary_rule = consensus_of_insdc` on a consensus of
one, and four did. Making the second half executable refused `lac`, `tac`,
`trc`, the CMV promoter and the SV40 early poly(A) — each corroborated by
exactly one submission, which is one lab's opinion. Seven Class B rows ship as
`proposed`. That is the finding, not a shortfall.

Two limits belong up here rather than in a footnote, because a release about not
overclaiming cannot overclaim:

- **The new posture check does not detect a coordinate, and nothing here could.**
  It is a process rule: every build stage that emits a boundary must declare how
  it avoided taking one from a vendor, and the gate checks that the declaration
  exists and matches the code. The artifact the taint gate is pinned to has four
  columns — no sequence, no coordinates, no lengths — so there is nothing in it
  to compare an extent against. The taint gate remains a check on the
  **description** column and must not be described as a check on the database.
- **There is a demonstrated false negative inside the shipping witness set.**
  `OP697991.1`, one of the two submissions corroborating `PLF:4006`, carries
  `/note` text byte-identical to the flagged `MH325107.1` but for the token
  `label: `. The screen passes it — correctly by its own rule, wrongly as a
  matter of fact. Widening the screen to that shape was deliberately not done,
  because honest records share it. So "2 independent submissions" must not be
  read as "2 SnapGene-free submissions".

### Added — 21 proposed feature records, and none of them ship yet

`features/features.tsv` goes from 89 rows to 110. **The 89 rows the tool
searches by default are unchanged, byte for byte; every one of their signatures
is still valid.** The 21 new rows are `proposed`, which means a program put them
there and no human has read them, so `pl annotate` ignores them unless you pass
`--include-proposed` and the desktop app ignores them unless you tick the box.
This is what "the tool may propose and never assert" looks like when it is
actually exercised, rather than when the table happens to be fully signed.

**14 further selection markers** (`PLF:1014`–`PLF:1027`), each verified by the
same chain as the existing natural-protein rows — translate the nucleotides,
require an exact residue-for-residue match to a UniProt canonical, cite the
depositor's own coordinates — and each dropped rather than corrected if any leg
disagreed: `pac`, `bsd`, `bsr`, `dhfrI`, `URA3`, `LEU2`, `HIS3`, `TRP1`, HSV
`TK`, mouse `Dhfr`, `gpt`, `bar`, `pat` and `rpsL`. They give the database its
first yeast markers of any kind. They **narrow** the eukaryotic selection-marker
gap `features/SOURCING.md` names as Gap 6 without closing it, and the earlier
draft of this entry said "close", which was an overclaim on two counts: three of
Gap 6's five named markers were already signed before today, the two these add
(`pac`, `bsd`) are `proposed` and so are searched by nobody, and Gap 6 also names
the codon-optimised forms, of which this adds none — every row here is a native
CDS. Gap 6's own entry now records that.

**7 Class B regulatory elements** — the T7 and SP6 promoters (`PLF:4000`,
`PLF:4001`), the CMV enhancer (`PLF:4006`), the T7, rrnB T1 and rrnB T2
terminators (`PLF:4007`–`PLF:4009`) and the bGH poly(A) signal (`PLF:4010`).
These are the first promoters and terminators of any kind in the database. A
Class B boundary is a *convention* and not a fact, so each row ships a coordinate
slice of one named INSDC record, and two claims are re-checked on every build:
that at least two records **from different submitting addresses** hold those
exact bases, and that at least two of those submissions annotate a feature at
**exactly** the shipped extent. The second of those is new — see below — and it
is why this is seven rows and not twelve.

Three things that came out of building it and are documented rather than
smoothed over:

- **INSDC records carry SnapGene annotation, and the CI taint gate cannot see
  it.** ENA folds SnapGene's `/label` into the `/note`, so its editorial prose
  arrives through a source this project cleared. The gate compares descriptions
  and can never notice a *coordinate* arriving that way. The stage therefore
  reads no `/note`, `/label`, `/gene`, `/product` or `/standard_name` at all,
  and refuses to count a SnapGene-annotated deposit as an independent witness.
  Two of the seven surviving rows have a witness excluded on those grounds, and
  three more of the five that were refused did too.
  **This is no longer an open hole — see "the coordinate route" below.**
- **The taint gate fired for real, for the second time in this project's
  history**, on the blasticidin deaminase description, whose first draft shared
  a five-token run with their file. Nothing was copied; the row was rewritten
  anyway, because the rule is mechanical on purpose.
- **Nine more elements were worked up and are not here**, each with its reason
  recorded in `features/build/stage_classb.py`: T3, the SV40 early promoter, U6,
  H1, EF-1α, PGK, CAG and araBAD are held, and tetO/TRE is dropped outright
  because the name covers four unrelated elements. `SOURCING.md` budgets about
  forty Class B rows; seven is what survives both rules applied honestly, and
  that is the finding rather than a shortfall.

**A curator worklist, [`features/PROPOSED.md`](features/PROPOSED.md).** Twenty-one
rows nobody has read is a request for several hours of a specialist's attention,
and "here is the table, good luck" is not how to ask for it. The file gives each
row's claim, the accessions to check it against, the boundary chosen and its
basis, and the exact `--show` invocation — and it leads with the rows where the
exemplars *disagreed*, because those need a decision rather than a check: the two
T7-terminator forms that are offset from each other by one base, rrnB T1's rivals
running 43 to 98 nt, and `PLF:1016`, which should not be signed at all until an
unresolved organism conflict between UniProt and the ENA record is settled. It
carries no digests on purpose — `SIGNOFF.tsv` says signing a digest nobody has
read is not an attestation, and a worklist you could copy twenty-one hashes out of
without opening a row would be a machine for producing exactly that. It now also
carries the five elements that were refused, so the work of rescuing them is
asked for rather than left implicit.

### Added — the coordinate route, declared by every stage that could carry it

The bullet above says the taint gate cannot see a coordinate arriving from
SnapGene through INSDC. That sentence described an open hole for one release.
This closes it — and the first thing to say is what "closes" means, because the
obvious repair is not available and pretending otherwise would be the exact
defect this project keeps catching in itself.

**A coordinate-level taint check cannot be built here, and the reason is not
effort.** `features/build/insdc_posture.py` carries the argument in full and
`features/SOURCING.md` §0.6 carries the measurements. In short: the artifact the
gate is pinned to, pLannotate's `snapgene.csv`, is four columns of `sseqid`,
`Feature`, `Type` and `Description` — **no sequence and no coordinate**, so there
is nothing in it to compare an extent against. SnapGene's feature bases live in a
separate bulk asset that carries no licence and sits on a host the build refuses,
and fetching a complete copy of their extents in order to prove we did not copy
their extents would be a larger act of copying than the one being disproved. And
the sequences are biology: the T7 promoter is the T7 promoter, an exact match
proves nothing about copying, and a rule keyed on agreement fires on **84%** of
the distinct extents in a 481-record survey of this database's own witnesses —
100% for the rarer ones. That is a check that gets switched off in a week, which
is a check that proves nothing.

**So the enforcement is structural rather than statistical.** Every stage in
`build.STAGES` must declare `INSDC_POSTURE`, naming one of four postures and
saying in its own words what it does about a depositor's coordinates. The gate
refuses a stage that declares nothing — the same shape, and for the same reason,
as the existing rule that refuses a stage that does not declare its id block —
and then checks the mechanical half of whatever was declared:

- a `no_insdc` stage must name no INSDC host;
- a `no_feature_table` stage must name no record flat-file endpoint, because a
  feature table is only served by the flat-file view;
- a `feature_table_forced` stage must **name** the test that forces its extents,
  and the gate drives that test with a CDS that translates to its protein and one
  that does not;
- a `feature_table_convention` stage must name its SnapGene screen, which the
  gate drives against a record carrying the tell and one without, and its
  corroboration floor, which may not go below two.

Each of those was proven to fail by breaking the real tree seven ways — deleting
a declaration, adding an ENA fetch to the stage that says it makes none, pointing
a FASTA-only stage at a flat file, blinding the SnapGene screen, making it fire
on everything, lowering the floor to one, and neutering the translation check
that `stage_uniprot`'s whole posture rests on. All seven go red. The gate runs
inside `taint_gate.py` **before** the fetch, so it still reports on a day the pin
is unreachable, and it is now a step in `tools/ci.ps1` — the first half of the
taint gate to have a local twin, since it needs no network at all.

**Say plainly what this does not buy.** It does not show that no coordinate in
`features.tsv` agrees with SnapGene's, and nothing in this repository can. It
shows that no stage reached the table without a human answering the question, and
that four named mechanisms still work. `SOURCING.md` §6 now forbids describing
the taint gate as a check on the database: it is a check on the description
column, and saying more than that is the overclaim.

### Changed — a Class B row must now show that depositors put the edges where we did

`features/SOURCING.md` §4 has always asked for "≥2 independent GenBank exemplars
**showing where depositors actually place it**", and only the first half of that
sentence was executed. `stage_classb.verify()` required two independent
submissions to *contain the bases* — a fact about the sequence, not about a
boundary — then measured where each of them drew the edges, wrote the answer into
`notes`, and tested nothing. A row could therefore ship
`boundary_rule = consensus_of_insdc` on a consensus of one, and four did.

`MIN_PLACEMENTS` makes the second half executable: two independent submissions
must annotate a feature at **exactly** the shipped extent, edge for edge, with no
tolerance — a tolerance would be a knob to widen until the row passed. **Five of
the twelve candidates fail and do not ship:**

| Row | Element | Submissions holding the bases | Placing it exactly |
|---|---|---|---|
| `PLF:4002` | lac promoter | 4 | 1 |
| `PLF:4003` | tac promoter | 3 | 1 *(two records, one lab)* |
| `PLF:4004` | trc promoter | 2 | 1 *(the anchor itself)* |
| `PLF:4005` | CMV promoter | 3 | 1 |
| `PLF:4011` | SV40 early poly(A) | 3 | 1 |

Two things about that table. The rows stay in the stage's allow-list rather than
moving to `HELD`, so they keep their ids, are re-measured on every build, and come
back by themselves the day a curator cites evidence that corroborates the extent
— or re-cuts it to one the evidence already corroborates. And `PLF:4005` is worth
looking at twice: the CMV promoter's *only* exact corroboration is `LC897329`, a
record whose feature table is SnapGene's Common Feature naming throughout with no
`label:` tell in it. That is the blind spot at the top of this entry, biting a
real row — caught by a rule that names no vendor and reads no `/note`.

**On review, that blind spot is wider than the sentence above admits, and the
one row it was said to spare is not spared.** `PLF:4006`, which ships, has
exactly two corroborating submissions and **both** carry a SnapGene fingerprint
the screen passes. `LC897329.1` is the naming case again. `OP697991.1` is
sharper and was measured on 2026-08-10: four of its `/note`s — over the CMV
enhancer, the CMV promoter, the ColE1 origin and the AmpR CDS — have a
descriptive half **byte-identical** to the corresponding `/note` in
`MH325107.1`, the record the screen does flag, in the same two-part shape ENA
emits when it folds a `/label`, differing by the token `label: ` and nothing
else. Neither observation refuses the row — an extent two independent
submissions publish is attested whatever tool drew it — but "2 of 3 independent
submissions" must not be read as "2 of 3 SnapGene-free submissions".
`features/PROPOSED.md` and `features/SOURCING.md` §0.6 now say so, and §0.6 also
now separates which of its four rejection grounds a reader can re-derive from
this tree (point 1, re-measured against the pinned artifact, along with §0.5's
figures) from which they cannot (points 3 and 4, a one-off 481-record survey
whose record list was not preserved).

**This rule is not a taint check and must not be described as one.** It cannot
show that an extent came from SnapGene. It answers the narrower question that is
answerable: did our own evidence force this extent, or is it one lab's opinion?

### Fixed

- The desktop app's "no promoter is in this database yet" line is computed by
  probing the table for literal INSDC feature keys, and the twelve new rows were
  invisible to that probe for the length of one build because they used the
  current INSDC spelling (`regulatory`) rather than the retired one. The app
  would have gone on saying "no promoter" after promoters were signed off. The
  rows now carry `promoter`, `enhancer`, `terminator` and `polyA_signal`, and a
  test pins the disclosure to the table it describes in both directions.
- Two counts in `features/README.md` were wrong before this change and are
  corrected by measurement: the alias-collision table said twelve colliding
  strings when there were more, and listed `smR` as resolving to two records
  when it resolves to three.
- **Every Class B row told the curator the anchor record annotated nothing near
  it, when what had been measured was narrower than that.** The sentence read
  "ANCHOR RECORD'S OWN ANNOTATION within 80 nt of this interval: none", but
  `parse_embl()` keeps only regulatory-type feature keys, so a CDS, gene or exon
  over the interval was never looked at and could not appear. It is not
  hypothetical: `X17403.1` annotates `CDS complement(173505..>173909)` straight
  across `PLF:4005`'s interval, and the row said "none". The rows are still
  right — a CDS is not a rival promoter boundary — but a curator reading "none"
  had been told the region was bare, which it is not. The note now says which
  keys were counted and that it is silent about the rest.
- `features/README.md` described `build/stage_curated.py` as "Stage 5" when it
  is and always was Stage 4. Harmless while there were four stages; actively
  misleading the moment a real Stage 5 (`stage_classb.py`) landed underneath it.
  Both rows are now in the file table, with the correct numbers.

## [0.5.0] - 2026-08-10

**Nothing in this release changes what Polylinker does to a sequence.** No
parser, no renderer, no digest, no annotation and no file format is touched.
What changes is what "CI is green" is worth: for six releases it was worth
nothing, because the gate `docs/RELEASING.md` names before tagging was run by
no workflow at all, and the number that gate produces was a terminal on one
Windows machine.

One thing here is user-facing, and it is first for that reason.

### Fixed

- **The Windows installer refused a correct download as incomplete.**
  `Install-Polylinker.ps1` checks the extracted files against
  `SHA256SUMS.txt` before it installs anything, and it built the relative name
  of each file by subtracting the source directory's path from
  `FileInfo.FullName` — two strings produced by two different normalisers.
  `Resolve-Path` hands back the string it was given; `Get-ChildItem` reports
  what the volume holds. Where those differ, every name came out wrong, nothing
  matched the manifest, and the installer stopped with *"this copy is
  incomplete — 21 file(s) the manifest lists are not here"* over a download in
  which all 21 were present.

  They differ in two measured ways. An **8.3 short name** anywhere in the path
  is shorter than what the volume reports — `C:\PROGRA~1` is 11 characters and
  `C:\Program Files` is 16 — so the subtraction left the tail of the source
  directory welded to the front of every filename. A **trailing separator**
  needs no alias at all and reproduces on any machine: `Substring(len + 1)` cut
  one character too many, and `a.txt` came back as `.txt`.

  Which invocations were exposed is measured rather than assumed, because the
  first draft of the fix's own comment got it wrong. Running the installer the
  way `README-WINDOWS.txt` and `Install.cmd` tell you to is **safe**: `-Source`
  defaults to `$PSScriptRoot`, and PowerShell hands that over already expanded
  and without a trailing separator, on 5.1 and on 7 alike. What is exposed is
  an explicit `-Source`, and the obvious thing a wrapper script passes to it is
  cmd's `%~dp0`, which carries both defects at once — the alias verbatim and a
  trailing backslash always.

  The same wrong subtraction sat in two more places in that file, where the
  failure would have been worse than a refusal: an uninstall decides whether to
  copy itself out of the directory it is about to delete by asking whether
  `$PSCommandPath` is under `$Prefix`, and `Stop-IfRunning` decides whether the
  app is running out of the prefix by comparing against `Process.Path`. Windows
  reports both of those expanded, so a short `$Prefix` answered "no" to both:
  the uninstaller would have been deleting its own running file, and an upgrade
  would have overwritten files mapped into a running process instead of
  refusing. All of it now goes through one `Get-DirectoryPrefix`.

- **What did *not* reach a shipped release, said plainly rather than left to be
  inferred.** Two defects of the same family turned up beside the one above,
  and the reason neither touched a published artifact is where they could
  reach, not luck.

  The same path arithmetic was in `tools/release.ps1`.
  `.github/workflows/release.yml` invokes that script as
  `release.ps1 -Out dist` — a relative path, resolved under the runner's
  workspace, which carries no 8.3 component — so the defect had nothing to bite
  on there. Checked rather than reasoned: the Windows archive published for
  v0.4.0 verifies against its own manifest, all 21 entries matching the bytes.

  Separately, `RUSTFLAGS: -D warnings` set at workflow level was *replacing*
  `.cargo/config.toml`'s `-C target-feature=+crt-static` rather than merging
  with it, putting a VC++ redistributable dependency back into anything built
  under it — a dependency whose installer needs administrator rights, which the
  user this project is aimed at does not have. That was in `ci.yml` only;
  `release.yml` has never set `RUSTFLAGS`, so no shipped binary ever carried
  it. Both were found because the gate started running in the places where the
  defects were reachable.

- **The gate now runs in CI, on all three platforms.** `tools/ci.ps1` is a
  72-step gate and [`docs/RELEASING.md`](docs/RELEASING.md) names it as the
  thing to run before tagging. **No workflow invoked it.** It appeared in
  `.github/workflows/ci.yml` six times and every one of those was on a comment
  line, so from v0.1.2 until now it had been failing on a clean tree and
  nothing reported that: 0.1.2, 0.1.3, 0.2.0, 0.3.0, 0.3.1 and 0.3.2 were all
  tagged with it red. `ci.yml` has a `gate` job now — `windows-latest`,
  `ubuntu-latest` and `macos-latest`, `fail-fast: false` — and a red gate is a
  red build.

  The three legs of that job, read out of the per-leg ledgers of run
  31364592031 — this release's parent commit — rather than described:

  | Leg | Steps | Ran | Skipped | `not windows` | corpus |
  |---|---|---|---|---|---|
  | `windows-latest` | 72 | 67 | 5 | 0 | 5 |
  | `ubuntu-latest` | 72 | 60 | 12 | 7 | 5 |
  | `macos-latest` | 72 | 60 | 12 | 7 | 5 |

  **The port did not weaken the Windows leg**, which is the leg that has been
  standing between a mistake and a published release: 67 steps ran before it
  and 67 ran after, the same five skipped for want of a corpus both times, and
  the two lists of what ran differ on two names — both of them renames of a
  step that ran either way. It was done by spelling rather than by skipping: an
  `$exe` suffix for 25 hardcoded `pl.exe` sites, `[IO.Path]::GetTempPath()` for
  17 unguarded `$env:TEMP` uses, a three-way `.pyd`/`.so`/`.dylib` table,
  `tar$exe`. Two things nothing had ever exercised now run for real: the tar.gz
  writer, on the two platforms whose users are the reason it exists; and the
  zip's "entry order is sorted" claim, which off Windows is fed ext4 and APFS
  enumeration orders instead of only NTFS's.

- **The gate's skips are checked, not printed.** A step whose tooling is
  missing SKIPs rather than failing — right on a workstation, dangerous on a
  runner, and the reason the gate passed on the author's machine for six
  releases *with six steps skipped*: the wasm-versus-native comparison, both
  chromatogram oracles, digest versus Biopython on real plasmids, the
  Rust-versus-Python reader, and the MSI install test. The job now installs
  what a runner can be given — eleven Python oracles, Node 24, the
  `wasm32-unknown-unknown` target, the TypeScript toolchain, and on Windows the
  WiX Toolset and a `dist/` for the MSI steps to read — and passes
  `-ExpectedSkips`, a new parameter that fails the run on any difference from
  [`.github/ci-expected-skips.txt`](.github/ci-expected-skips.txt) in either
  direction. Set equality, not a count: a count of five is satisfied by the
  wrong five skipping, and a name matching no step in the gate is itself a
  failure, so a renamed step cannot drift off the list unnoticed.

  Five of the six skips remain, and they are one skip five times: those steps
  need real `.dna` and `.ab1` files, and a lab's plasmids are not ours to
  publish. It is the same five on all three legs, which is why that file needs
  no platform column. The sixth, the MSI install-and-uninstall test, now runs
  on every push — the only check that puts a real `msiexec` against a real
  registry, and until now its first execution on any given release was after
  the tag.

- **Three gate steps that had never run in CI at all now do**, because each
  needs a Python package the existing `oracles` job does not install: *gel
  calibration spline vs SciPy*, *PDF is a PDF, and matches the SVG* against
  PyMuPDF and pypdf, and *the release workflow parses and covers three
  platforms*, which parses `release.yml` with PyYAML.

- **`Get-DirectoryPrefix` is one function copied three times, and that is now
  checked rather than asserted.** It is defined in `tools/ci.ps1`,
  `tools/release.ps1` and `tools/installer/Install-Polylinker.ps1`, and the
  duplication is legitimate and stays: `release.ps1` copies the installer into
  the archive root as a single flat file with nothing beside it to dot-source.
  What was missing is that nothing compared the copies while `ci.ps1`'s comment
  called `release.ps1`'s the "identical function" — prose standing in for a
  check, sitting directly on top of the defect above. Two steps hold it now.
  One finds every definition under `tools/` by parsing rather than by path,
  and compares the bodies as token streams, so a reformat is invisible and a
  dropped `TrimEnd` is not. The other is the one that matters, because three
  identically wrong copies pass a drift guard: it drives each extracted
  definition over a **real** 8.3 alias, discovered on the machine rather than
  minted, and requires the arithmetic it replaced to still come back mangled on
  the same input.

### Added

- **A `reconcile` job, because no leg of a matrix can see the others.** Each
  leg writes a ledger — every step, `ran` or `skipped`, and the reason a skip
  gave for itself — and `tools/reconcile-ledgers.ps1` compares the three. It is
  the only place a step that skipped on **all three** platforms can be seen
  (`not windows` on two legs is only honest if the third ran it), and the only
  place a step deleted from one leg at file level can be seen. The job runs the
  reconciler's own self-test first, because a reconciler whose parser stopped
  matching reports every push clean.

### Changed

- **`ci.yml`'s `test` matrix drops `windows-latest`** and runs on
  `ubuntu-latest` and `macos-latest`. That leg was the one piece of genuine
  duplication, because the `gate` job runs those steps on the same runner
  image. What it is **not** is a copy of the gate, and the cost is stated in
  the workflow rather than glossed: every cargo invocation in `test` passes
  `--locked` and no cargo invocation in the gate does, so a rewritten lockfile
  is a red build there and nowhere else, and `pl-draw/tests/memory.rs` and
  `pl-features/tests/schema_pin.rs` are run by no step in the gate at all. Lock
  drift is a property of the tree and not of an operating system, and the two
  suites count `Layout` sizes and compare a Python file to a Rust constant, so
  none of the three has an operating system in it; they run on two platforms
  now instead of three. `tools/ci.ps1`'s step *every integration suite is run
  by a gate* is what keeps that division honest — it reads the tree, and every
  `tests/*.rs` target under `crates/` and `bins/` must be run, whole, by one of
  the two files.

### Known limitations at this release

- **A mislabelled skip is held by review alone, and nothing mechanical catches
  it.** The skip rules catch a step that stops running for a reason *nobody
  declared* — a wheel that stops building, a `pip install` line edited in a
  hurry — which is the failure that actually happens. They do not catch a human
  hand-writing `WindowsOnly` into a portable step's own precondition. Such a
  step goes on running on Windows, so every per-leg rule is satisfied (the
  reason was declared, it agrees with the platform, it is not a corpus skip)
  and so is the reconciler's requirement that every step ran somewhere. Two
  platforms of coverage disappear and nothing anywhere turns red.

  This is measured, not reasoned. It was done on purpose to *gel calibration
  spline vs SciPy* and pushed: run 31361991651's Linux leg
  reported **eight** `not windows` skips where seven is the honest count, and
  it passed. The sentence that used to say there was "no line anybody can add
  anywhere to quiet a platform skip" was wrong and now says what is true: no
  line in `.github/ci-expected-skips.txt` can, which is what the split between
  that file and the preconditions buys, and it is the whole of what it buys.
  What holds the rest is that `WindowsOnly` is one greppable identifier in
  seven places and `tools/ci.ps1` is reviewed. The two obvious repairs — a
  file per platform, or a platform column — are both a second, unverified claim
  about where a step *ought* to run laid on top of the one thing that is
  actually verified, which is what it did; `.github/ci-expected-skips.txt` sets
  that argument out at length.

## [0.4.0] - 2026-08-09

The minor number moves rather than the patch because the window looks
different on launch, the minimum window size changed, and a font was added
to the archive. Nothing about a file Polylinker reads or writes changed.

### Changed

- **The desktop window has a design system.** Colour, spacing, typography,
  rounding and shadow are ported from the author's other eframe application so
  the two programs read as one piece of software: an orange accent taken from
  Polylinker's own icon, softer and more downward shadows, 6 pt widgets in 10 pt
  windows, and slightly larger body text. No panel moved, no tab was renamed and
  no screen was reorganised. Two of the ported colours were overruled by
  measurement rather than copied — see *Accessibility* below.
- **The light/dark choice is remembered.** The toolbar switch has been there for
  a while and died with the process, because this application deliberately does
  not use `eframe`'s own persistence. It is now written to the layout file on
  the click, and **Help ▸ Follow the desktop's theme** puts it back to following
  the system, which egui's two-state switch offers no way to do.
- **The minimum window is 990 × 560, up from 880 × 560.** This is the design
  system's one measured cost and it is paid rather than hidden. The toolbar's
  fixed run is priced in button padding and button text size, and the ported
  values are larger than egui's on both; measured through the real toolbar at
  880 pt, the run's right edge moves 86.9 pt and the title block it leaves room
  for falls from 193 pt to about 48, at which point the status line is not
  truncated but **absent** — and the status line is where an export says "this
  drops 9 feature(s) and the topology". The width was swept in 10 pt steps: the
  status returns at 960 and reaches the length the old minimum used to show at
  980. The default size is unchanged at 1280 × 840.
- **A destructive button stays legible while it is pressed.** *Delete feature*
  carried its red in the string, which pins one colour through every widget
  state; with the accent behind a pressed control that measured 2.10:1 in dark
  mode. The red now lives in the widget's resting ink, and egui's own label
  colour takes over under the pointer.

### Added

- **Inter SemiBold**, SIL OFL 1.1, for headings only — window and dialog titles.
  It is in a font family of its own and in neither text chain, on purpose: its
  capital `I` and its lowercase `l` are the same bare stem, and this
  application's proportional text is enzyme names like `AflII` and `BspLU11III`.
  IBM Plex Sans keeps the body. Inter Regular is not vendored at all. The
  archive now carries eight font licence texts, and `licences/Inter-OFL.txt` is
  required by name by the packaging gate.

### Accessibility

- **Every ink the window paints is now measured against every surface it is
  painted on**, in both themes, by a test rather than by a review. Two of the
  ported tones did not clear WCAG AA and were moved: the light striped-row
  colour, on which the feature editor's inverted-span sentence measured
  **4.48:1**, and the dark one, on which the tertiary text role measured
  4.5007:1 — passing by seven ten-thousandths, which is not a margin. Both moved
  one notch towards their own panel, to values already in the design system.
- The accent is a pair, because one value cannot do both jobs: `#E69F00` is
  2.25:1 on white and unusable as light-mode ink, so light mode uses
  `rgb(140, 97, 0)` — the same colour scaled, hue unchanged — and the two swap
  roles with the theme. Every accent fill takes its foreground by measurement.

### Fixed

- **Layout tests were measuring a `Style` the binary does not install.** The
  test context installed the shipped fonts and then left egui's default spacing
  and text sizes in place; making it honest turned eleven green tests red at
  once, two of them against the style 0.3.2 shipped. All eleven were traced
  rather than re-baselined: six read the sequence grid's geometry from an
  unsettled first frame and then clicked with it, landing up to three bases
  away; one read a window's footer before the window had finished growing; one
  asserted a panel width egui had stopped being able to grant; one duplicated a
  `pl-doc` invariant through a 420 pt viewport.

## [0.3.2] - 2026-08-09

A 32-agent audit raised 25 findings; 8 were refuted and 17 survived an
independent skeptic. All 17 are fixed here, each with a test shown to fail
against the unfixed code first.

### Fixed

- **Saving one tab could destroy the work in another, silently.** With two
  documents open and both edited, answering the quit dialog's
  "Save as .dna…" closed the window over every *other* dirty tab — and
  deleted their crash drafts on the way out. No prompt, no draft, no way
  back. **If you have been running 0.3.1 or earlier with more than one tab
  open, this is the reason to update.**

  The guard asked only the *active* document, which had been marked clean
  four lines earlier, so the check could never fail. The GenBank arm of the
  same dialog already did the right thing and its comment described this
  exact hole; one arm had been fixed and its sibling left. The condition now
  lives in the single place both arms pass through, because two half-guards
  that must agree is the defect, not the asymmetry.

- **Closing a tab with unsaved work then quitting lost it.** `Ctrl+W`
  deleted the tab's crash draft and hid its edits from the quit guard, so
  the app exited without asking. Closed-but-dirty tabs are now put back on
  the bench before the guard reads it.

- **A `.dna` file could round-trip to a different molecule.** Strings taken
  from the input were written into GenBank lines where column position
  carries meaning, so a name or description containing the wrong character
  moved the fields around it. Everything interpolated into a line is now
  flattened first, and what had to change is reported rather than done
  quietly.

- **`join(complement(a),complement(b))` was rewritten as
  `complement(join(a,b))`.** Those name different spliced products. A
  feature read from one file and written back out could describe a
  construct nobody built.

- **A `>` inside a sequence split an exported FASTA into two records.**

- **Methylation was judged at an enzyme's first site only**, so rotating a
  plasmid changed whether the app said Dam or Dcm blocked it. The verdict is
  now per site, and says how many of how many are affected.

- **`pl-clone::digest` gave different fragments depending on the order the
  enzymes were passed** — blunt or sticky ends could turn on an argument
  order the caller has no reason to think matters.

- **Four buttons' explanations were unreachable**, a failed update download
  left its partial file behind while reporting that nothing was written, and
  a superseded Sanger comparison ran to completion because its cancel flag
  never reached the worker.

### Changed

- **Gate steps that were running nothing now run something.** 52 of the 53
  command-line integration tests were executed by no gate; one step filtered
  on a test name that no longer existed and ran zero tests; a `pl-fileio`
  suite was referenced nowhere. A step that runs no tests and one that runs
  53 look identical in a green log, which is how they survived.

- Four comments claimed guarantees the code no longer honoured, including
  `pl-draw` promising byte-identical output on every platform while its own
  raster module records that the PNG path is not.


## [0.3.1] - 2026-08-08

### Fixed

- **Four greyed-out buttons could not say why they were greyed out.** They
  attached their explanation with `on_hover_text`, which on a *disabled*
  widget shows nothing at all: egui 0.35 routes it through
  `Tooltip::for_enabled`, and that opens the popup only when the response is
  enabled. So every one of those sentences was written in the branch that
  runs precisely *because* the button is grey, and could never be read.

  It is the shape of mistake that leaves nothing behind — no warning, no
  wrong answer on screen, just a hint nobody can reach. The user hovers the
  grey button asking what to do, and the app says nothing back. Now
  `on_disabled_hover_text`, in "Design primers…", "New feature…", "Copy
  rev-comp" and the primer designer's "Add to document". "Copy protein" in
  the same row was already right, and carried the comment explaining the
  trap that its three neighbours had fallen into.

### Changed

- **The feature-database gate no longer depends on anyone else's uptime.** The
  CI step that proves the build never writes `features/SIGNOFF.tsv` did it by
  running the real build against live EBI, NCBI, UniProt and RCSB. On
  2026-08-07 EBI timed out twice in a row and main went red twice, both times
  for a reason no commit under test had caused. The step was not wrong to fail —
  the build had died before reaching the writer, so it had genuinely checked
  nothing — but a red gate that says nothing about the code teaches people to
  ignore red gates, which costs more than the check is worth.

  The rule is unchanged and is now proved twice, offline, on every push.
  `features/build/check_writer.py` drives the real writer over the real shipped
  rows with the real signatures applied; the end-to-end run sets `PLF_OFFLINE=1`
  so `fetch` refuses the network instead of hoping for it. Neither can be turned
  red by a third party, and both now run in `tools/ci.ps1` too, which they could
  not when they needed a network.

  The check got stricter in three places on the way. It now also proves the
  build *reads* the sign-off — the step was named "The build reads SIGNOFF.tsv
  and never writes it" and only ever tested the second clause, so a build that
  ignored the file entirely passed it. It looks for a stray `SIGNOFF.tsv` at any
  depth and in any case, where the old `test ! -e` saw neither a subdirectory nor
  `signoff.tsv` on a case-insensitive filesystem. And it plants five misbehaving
  writers and requires itself to catch each one before it will certify the real
  one, then requires itself to pass a writer that does nothing wrong — because a
  check that fires on everything proves as little as one that fires on nothing.

  Verification against live sources still happens, in a new scheduled
  `features (live sources)` workflow that also reports whether the shipped table
  still reproduces from upstream. It is not a gate, and an unreachable source
  there is reported as *not checked* rather than as a failure.

- **`build.py` tells an outage apart from a defect.** A failed fetch raised
  `SystemExit`, which killed the interpreter before `write_outputs` ran and was
  indistinguishable from `check_fetch_host` refusing a source no licence covers.
  It now raises `SourceUnavailable`, the stage drops out, the build reaches its
  writer and exits **3** with `build-source-unavailable`. A sourcing violation
  still stops everything, and an HTTP 4xx — a withdrawn or mistyped accession,
  which *is* this repository's defect — is now fatal instead of being retried
  four times and then excused as an outage. Nothing can ship from a short build:
  the id-stability audit already refuses to overwrite a published table when rows
  go missing, and that, not the abort, was always what protected it.

  A per-host circuit breaker stops a real outage costing four timeouts on every
  one of the hundreds of accessions `stage_curated` requests; the first failure
  against a host is remembered and the rest give up at once.

## [0.3.0] - 2026-08-07

### Added

- **Linear molecules get a linear figure.** Exporting a PCR product, a
  linearised vector, a gene fragment or a gBlock produced a C-shaped ring with a
  gap in it. The gap was correct about topology and was still the wrong picture:
  every FASTA and every assembly opens linear, so this was not an edge case. A
  linear molecule now exports as a horizontal track — features as boxes and
  arrowheads on a band the backbone runs through, cut sites as ticks with their
  coordinates above, a ruler beneath — in SVG, PDF, EPS and PNG, from `pl
  export` and from the app's Map items alike. **Circular molecules are
  untouched, byte for byte.**

  It is one geometry, not a second renderer. The figure is built as the same
  `Scene` the ring is, out of the same three primitives, so no writer changed and
  the app's on-screen `Scene` painter can consume it; labels are packed by the
  same isotonic regression, features resolved by the same `ranges`/`mid_base`
  pass, and a base's position along the molecule now comes from one function that
  the ring multiplies by a turn and the track by its width. What is new is a band
  of label rows above the track, filled nearest-first, where a row that cannot
  hold a label hands it to the next row out — so a polylinker's twelve cutters
  cost nothing rather than eleven names.

  The shape is `Options::shape`, which defaults to asking the molecule. Both
  overrides have a user and neither was reachable before: `Shape::Linear` on a
  plasmid is the cut map, and says so in the figure and in `Report::cut_open`,
  because nothing in the geometry of a track distinguishes a linearised plasmid
  from a molecule that really is a line. `Shape::Circular` on a linear molecule
  is the gapped ring, unchanged.

  `Options::height` is a budget on a linear figure rather than a canvas: the
  scene comes back as tall as it needed. At the 720 × 720 default a PCR product
  is a 138 pt drawing, not a 138 pt drawing centred in 720 pt of white that
  `page::Fit` would print as an 89 × 89 mm block and a raster export would pay
  for in pixels. A budget too small for the caption, the band and the ruler
  yields a figure taller than it rather than a figure with its scale cropped
  off.

  A feature spanning the origin of a plasmid drawn cut open is **split**, one box
  per span, at the two ends of the track — those are the bases a reader would
  find there — and the caption saying the circle was cut at base 1 is what makes
  two boxes under one name read as a wrap rather than as two copies.

  `pl methods map` is a new methods paragraph for the figure, with the defaults
  interpolated from `pl_draw::Options` and its limits stated: overlapping
  features overprint in one band rather than being separated into lanes, and a
  map missing three names looks exactly like a molecule with three fewer
  features, so the count printed beside the export is part of the figure.

- **The gate renders the same molecule from two processes and compares bytes.**
  "Byte-identical on every platform" is on the front of this project and nothing
  in the gate had ever compared two separate *runs* — the renderer's own
  determinism tests loop inside one process, which holds constant every single
  thing that varies between them: the allocator, `RandomState`'s per-process
  seed, the environment, the locale. Demonstrated rather than assumed: a
  `std::process::id()` term added to the linear figure's height leaves the
  in-process test green and turns the new step red. Both shapes, all four
  formats, no Python and no corpus, so it runs everywhere the gate runs.

- **File ▸ New (Ctrl+N): a molecule that never came from a file.** Every door
  into the app was a file, so bases that arrive as bases — a gBlock in an email,
  a synthesis vendor's plain sequence, 300 bp pasted into a message — had to be
  written out as a FASTA in a text editor before Polylinker would look at them.
  The dialog takes a name and a block of bases and makes a document: line breaks
  and indentation, a FASTA header line, the coordinates off a numbered sequence
  listing, lower case and U are all accepted, and it says on screen what it
  ignored rather than dropping characters quietly. Anything that is not a
  nucleotide is **refused**, with the character and the position it first appears
  at, instead of being silently removed — a molecule with a hole in it is one
  nobody can check afterwards. **Circular or linear is chosen at creation**,
  because it changes the digest, the origin-crossing features and the gel, and
  because FASTA has no field to say it in. The bases go in through the same
  content-sniffed loader every file uses, so the new document undoes, autosaves,
  gets annotated on open and prompts for a location when you save it, exactly
  like one read from disk.

- **You can take the protein out of the desktop app.** It has painted a
  six-frame amino-acid track since 0.2.0 and there was no way to get a residue
  string onto the clipboard or into a file — so the most routine downstream step
  there is, pasting a protein into BLAST or a structure predictor or a
  colleague's email, ran through retyping it off the screen. There are now three
  doors, and they share one translator with the track rather than adding a
  second: **Copy protein** beside the sequence readout (Ctrl+Shift+P) takes the
  selection's reading, **Copy protein** in the Features toolbar takes the
  selected feature's, and **Save ▸ Protein FASTA…** writes every reading the
  document has plus the selection, one record each, through the same atomic
  writer every other save uses.

  **The genetic code travels with the protein.** Polylinker offers all 27 NCBI
  tables with a per-feature `/transl_table` override, and thirteen of the 27 do
  not treat `TGA` as a stop — so a residue string on its own does not determine
  its own bases, and a protein produced under table 11 and pasted somewhere that
  assumes table 1 is a wrong answer that looks right. Every header carries
  `transl_table=`, GenBank's own spelling of the number, alongside the reading's
  location in GenBank's own notation: `location=complement(join(1976..3310,3311..3397))`
  says the strand, the bases and the fact that there is more than one piece. The
  clipboard gets a FASTA record for the same reason rather than bare letters —
  it is the only form in which the number can travel, and everything that takes
  a protein takes FASTA.

  **The awkward cases are stated rather than guessed at.** A selection whose
  length is not a multiple of three says how many bases were left over; a
  reverse-strand or multi-segment reading says so in its location and again in
  words; an internal stop codon is counted and its residues named; a partial CDS
  running off the end of a linear molecule says how many bases the annotation
  claims that the molecule does not have, which was previously clamped in
  silence and read as a merely shorter protein; and an initiator that does not
  spell M — `GTG` under table 11, `V` under table 1 — says that the letter is a
  substitution and not what the codon spells.

  Help ▸ "Open reading frames and translation" now says where all three doors
  are, which is the half of that page that had a method and no location. Its
  methods paragraph — the one written to be pasted into a paper — also states
  the residue convention for the first time: the first codon of a reading is
  written `M` wherever the code permits initiation there whatever the codon
  spells, and a termination codon is written `*`. Both `pl orfs` and the desktop
  app have always done that, through the same `translate_cds`, and neither said
  so.

- **The desktop app can now find where an oligo binds.** Paste a primer into the
  new **Primers** tab and it lists every place that oligo anneals on the open
  molecule, on both strands, including sites that cross the origin of a circular
  plasmid. Each site is drawn on the map and boxed in the sequence view, and
  clicking one selects the bases it pairs with. This is the thing a cloner does
  most often with a primer they already have, and until now `pl primers <file>
  --primer SEQ` was the only way to do it: `pl-primer` reached the desktop binary
  only *transitively*, inside `pl-design`'s off-target prefilter, so the engine
  shipped with no caller a user could reach. The app also shipped a Help page
  titled "Primer binding sites" describing a search it could not run — the same
  defect, one crate over, that feature annotation had a day earlier, and it is
  recorded as such in `bins/pl-gui/Cargo.toml` beside the dependency.

- What the panel shows, and why each part is there. The **annealed footprint is
  kept visibly apart from any 5' tail**, because a 20 nt primer with a 20 nt
  Gibson arm is a 40-mer whose annealing temperature is the 20-mer's — the
  melting temperature is computed over the footprint alone, and a tool that
  prints one string cannot say so. The **number of sites is stated before the
  list**, in a warning colour and in words, because a primer that binds twice is
  a failed PCR and a panel that leads with the best site answers a question
  nobody asked. No melting temperature is reported for a footprint carrying a
  mismatch or an ambiguity code; the row says which of the two it is and what
  that means, rather than leaving a blank cell. Annealing temperatures are
  offered per polymerase for the selected site, over that site's footprint
  length, and labelled as vendor advice rather than a measurement.

- The panel exposes the same controls as `pl primers` — `--seed`,
  `--seed-mismatch` and `--exact` — with the same defaults, and the seed bounds
  are now a pair of constants in `pl-primer` that both surfaces read, so a GUI
  that accepted a seed the CLI refuses is arranged against rather than asserted.
  `the_primers_panel_and_the_cli_agree_about_the_same_primer_and_molecule`
  compares whole binding lists against the expression `cmd_primers` evaluates,
  and `the_panel_and_pl_tm_agree_about_the_footprint_and_not_the_whole_oligo`
  does the same for the temperature against the expression `pl tm` evaluates.

### Changed

- **Three things came off the roadmap on 2026-08-06, and none of them changes a
  byte that ships.** Code signing and macOS notarisation; Bar-Ilan
  technology-transfer clearance; and the rule that v1.0 waits for a second
  maintainer holding commit and release keys. None is planned work any more —
  not deferred, not blocked on money, not waiting on an office or on a person.
  They are struck rather than deleted in `docs/PLAN.md` (§4, §9.2, §10 risks 1,
  10 and 12, §11.1, §12), because a withdrawn plan that leaves no trace is
  indistinguishable from one that was never made. This entry exists so that a
  reader comparing two versions finds a gate disappearing here, rather than by
  diffing the plan.

- **The builds are unsigned, exactly as before, and it costs you exactly what it
  cost you before.** What changed is the tense: "not done yet", "outstanding"
  and "when a certificate arrives" implied one was on its way, and none is.
  `docs/RELEASING.md`, `SECURITY.md`, `README.md`, `README-WINDOWS.txt`,
  `README-MACOS.txt`, `tools/release-notes.md` and `Install-Polylinker.ps1` now
  say that plainly. Nothing was removed from any of them, and the gate in
  `tools/ci.ps1` that reads the shipped text still passes: Windows SmartScreen
  shows *"Windows protected your PC"* on first run and what that means is still
  explained; macOS Gatekeeper still refuses a downloaded binary and
  `xattr -d com.apple.quarantine` on the named files is still the remedy given;
  the SHA-256 still proves the bytes and nothing about who produced them; the
  Ed25519 signature over `SHA256SUMS.txt` is still a *manifest* signature and
  still not code signing; and a managed or locked-down machine may still refuse
  unsigned software outright, where the answer is still to ask the
  administrator rather than work around it. `README-LINUX.txt` was not touched,
  because it never described a future.

- `PROVENANCE.md` gains a dated amendment rather than an edit: the record of
  what was decided in July 2026 stands, and the note beneath it says which half
  of it stopped being planned work. Legal advice on the trademark and Israeli
  §24 questions did not stop being owed.

### Fixed

- **The disclosure line on a linear figure counted a different figure from the
  one it was printed on.** `pl export` and the app both build the "*N of M
  cutters labelled*" line in two passes — render once to learn how many labels
  fit, render again with a line saying so — and both carried a comment claiming
  this cannot change what it counts. On the ring it cannot: the note reaches
  `centre_room` → `keep_clear` → the ruler's radius and stops. On the track it
  reached the caption, which is one of the four terms fixing how many rows of
  labels there is room for, so drawing the note stole a row. Measured on a 6 kb
  track with 40 cut sites at 720 × 180: the line said 33 enzymes named and 7
  hidden, and the figure it was printed on named 24 and hid 16.
  `debug_assert!(Disclosure::closes)` passed on both, because 24 + 16 and 33 + 7
  both reach 40 — the arithmetic closed over numbers taken from the wrong
  picture. The linear figure now reserves the note's line in that arithmetic
  whether or not there is a note, which costs at most one row on a figure whose
  height is already binding and names every label it costs.

- **"in the PDF annotation" was never true.** Two comments justified shortening a
  feature name rather than a cut coordinate by saying the whole name survives
  "in the SVG `<title>`, in the PDF annotation and in the app's Features tab".
  There is no PDF annotation: `pdf.rs`'s own module doc has always said an
  annotation "would be furniture in a figure", and the writer emits no `/Annots`
  array at all. Traced one writer at a time and written down as measured — the
  SVG carries a real `<title>`, the EPS carries the text as a comment nothing
  renders, and a PDF and a PNG carry no copy whatsoever, so on those two the
  only surviving record is `Report::labels_truncated`. The conclusion the
  comments were reaching for holds on the true premise, because a reported loss
  is still a different thing from a silent one; a test now pins where the name
  does and does not appear, in all four formats.

- **A melting temperature in a methods paragraph now always carries the
  conditions it was computed under.** `pl methods primers` (and the same page in
  the app's Help window) said the temperature "is computed from the footprint
  alone" and then named no nearest-neighbour table, no salt correction and no
  concentration at all. These paragraphs exist to be pasted into a paper — the
  Help page has a "Copy this paragraph" button — and the same 20 nt footprint
  reads 53.9 °C on this model's 50 mM Na+ scale and about five degrees higher in
  an ordinary PCR buffer, so a reader given the number without the scale could
  neither reproduce it nor compare it. The paragraph now interpolates the
  conditions, states that no temperature is reported for a mismatched footprint
  and why, and says the extension rule the `--exact` flag switches. A test sweeps
  every topic and fails any paragraph that reports a temperature without naming
  its table and its sodium.

- The Design panel's conditions line and the `/note` it writes into your file
  now read the thermodynamics the pair was actually **scored** under, instead of
  re-deriving `Constraints::default()`. Same string today, because that panel
  puts no control on the salt; the point is that the note is saved into the
  document, so it had to be true by construction rather than by coincidence.

## [0.2.0] - 2026-08-06

### Added

- **The desktop app annotates.** Opening or pasting a molecule now searches it
  against the 89-record features database that was already compiled into
  `polylinker.exe`, and lists what it found at the top of the Features tab.
  Until now the app shipped a methods page *describing* an annotation it could
  not perform: `bins/pl-gui` had no dependency on `pl-features` at all, and the
  flagship item in `docs/PLAN.md` §v1.0 was reachable only from `pl annotate` on
  the command line.

  **They are proposals, and your document does not contain them.** Nothing is
  added until you press Accept — one row, or all of them — and each accepted
  feature is one undo step carrying the same provenance note
  `pl annotate --genbank` writes, from the same function, so a `.gb` written by
  the app and one written by the command line cannot come to say different
  things about the same hit. That is `features/SIGNOFF.tsv`'s rule in the
  interface: the tool may propose and may not assert. An implementation that
  silently wrote the hits into the file on open would demo better and would be
  asserting on somebody's behalf.

  Every row shows its identity **and** its coverage, never one without the
  other — the first 300 bp of a 600 bp marker copied perfectly is 100% identity
  at 50% coverage, and "100%" alone reads as "this is that feature". Rows also
  carry whether the match was nucleotide or protein, the record's `PLF:` id, and
  whether a curator has ever checked that record; an unreviewed record is
  marked in warning ink, and accepting it writes that caveat into your file.

  Defaults match `pl annotate` exactly: reviewed rows only, partial matches
  hidden, both one click away. The scan runs on a worker thread, is thrown away
  rather than remapped whenever an edit moves bases, and never touches the
  network. "Annotate on open" ships **on** — unlike the update check, which
  ships off, there is no privacy question here, only time, and the time was
  measured rather than assumed.

- **The app says what the database has no rows for.** There is not one promoter,
  terminator or origin of replication among the 89 records — those three classes
  have no automatable source that gives a defensible boundary, which
  `features/README.md` has always been candid about and which nobody reads
  before opening a plasmid. A user who watches `AmpR` light up and sees no `ori`
  concludes their plasmid has no `ori`, and the tool caused that by having just
  demonstrated that it knows what features are. The proposals panel, the About
  page and `pl methods annotate` all say so now, each computed from the shipped
  table by one function (`Db::absent_common_kinds`) rather than written down, so
  the sentence shortens by itself the day a `promoter` row lands.

- `CHANGELOG.md` — this file.
- `CITATION.cff`, so the repository is citable by people who have to cite their
  tools. There is no DOI; see the file.

- **A security policy, and a way to report a flaw.** `SECURITY.md` did not
  exist. The project now ships an embedded signing key and code that executes on
  it, and there was no channel to report a problem in either; reports go through
  GitHub private vulnerability reporting. It is specific rather than generic:
  the highest-value report is named, file parsers are in scope because a plasmid
  map arrives by email, and the key-compromise section states plainly that
  anyone who can push to the repository, anyone who can read the Actions secret,
  and GitHub itself can sign a release every installed copy will accept. It
  gives a rotation procedure and then says the procedure is untested.

- **`CITATION.cff`**, for an audience that cites things. There is no DOI, and
  the file says so rather than inventing one.

### Fixed

- **The minimum Rust version was a guess, and the guess was wrong.**
  `Cargo.toml` declared `rust-version = "1.82"`, and `README-LINUX.txt` and the
  release notes told anyone whose glibc is too old for the Linux binaries that
  building from source needs Rust 1.82. That is advice aimed precisely at the
  people who cannot check it cheaply, and nothing had ever compiled this tree
  with 1.82: every toolchain step in both workflows is
  `dtolnay/rust-toolchain@stable`. 1.82 does not get as far as compiling —
  `indexmap 2.14.0` in `Cargo.lock` is edition 2024, which cargo 1.82 refuses
  at the manifest. The floor is **1.92**, bounded from both sides: 1.92 checks
  the whole workspace clean, and 1.91.1 is rejected by the eight egui 0.35
  crates the editor depends on. All three copies of the number now say 1.92, a
  new `msrv` job in CI installs whatever `rust-version` declares and runs
  `cargo check --workspace --locked` on it, and a gate step fails if the prose
  and the manifest disagree.

  Nothing about the published binaries changes. What changes is that the
  number a reader acts on is now compiled against on every push.

- **The 200 ms annotation budget had never been measured.**
  `docs/PLAN.md` §v1.0 item 5 has claimed "under 200 ms for a 10 kb plasmid"
  since the plan was written, with nothing computing it — the same shape of
  unchecked number as the `1.82` above, on a claim more people quote.
  `crates/pl-features/tests/budget.rs` measures it now, on two 10 kb circular
  plasmids built out of real records from the shipped table. **The budget holds:
  11 ms and 103 ms, release build**, against 106 ms and 1,075 ms debug.

  The interesting part is that the two differ by nine times, and in the
  direction nobody would pick: the plasmid with four multi-kilobase CDSs costs
  nine times the one carrying 37 short parts, because the cost is the aligner
  and the aligner is the product of the two lengths. Measuring only the busy
  plasmid — the one that looks harder — would have reported 11 ms and declared
  the budget met with eighteen times the room it actually has.

- **A GUI build spawning `curl` opened a console window on Windows.**
  `polylinker.exe` is a windows-subsystem binary with no console of its own, so
  Windows makes one for a console child; `curl` finishes in well under a second,
  and what a user saw was a black window appearing and vanishing. On a tool
  whose whole claim is that it does not touch the network unless asked, an
  unexplained terminal flashing at launch is the worst possible thing to show.
  `CREATE_NO_WINDOW` is now set on both `curl` invocations, which
  `bins/pl-gui/src/recover.rs` had already been doing for its own child process
  since long before the updater was written. Nobody had seen the flash because
  the update check ships off: the defect was real, latent, and reserved for the
  first person ever to switch the setting on.

Nothing else has landed since v0.1.3 that changes what the programs do: a
`cargo fmt` of the release-signature test, and a recount of the line count the
README asserts about itself.

- **The MIT half of "MIT OR Apache-2.0" did not exist.** `Cargo.toml`, the
  release notes and the npm package all offered a dual licence; only the Apache
  text was committed, and `packages/circular-map/package.json` listed a
  `LICENSE-MIT` in its `files` array that was not there. Anyone who chose the
  MIT half was offered a licence they could not read. Both texts now ship in
  every archive and the MSI, and `tools/check-archive.ps1` requires them **by
  name** — this project has lost licence texts from a packaging step twice, and
  a count cannot tell which one went missing.

- **The README claimed a relationship with REBASE that does not exist.** It said
  restriction-enzyme data "is REBASE, redistributed under its own terms". It is
  not: `NOTICE` says REBASE data *will be* sourced into a separate repository,
  and what ships is 58 enzymes transcribed from published references,
  "not a reproduction of any database". Claiming to redistribute a database the
  project has not licensed is the wrong direction to be wrong in.

- **`Cargo.toml` pointed at a GitHub organisation that does not exist**
  (`polylinker/polylinker`). The identical mistake shipped in the updater in
  0.1.2 and made `pl update` fail with a 404.

- **`CONTRIBUTING.md` described a different project** — "there is no build. The
  reference implementation is Python with no dependencies beyond the standard
  library" — against 21 workspace crates, three-OS CI and a 65-step gate.

## [0.1.3] - 2026-08-06

### Fixed

- **`pl update` reaches a repository that exists.** The compiled-in
  `RELEASE_BASE_URL` was `https://github.com/polylinker/polylinker`, an
  organisation nobody registered, so `pl update --check` returned 404 the first
  time it was pointed at the real internet. Every unit test passed throughout:
  they assert that a URL is *built* correctly from the constant, and none of
  them asserted that the constant was right.

  **If you are running 0.1.2, its updater cannot work and cannot tell you this
  release exists.** Download 0.1.3 from the releases page by hand.

### Added

- The signature CI actually produced is now a committed test fixture, together
  with the manifest it covers: the real `SHA256SUMS.txt` from the v0.1.2 release
  page and the real 64 signature bytes `openssl` produced on the runner from
  `POLYLINKER_RELEASE_KEY`. Everything else about signing was tested against
  keys the tests invent, which proves self-consistency — exactly what a pipeline
  signing with the wrong key would also have proved. This is the first test in
  which the private half and the compiled-in public half meet.
- Negative controls for it, because a test that only checks a valid signature
  passes against a verifier that returns `true`: a flipped bit in the manifest,
  a flipped bit in the signature, and a public key one bit away from the release
  key are each required to be refused.
- `.gitattributes` pins those two fixtures to LF. Their bytes are the message
  the signature was made over, so a CRLF checkout would change what was signed
  and fail the test on Windows alone — announcing that the release key does not
  match the compiled-in key, which would be false and is the most alarming thing
  this repository could say.

## [0.1.2] - 2026-08-06

The first signed release. Manifest signing and the updater's verification of it
had never met: nothing on a development machine can sign with the CI secret, and
the publish job only runs on a tag. This is the tag that tested it.

**Broken in this release:** `pl update` points at a GitHub organisation that
does not exist and returns 404 for every check. Fixed in 0.1.3, which 0.1.2
cannot tell you about. Everything else below works.

### Added

- **An Ed25519 signature over the release manifest.** Every release page now
  carries `SHA256SUMS.txt.sig` beside `SHA256SUMS.txt`, and prints the OpenSSL
  command to check it by hand. A checksum proves your download matches the
  release page; the signature proves the release page came from whoever holds
  the release key. The private half is a GitHub Actions secret,
  `POLYLINKER_RELEASE_KEY`, and is on no machine here.
- **The public key, compiled into `pl` and `polylinker`.** Only those two:
  `pl-mcp`, the Python extension module and the wasm build do not carry it,
  because none of them can update anything. The trust anchor is in the binary
  being replaced rather than fetched from the network, which is the whole point.
  Rotating that key needs every installed copy to be replaced by hand — there is
  **no revocation channel**, because a revocation channel is a network call.
  [`docs/RELEASING.md`](docs/RELEASING.md) records that cost.
- **`crates/pl-update`, an opt-in updater**, meeting the four conditions
  `docs/RELEASING.md` had set for one before it was allowed to exist. It fetches
  `SHA256SUMS.txt` and `SHA256SUMS.txt.sig` into memory and verifies the
  signature *before it requests the platform artifact at all*; a failed
  signature means the artifact is never asked for and nothing is written. The
  download lands on a `.part` file and is renamed into place only if its
  SHA-256 matches the entry in the verified manifest. It then prints the path
  and stops: it replaces nothing, refuses to write into the directory it is
  running from, and running the new file is yours to do.
- **`pl update` and `pl update --check`.** One request, made because somebody
  typed the verb. No thread, no timer, no stored "last checked".
- **An update check in the desktop app, under Help, shipped off.** Turned on it
  asks once per launch and shows a notice pointing at the release page; it never
  downloads. A new installation contacts nothing, and a truncated or hand-edited
  settings file falls back to off rather than on.
- **SHA-512 and Ed25519 verification in `pl-core`**, hand-written and with no
  dependency, checked against Wycheproof vectors and adversarially tested.

### Changed

- The release notes and `docs/RELEASING.md` no longer say "no updater". They say
  no *auto*-updater, and describe the two opt-in paths. The distinction is the
  point: nothing runs on a timer and nothing installs anything.

## [0.1.1] - 2026-08-05

### Added

- **A Windows MSI installer**, `polylinker-0.1.1-windows-x64.msi`. It installs
  for you alone by default — no administrator, no elevation prompt — with "for
  everyone" offered for machines where you are one. It puts Polylinker in the
  Start Menu and in Settings → Apps and offers to put `pl` on your PATH. It
  **adds** Polylinker to the "Open with" list for eight extensions — `.dna`,
  `.gb`, `.gbk`, `.genbank`, `.fasta`, `.fa`, `.fna` and `.ab1` — and takes none
  of them away: it writes `OpenWithProgids` entries and never an extension's own
  default, so if SnapGene owns `.dna` on your machine it still does afterwards.
  The installer contacts nothing and registers no service, no scheduled task and
  no auto-updater — nothing it puts on the machine ever runs on its own, and
  `tools/ci.ps1` fails the build if any network or scheduling facility appears in
  the installer sources.
- The MSI's file list is generated from the same `SHA256SUMS.txt` the zip is
  verified against, rather than written out a second time, because a second list
  is how a licence text stops shipping.
- Install, verify and uninstall are exercised on a CI runner: every payload file
  under `LocalAppData`, the installed `pl.exe` reporting its own version, the
  Start Menu shortcut, the `.dna` handler registration, and a planted foreign
  default handler surviving both install and uninstall.

The portable zip is unchanged and still ships `Install-Polylinker.ps1` for
anyone who would rather run something they can read.

## [0.1.0] - 2026-08-05

First public release.

### Added

- **Three platforms**, each built on the operating system it runs on:
  `polylinker-0.1.0-windows-x64.zip`, `polylinker-0.1.0-macos-universal.tar.gz`
  (one binary for Apple Silicon and Intel) and
  `polylinker-0.1.0-linux-x64.tar.gz` (glibc 2.39 or newer).
- Each archive contains `polylinker` (the desktop editor), `pl` (the command
  line), `pl-mcp` (a read-only MCP server), the Python extension module, the
  licence texts that have to accompany every copy — `LICENSE.txt`, `NOTICE.txt`,
  `TRADEMARKS.md`, `features/NOTICE.txt` and seven font licences under
  `licences/` — a per-platform read-me, and a `SHA256SUMS.txt` covering all of
  them. The release page carries a second `SHA256SUMS.txt` over the three
  archives themselves. The Windows zip adds `Install-Polylinker.ps1`,
  `Install.cmd`, `README-WINDOWS.txt` and the icon.
- `tools/check-archive.ps1` asserts the required members of each archive **by
  name and per platform**, not by count: an empty archive agrees perfectly with
  an empty manifest, and a count cannot tell a missing binary from a missing
  licence.

### Known limitations at this release

- **Unsigned, on every platform.** No code-signing certificate and no Apple
  Developer ID. macOS Gatekeeper refuses the files until
  `com.apple.quarantine` is removed from them by hand; Windows SmartScreen warns
  on first run. Neither is an oversight, and the words for clicking past a
  security warning appear in nothing this project ships.
- **No updater of any kind.** `pl --version` printed the version and the commit
  and asked nobody anything. One was added in 0.1.2.
- **No manifest signature.** `SHA256SUMS.txt` shipped unsigned, so the release
  page proved integrity and not origin. Added in 0.1.2.

[Unreleased]: https://github.com/liorlobel/polylinker/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.6.0
[0.5.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.5.0
[0.4.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.4.0
[0.3.2]: https://github.com/liorlobel/polylinker/releases/tag/v0.3.2
[0.3.1]: https://github.com/liorlobel/polylinker/releases/tag/v0.3.1
[0.3.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.3.0
[0.2.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.2.0
[0.1.3]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.3
[0.1.2]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.2
[0.1.1]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.1
[0.1.0]: https://github.com/liorlobel/polylinker/releases/tag/v0.1.0
