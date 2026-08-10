# PROPOSED.md -- the curator worklist for the 21 unsigned rows

`features/SIGNOFF.tsv` states the rule this database exists to enforce:
**AI may propose, never assert.** These 21 rows are the proposal. Every one
ships `review_status = proposed` with an empty curator, none appears in
`SIGNOFF.tsv`, and `Db::reviewed()` ships none of them. Nothing here is in
the product until a named human signs it.

This file is the reading list for that human. It says what each row claims,
which accessions to check it against, what boundary was chosen and on what
basis, and -- the part that matters -- **which rows are asking you to ratify
a contested choice rather than to confirm an arithmetic fact.**

| | |
|---|---|
| Table | 110 rows, of which 89 signed and 21 proposed |
| This worklist | 21 rows: 14 selection markers (Stage 2), 7 Class B conventions (Stage 5) |
| Refused, not proposed | 5 Class B elements that were built and then failed the extent-corroboration rule; they are in *Refused on the evidence* below, and rescuing one is real work asked for here |
| Branch | `features-promoters-markers-terminators` |
| Signatures on file | 89, all still valid, `SIGNOFF.tsv` byte-identical to `main` |

---

## How to sign

From `SIGNOFF.tsv`'s own HOW TO SIGN section. Step 1 is *read the row*, and
there is a command for that -- it prints every column the signature covers,
unescaped, plus the provenance quads and the resulting digest, and it writes
nothing:

```
python features/build/build.py --show PLF:1014
python features/build/build.py --show PLF:1015
python features/build/build.py --show PLF:1016
python features/build/build.py --show PLF:1017
python features/build/build.py --show PLF:1018
python features/build/build.py --show PLF:1019
python features/build/build.py --show PLF:1020
python features/build/build.py --show PLF:1021
python features/build/build.py --show PLF:1022
python features/build/build.py --show PLF:1023
python features/build/build.py --show PLF:1024
python features/build/build.py --show PLF:1025
python features/build/build.py --show PLF:1026
python features/build/build.py --show PLF:1027
python features/build/build.py --show PLF:4000
python features/build/build.py --show PLF:4001
python features/build/build.py --show PLF:4006
python features/build/build.py --show PLF:4007
python features/build/build.py --show PLF:4008
python features/build/build.py --show PLF:4009
python features/build/build.py --show PLF:4010
```

Several at once, comma-separated and no spaces:

```
python features/build/build.py --show PLF:4000,PLF:4001,PLF:4006,PLF:4007,PLF:4008,PLF:4009,PLF:4010
python features/build/build.py --show PLF:1014,PLF:1015,PLF:1016,PLF:1017,PLF:1018,PLF:1019,PLF:1020,PLF:1021,PLF:1022,PLF:1023,PLF:1024,PLF:1025,PLF:1026,PLF:1027
```

Then take the 64 hex characters from that output -- or run
`python features/build/build.py --print-digests` for the whole table -- and add
one line per row to `SIGNOFF.tsv`: record id, `reviewed` or `verified`, your
name, the date, the digest, and a note saying what you actually checked.

**No digests are printed in this file, on purpose.** `SIGNOFF.tsv` says
signing a digest nobody has read is not an attestation; a worklist that let
you copy 26 hashes out of it without opening a single row would be a machine
for producing exactly that. The digests also change the moment any prose in a
row changes, so a copy here would go stale silently.

Note the order of work `SIGNOFF.tsv` records: **change prose first, then
sign.** `description` and `notes` are inside the digest, so rewriting them
after signing lapses the signature.

---

## Read these first

### Blocking: do not sign until resolved

**PLF:1016 bsr** -- **ORGANISM CONFLICT, UNRESOLVED.** UniProt P33967 gives Bacillus cereus; ENA S81409 carries /organism="Escherichia coli" /strain="TK121", which reads as the expression host. The sequence verifies exactly either way -- this is a provenance-of-the-gene question, not a sequence question -- so the description deliberately names NO organism. S81409 is also an S-prefixed literature-derived record, not a depositor submission. Read the cited paper and write the organism in before signing.

### Contested: the row picked one of several defensible answers

These are the rows where exemplars disagreed, where the anchor record does
not settle the boundary, or where the name covers more than the row does.
Signing them is a judgement, not a check.

| Row | What is contested |
|---|---|
| **PLF:4006** CMV enhancer | **DO NOT SIGN ALONE.** This row previously carried the note *"ship with PLF:4005 or not at all -- shipping one alone silently picks a convention"*, and PLF:4005 has since been refused by the extent rule below. That advice has not been withdrawn; it has become a blocker. A user who annotates a CMV region and sees only an enhancer light up is being told something false by omission. Also: a rival 378 nt form differs internally, not just at the ends. |
| **PLF:4007** T7 terminator | Two rival 48 nt forms, OFFSET FROM EACH OTHER BY ONE BASE. Neither is wrong. Also: one deposit gives the name to a different part of the T7 genome. |
| **PLF:4008** rrnB T1 terminator | Rival extents 43-98 nt, nested; 5' ends vary ~10 bases, 3' ends ~45. Nothing primary says 'T1'. |
| **PLF:4001** SP6 promoter | The anchor annotates no promoters at all, so nothing in it bounds the interval. Bounded by analogy to T7. |
| **PLF:1020** HIS3 | The classic HIS3 clone is a different sequence (219 aa vs 220), and the automated survey structurally cannot report it. |
| **PLF:1022** TK | Two open decisions: the alias/lookup gap ('HSV-TK', 'UL23' do not resolve) and the choice between two reviewed strains differing at 4 residues. Patent flagged. |
| **PLF:1018** URA3 | Four of nine cross-references carry A160S, three of them in records titled 'cloning vector' -- the deviating allele is what constructs carry. |

### Refused on the evidence: five Class B elements that are NOT in the table

Added 2026-08-10. `stage_classb.MIN_PLACEMENTS` requires **two independent
submissions to annotate a feature at exactly the shipped extent**, edge for
edge, before a row may carry `boundary_rule = consensus_of_insdc`. Until that
build the stage measured where each depositor drew the edges, wrote it into
`notes`, and tested nothing -- so "consensus" could rest on one lab. These five
rested on one lab, and the build now drops each of them with its numbers
printed:

| Row | Element | Submissions holding the bases | Placing it at our extent |
|---|---|---|---|
| `PLF:4002` | lac promoter | 4 | 1 (HM126493.1) |
| `PLF:4003` | tac promoter | 3 | 1 (MH488909.1 + MH488911.1, one address) |
| `PLF:4004` | trc promoter | 2 | 1 (U13872.1, which is the anchor itself) |
| `PLF:4005` | CMV promoter | 3 | 1 (LC897329.1) |
| `PLF:4011` | SV40 early poly(A) | 3 | 1 (LT009443.1) |

**These are not deletions and they are not `HELD`.** They stay in
`stage_classb.ITEMS`, keep their ids, and are re-measured on every build, so a
row returns by itself the moment its evidence does. Two ways to rescue one, both
a curator's and neither a program's:

1. **Cite more evidence.** Find further independent submissions that draw the
   same edges and add them to the item's `exemplars`. Note what this is not:
   searching until the check goes green is the failure mode, and for at least
   `PLF:4005` and `PLF:4011` a wider survey says it will not work -- across 481
   records, roughly one independent submission in nine uses our CMV-promoter
   extent, and the SV40 poly(A) figure is one in forty-six.
2. **Re-cut the extent** to one the evidence already corroborates, and rewrite
   the row's basis to match. For `PLF:4002` that means confronting the 84 nt
   convention the anchor's own annotation gestures at; for `PLF:4005` it means
   deciding whether the CMV promoter is separable from the enhancer at all.

One of these is worth reading twice. **`PLF:4005`'s only exact corroboration was
`LC897329`**, whose feature table is SnapGene's Common Feature naming from top to
bottom -- `CMV enhancer`, `CMV promoter`, `bovine growth hormone (bGH)
polyadenylation signal` -- with no `label:` tell anywhere in it, so the stage's
SnapGene screen passes it and counts it. That is the blind spot `SOURCING.md`
§0.6 describes, in a record this database relies on. It was not a taint check
that caught the row; it was the extent rule, which names no vendor at all.

### Flagged for patent, not adjudicated

No patent database was searched (SOURCING.md Risk 6). `patent_flag = 1` on:

- **PLF:1022 TK**
- **PLF:1025 bar**
- **PLF:1026 pat**

---

## What was already checked mechanically, and what that does not prove

Independently re-derived for this PR by a checker sharing no code with the
build -- its own fetches, its own coordinate arithmetic, its own codon table:

- **26 of 26 rows re-derived exactly** when this was written, and the five rows
  since refused are among them: they were refused on their *boundary evidence*,
  never on their bases. Every Class B slice was re-cut from a
  window padded 25 nt either side and required to land at exactly that offset,
  so the coordinates were tested rather than handed to the server. Every marker
  CDS was re-translated and matched its shipped protein residue for residue.
- **All 14 marker proteins are the enzyme the row names**, confirmed against
  the UniProt canonical sequence and its recommended name.
- **The T7 boundary argument holds**: all 7 copies of the 17-mer in the T7
  genome end one base before an annotated promoter point, and in all 7 the next
  base is G.
- **The CMV split is arithmetic**: 204 + 380 = 584, contiguous, nothing over.
  That says the two intervals abut. It does not say the split is where the
  community puts it, which is why `PLF:4005` needed corroboration and did not
  have it.

None of that is a substitute for signing. It proves the bases are the bases
the accession holds. It says nothing about whether the *boundary* is the one
the community means by the name, which is the whole of what Class B is, and
nothing about the organism, strain and allele questions listed above.

---

## The 7 Class B rows (Stage 5, `features/build/stage_classb.py`)

SOURCING.md section 3 classes promoter, terminator and poly(A) boundaries as
**conventions, not facts** -- there is no database that says where 'the CMV
promoter' ends -- and section 6 prescribes human curation plus at least two
independent GenBank exemplars each -- and section 4 says what for: "showing
where depositors actually place it". The stage executes both. Each row is a
coordinate slice of one anchor record, re-fetched and re-sliced every build,
with at least two further records **from different submitting addresses**
carrying the exact bases, and at least two independent submissions annotating a
feature at **exactly** the shipped extent.

Three things to know while reading these:

- **Witness counts are floors.** Submitting addresses are merged fuzzily when
  they look like one lab writing its address two ways.
- **SnapGene-annotated deposits are refused as witnesses**, because INSDC
  carries them and the CI taint gate structurally cannot see one -- it compares
  descriptions, never coordinates. Rows below name the records excluded. That
  screen catches only the deposits that kept the `label:` tell; `SOURCING.md`
  §0.6 records that no detector can do better, and what is enforced instead.
- **Holding the bases and drawing the same edges are different claims**, and
  the second is the one `consensus_of_insdc` rests on. Each row's `notes` now
  carries both counts. Five elements were built and refused on the second; they
  are in *Refused on the evidence* above and are not rows.

### PLF:4000 -- T7 promoter

- **Claims**: The 17 bp class III promoter of bacteriophage T7: the seventeen bases immediately upstream of a T7 transcription start. T7 RNA polymerase reads it as a single subunit with no sigma factor and no accessory protein, and the host polymerase cannot read it at all. That mutual blindness is what a T7 expression system is built on.
- **Anchor**: `V01146.1:22887-22903:+`  (17 nt, `promoter`, `consensus_of_insdc`)
- **Check against**: AF053733.1, KJ641600.1, PV764404.1, GQ421427.1
- **Witnesses**: 5 independent submitting address(es) over 5 record(s)
- **Places it at our extent**: 4 of 5 -- AF053733.1, KJ641600.1, PV764404.1, GQ421427.1
- **Boundary decision**: Ship the promoter as -17..-1, excluding the +1 base.
- **Basis**: The anchor settles it mechanically: all seven copies of these 17 bases in the T7 genome end exactly one base before a coordinate the record annotates as a promoter, and in all seven the next base is G. Re-measured for this PR: 7 of 7. A 20 nt convention exists and runs three bases into the transcript. PLF:1004 already calls this promoter 17 bp, so a longer row would contradict a signed row.

  ```
  python features/build/build.py --show PLF:4000
  ```

### PLF:4001 -- SP6 promoter

- **Claims**: The 17 bp promoter of Salmonella phage SP6, read by SP6 RNA polymerase and by no host enzyme. Bounded by the same convention as the T7 row: the seventeen bases upstream of the transcription start. Paired with a T7 or T3 promoter at the other end of a polylinker it is what lets a vector transcribe either strand of an insert in vitro.
- **Anchor**: `AY288927.2:12542-12558:+`  (17 nt, `promoter`, `consensus_of_insdc`)
- **Check against**: DQ250998.1, FJ457001.1, KC800697.1
- **Witnesses**: 3 independent submitting address(es) over 4 record(s)
- **Places it at our extent**: 2 of 3 -- DQ250998.1 + FJ457001.1 (one address), KC800697.1
- **Boundary decision**: Ship 17 nt, bounded by analogy to T7 rather than by an annotation.
- **Basis**: WEAKER EVIDENCE THAN T7 AND THE ROW SAYS SO. The SP6 genome record annotates no promoters at all, so nothing in the anchor bounds this interval; the bases occur three times identically and the cited copy is the first. The boundary rests on the three witness deposits and on the T7 analogy, not on the anchor.

  ```
  python features/build/build.py --show PLF:4001
  ```

### PLF:4006 -- CMV enhancer

- **Claims**: The immediate-early enhancer of human cytomegalovirus: a tandem array of repeated binding sites for host transcription factors, directly upstream of and contiguous with the immediate-early promoter. It is the part that supplies the strength, and it works in most mammalian cell types, which is why it travels with the promoter into vectors.
- **Anchor**: `X17403.1:173949-174328:-`  (380 nt, `enhancer`, `consensus_of_insdc`)
- **Check against**: LC897329.1, OP697991.1
- **Witnesses**: 3 independent submitting address(es) over 3 record(s) -- EXCLUDED as SnapGene-annotated: MH325107.1
- **Places it at our extent**: 2 of 3 -- LC897329.1, OP697991.1. **LC897329.1 is the record whose feature table is SnapGene's Common Feature naming throughout with no `label:` tell**, so half this row's corroboration comes from a deposit the screen cannot see. That is not a reason to refuse it; it is a reason to read it. See *Refused on the evidence* above, where the same record was the whole of PLF:4005's corroboration.
- **Boundary decision**: Ship the 380 nt enhancer as the upstream half of the block.
- **Basis**: A 378 nt convention is also widely deposited and is NOT this row two bases shorter -- a record annotating it was checked and does not contain these 380 bases at all, so the two differ internally.

  ```
  python features/build/build.py --show PLF:4006
  ```

### PLF:4007 -- T7 terminator

- **Claims**: The Tphi transcription terminator of bacteriophage T7: a GC-rich hairpin followed by a run of thymines, which together stall T7 RNA polymerase. Placed downstream of the insert in T7 expression vectors so that transcription stops instead of running on round the plasmid.
- **Anchor**: `V01146.1:24164-24210:+`  (47 nt, `terminator`, `consensus_of_insdc`)
- **Check against**: AF525444.1, PV764404.1, KJ641600.1
- **Witnesses**: 4 independent submitting address(es) over 4 record(s)
- **Places it at our extent**: 2 of 4 -- AF525444.1, PV764404.1
- **Boundary decision**: Ship the 47 nt form whose 3' end is the coordinate the anchor annotates.
- **Basis**: CONTESTED, AND THE CLEAREST CASE THAT THESE ARE CONVENTIONS. Two rival 48 nt forms are deposited and they are OFFSET FROM EACH OTHER BY ONE BASE; neither is wrong. Separately, at least one deposit labels 'T7 terminator' a sequence from a different part of the T7 genome entirely.

  ```
  python features/build/build.py --show PLF:4007
  ```

### PLF:4008 -- rrnB T1 terminator

- **Claims**: The first of the two tandem rho-independent terminators at the end of the Escherichia coli rrnB ribosomal RNA operon: a GC-rich stem-loop followed by a thymine run. Used downstream of a cloned gene to stop transcription, and upstream of a promoter to insulate it from read-through from the vector.
- **Anchor**: `J01695.2:6369-6412:+`  (44 nt, `terminator`, `consensus_of_insdc`)
- **Check against**: DQ115377.1, EF216319.1, U13872.1
- **Witnesses**: 3 independent submitting address(es) over 4 record(s)
- **Places it at our extent**: 2 of 3 -- DQ115377.1 + EF216319.1 (one address), U13872.1
- **Boundary decision**: Ship 44 nt, named from vector records and located from the primary operon record.
- **Basis**: CONTESTED EXTENT. Rivals are nested around this one and run 43 to 98 nt: 5' ends vary by about ten bases, 3' ends by about forty-five. The rrnB operon record annotates no terminator, so nothing primary says 'T1'. Rfam cannot help -- SOURCING.md line ~193 records as a confirmed negative that Rfam does not model standalone rho-independent terminators.

  ```
  python features/build/build.py --show PLF:4008
  ```

### PLF:4009 -- rrnB T2 terminator

- **Claims**: The second of the two tandem terminators of the Escherichia coli rrnB operon, downstream of T1. Vectors that carry 'rrnB T1T2' carry both with the natural spacer between them; this row is T2 alone.
- **Anchor**: `J01695.2:6544-6571:+`  (28 nt, `terminator`, `consensus_of_insdc`)
- **Check against**: LT739213.1, U13859.1, U13872.1
- **Witnesses**: 3 independent submitting address(es) over 4 record(s)
- **Places it at our extent**: 2 of 3 -- LT739213.1, U13859.1 + U13872.1 (one address)
- **Boundary decision**: Ship 28 nt.
- **Basis**: NO COMPETING EXTENT WAS FOUND -- every deposit that annotates T2 separately encloses exactly these 28 bases. Note 'rrnB T1T2' as a single annotation is a THIRD element (T1 + spacer + T2) and is neither this row nor PLF:4008.

  ```
  python features/build/build.py --show PLF:4009
  ```

### PLF:4010 -- bGH poly(A) signal

- **Claims**: The polyadenylation signal of the bovine growth hormone gene: the AATAAA hexamer together with enough flanking sequence to include the downstream GT-rich element, which cleavage and polyadenylation need as much as the hexamer itself. The standard 3' element of mammalian expression vectors.
- **Anchor**: `M57764.1:2326-2550:+`  (225 nt, `polyA_signal`, `consensus_of_insdc`)
- **Check against**: LC897329.1, MN224159.1, OR659033.1
- **Witnesses**: 4 independent submitting address(es) over 4 record(s) -- EXCLUDED as SnapGene-annotated: MN811118.1
- **Places it at our extent**: 3 of 4 -- LC897329.1, MN224159.1, OR659033.1
- **Boundary decision**: Ship 225 nt, hexamer plus the downstream GT-rich element.
- **Basis**: BEST-CONVERGED ELEMENT IN THE STAGE; rival extents differ at the 5' end only. A short row would be actively wrong here: the AATAAA hexamer alone is six bases and occurs by chance in any plasmid. Re-measured: AATAAA present at position 91 of 225.

  ```
  python features/build/build.py --show PLF:4010
  ```

### PLF:1014 -- pac (Puromycin N-acetyltransferase)

- **Claims**: Puromycin N-acetyltransferase of Streptomyces alboniger. Transfers an acetyl group from acetyl-CoA onto the free amino group of the tyrosinyl moiety of puromycin. Puromycin is an aminonucleoside that mimics the 3' end of an aminoacyl-tRNA and terminates the growing peptide chain; acetylation abolishes the mimicry. The standard dominant selection marker for mammalian cell culture, where killing is fast and selection is usually complete within a few days.
- **Sources**: UniProt `P13249` -> ENA CDS `M25346.1:254..853`  (600 nt, 199 aa, initiator ATG)
- **Decision**: Ship the ORF, initiator through stop.
- **Basis**: CASSETTE, NOT ORF is what a map means by 'PuroR'. Codon-optimised pac is common in mammalian work and cannot match these nucleotides at all; the protein reference serves that case.

  ```
  python features/build/build.py --show PLF:1014
  ```

### PLF:1015 -- bsd (Blasticidin-S deaminase)

- **Claims**: An enzyme of the fungus Aspergillus terreus that inactivates the nucleoside antibiotic blasticidin S. It hydrolyses the amino group off the drug's cytosine ring, and the deaminohydroxy product no longer blocks peptide-bond formation at the ribosome. Two entirely unrelated deaminases are sold under the name 'blasticidin resistance'; this is the fungal one.
- **Sources**: UniProt `P0C2P0` -> ENA CDS `D83710.1:50..442`  (393 nt, 130 aa, initiator ATG)
- **Decision**: Ship the fungal bsd.
- **Basis**: Two unrelated deaminases are both sold as 'blasticidin resistance'. This is the Aspergillus terreus one; PLF:1016 is the bacterial one. Different lengths, different sequences, must not merge.

  ```
  python features/build/build.py --show PLF:1015
  ```

### PLF:1016 -- bsr (Blasticidin-S deaminase)

- **Claims**: Blasticidin S deaminase of the bsr type. Inactivates blasticidin S by the same hydrolytic deamination as the fungal bsd enzyme, from a different protein family and a bacterial source. Widely used as a blasticidin selection marker in mammalian and insect cells.
- **Sources**: UniProt `P33967` -> ENA CDS `S81409.1:182..604`  (423 nt, 140 aa, initiator ATG)
- **Decision**: Ship the bacterial bsr -- BUT SEE THE BLOCKER.
- **Basis**: **ORGANISM CONFLICT, UNRESOLVED.** UniProt P33967 gives Bacillus cereus; ENA S81409 carries /organism="Escherichia coli" /strain="TK121", which reads as the expression host. The sequence verifies exactly either way -- this is a provenance-of-the-gene question, not a sequence question -- so the description deliberately names NO organism. S81409 is also an S-prefixed literature-derived record, not a depositor submission. Read the cited paper and write the organism in before signing.

  ```
  python features/build/build.py --show PLF:1016
  ```

### PLF:1017 -- dhfrI (Dihydrofolate reductase type 1)

- **Claims**: Type I dihydrofolate reductase, the trimethoprim-insensitive enzyme carried on the Tn7 dfrA1 cassette. It reduces dihydrofolate to tetrahydrofolate exactly as the chromosomal enzyme does, but is bound by trimethoprim far more weakly, so one-carbon metabolism continues while the host enzyme is inhibited. Trimethoprim selection is useful where beta-lactam and aminoglycoside markers are already spent, and the cassette travels in integrons and in broad-host-range backbones.
- **Sources**: UniProt `P00382` -> ENA CDS `X00926.1:236..709`  (474 nt, 157 aa, initiator GTG (non-AUG initiator read as Met))
- **Decision**: Ship the Tn7 dfrA1 type I enzyme.
- **Basis**: The claimed DHFR/dhfr alias collision with PLF:1023 WAS MEASURED AND DOES NOT EXIST: UniProt calls this one 'Dihydrofolate reductase type 1' and the mouse enzyme 'Dihydrofolate reductase', so each resolves to one record. The real gap is that the vernacular 'DHFR' on a map resolves to NEITHER -- a naming decision for the curator. Non-ATG start: GTG.

  ```
  python features/build/build.py --show PLF:1017
  ```

### PLF:1018 -- URA3 (Orotidine 5'-phosphate decarboxylase)

- **Claims**: Orotidine 5'-phosphate decarboxylase of Saccharomyces cerevisiae, the final step of de novo pyrimidine biosynthesis. Complements a ura3 auxotroph, and is the standard yeast counter-selectable marker as well: cells that carry it convert 5-fluoroorotic acid into a toxic product, so growth on 5-FOA selects for having lost the gene.
- **Sources**: UniProt `P03962` -> ENA CDS `U18530.1:26573..27376`  (804 nt, 267 aa, initiator ATG)
- **Decision**: Pin the primary chromosome V record, not a vector-derived allele.
- **Basis**: MULTI-ALLELE TRAP. Four of the entry's nine EMBL cross-references carry the same residue-160 polymorphism (A160S), and three of those four sit in records whose own titles say 'cloning vector' -- so the deviating allele is what a construct is likely to carry.

  ```
  python features/build/build.py --show PLF:1018
  ```

### PLF:1019 -- LEU2 (3-isopropylmalate dehydrogenase)

- **Claims**: 3-isopropylmalate dehydrogenase of Saccharomyces cerevisiae, the third enzyme of leucine biosynthesis. Complements a leu2 auxotroph, and is one of the four markers the pRS shuttle-vector series is built on.
- **Sources**: UniProt `P04173` -> ENA CDS `X59720.2:91323..92417`  (1095 nt, 364 aa, initiator ATG)
- **Decision**: Ship the intact LEU2, not leu2-d.
- **Basis**: The high-copy leu2-d allele differs in how much upstream sequence it retains, i.e. in a promoter boundary, which SOURCING.md classes as a convention. If leu2-d is wanted it is a separate row and it is not in this database.

  ```
  python features/build/build.py --show PLF:1019
  ```

### PLF:1020 -- HIS3 (Imidazoleglycerol-phosphate dehydratase)

- **Claims**: Imidazoleglycerol-phosphate dehydratase of Saccharomyces cerevisiae, the sixth step of histidine biosynthesis. Complements a his3 auxotroph. The enzyme is competitively inhibited by 3-aminotriazole, which is what makes HIS3 a tunable reporter in two-hybrid work: raising the inhibitor raises the expression threshold a colony has to clear before it grows.
- **Sources**: UniProt `P06633` -> ENA CDS `Z75110.1:238..900`  (663 nt, 220 aa, initiator ATG)
- **Decision**: Ship the S288c 220 aa sequence.
- **Basis**: THE CLASSIC HIS3 CLONE IS NOT THIS SEQUENCE and the automated survey cannot say so: three cross-references including CAA27003 are 219 aa, and for unequal lengths the survey only reports that positions are not comparable. Aligned from both ends by hand they agree for 108 residues, agree again over the last 109, and differ only in the window at residue 109 -- a one-residue indel plus a substitution. Older vectors descend from that clone.

  ```
  python features/build/build.py --show PLF:1020
  ```

### PLF:1021 -- TRP1 (N-(5'-phosphoribosyl)anthranilate isomerase)

- **Claims**: N-(5'-phosphoribosyl)anthranilate isomerase of Saccharomyces cerevisiae, the third step of tryptophan biosynthesis. Complements a trp1 auxotroph.
- **Sources**: UniProt `P00912` -> ENA CDS `V01341.1:103..777`  (675 nt, 224 aa, initiator ATG)
- **Decision**: Ship the ORF alone.
- **Basis**: 'TRP1' ON A MAP USUALLY MEANS TRP1-ARS1. In YRp7 and descendants the label covers this gene together with the adjacent ARS, which is what makes the plasmid replicate. ARS1 is a separate element, its boundary is a convention, and it is not in this database.

  ```
  python features/build/build.py --show PLF:1021
  ```

### PLF:1022 -- TK (Thymidine kinase)

- **Claims**: Thymidine kinase of herpes simplex virus type 1, gene UL23. Much less selective than the cellular enzyme, it phosphorylates nucleoside analogues such as ganciclovir and aciclovir, which are then extended to triphosphates that poison DNA synthesis. That promiscuity is the point: the gene is the classic negative-selection and suicide marker, killing the cells that carry it as soon as the prodrug is supplied.
- **Sources**: UniProt `P0DTH5` -> ENA CDS `complement(X14112.1:46672..47802)`  (1131 nt, 376 aa, initiator ATG)
- **Decision**: Ship strain 17 UL23, display name 'TK'.
- **Basis**: TWO THINGS TO DECIDE. (1) LOOKUP GAP: the name is UniProt's gene symbol and here that is two letters, so 'HSV-TK', 'HSVtk' and 'UL23' -- the spellings on real maps -- are NOT aliases and will not resolve. Adding them means writing names ourselves into a column sourced entirely from UniProt under CC BY. (2) STRAIN: a second reviewed 376 aa HSV thymidine kinase exists under a different accession and differs at four positions; a construct built from it is not corrupt. PATENT FLAGGED, NOT ADJUDICATED.
- **PATENT FLAGGED, NOT ADJUDICATED.**

  ```
  python features/build/build.py --show PLF:1022
  ```

### PLF:1023 -- Dhfr (Dihydrofolate reductase)

- **Claims**: Mouse dihydrofolate reductase. Reduces dihydrofolate to tetrahydrofolate, the one-carbon donor for thymidylate and purine synthesis. Used as a selection marker in DHFR-negative CHO lines, and as an AMPLIFICATION marker: stepping methotrexate up selects for cells that have amplified the locus, and a linked transgene is amplified with it.
- **Sources**: UniProt `P00375` -> ENA CDS `BC005796.1:49..612`  (564 nt, 187 aa, initiator ATG)
- **Decision**: Ship the cDNA, not the genomic join.
- **Basis**: The mRNA record and the six-exon genomic join give the identical 187 aa and their 564 nucleotides differ at exactly one position, 396, C here and T there, synonymous. The protein cannot tell them apart and a nucleotide match can. A vector carries the cDNA.

  ```
  python features/build/build.py --show PLF:1023
  ```

### PLF:1024 -- gpt (Xanthine-guanine phosphoribosyltransferase)

- **Claims**: Xanthine-guanine phosphoribosyltransferase of Escherichia coli. Salvages guanine, xanthine and hypoxanthine into their nucleotides. Mammalian cells cannot use xanthine this way, so in medium containing mycophenolic acid, which blocks de novo GMP synthesis, together with xanthine, only cells expressing this enzyme make GMP and survive.
- **Sources**: UniProt `P0A9M5` -> ENA CDS `U00096.3:255977..256435`  (459 nt, 152 aa, initiator ATG)
- **Decision**: Ship the bacterial enzyme.
- **Basis**: TWO DIFFERENT GENES ARE WRITTEN 'gpt'. This is the bacterial xanthine-guanine enzyme (often Ecogpt in mammalian work), not the mammalian HPRT.

  ```
  python features/build/build.py --show PLF:1024
  ```

### PLF:1025 -- bar (Phosphinothricin N-acetyltransferase)

- **Claims**: Phosphinothricin N-acetyltransferase from the bialaphos biosynthesis cluster of Streptomyces hygroscopicus. Acetylates the free amino group of phosphinothricin, the glutamine-synthetase inhibitor released from bialaphos and sold as the herbicide glufosinate. The standard herbicide-resistance selection marker for plant transformation.
- **Sources**: UniProt `P16426` -> ENA CDS `X17220.1:31..582`  (552 nt, 183 aa, initiator ATG)
- **Decision**: Ship the ATG form from the plant-transformation cassette record.
- **Basis**: THE BOUNDARY IS ONE BASE AND IT IS BASE 1. The native-locus record begins GTG; the pinned record begins ATG. Both 552 nt, both give the identical 183 aa, differing at exactly one nucleotide. Verified independently for this PR. A curator wanting the native gene pins the other record and must expect a position-1 mismatch against every construct. PATENT FLAGGED.
- **PATENT FLAGGED, NOT ADJUDICATED.**

  ```
  python features/build/build.py --show PLF:1025
  ```

### PLF:1026 -- pat (Phosphinothricin N-acetyltransferase)

- **Claims**: Phosphinothricin N-acetyltransferase of Streptomyces viridochromogenes. The same reaction and the same glufosinate selection as the bar gene of the row above, from a different producer strain. The two are used interchangeably in plant transformation and are distinct sequences, so a construct carrying one does not match the other at the nucleotide level.
- **Sources**: UniProt `Q57146` -> ENA CDS `X65195.2:29930..30481`  (552 nt, 183 aa, initiator GTG (non-AUG initiator read as Met))
- **Decision**: Ship pat as a separate row from bar.
- **Basis**: 'bar' and 'pat' are used interchangeably in the literature and are two genes. Whether a map's 'BlpR' or 'PPT-AT' label means this row or PLF:1025 cannot be settled from the label, only from the sequence. Non-ATG start: GTG. PATENT FLAGGED.
- **PATENT FLAGGED, NOT ADJUDICATED.**

  ```
  python features/build/build.py --show PLF:1026
  ```

### PLF:1027 -- rpsL (Small ribosomal subunit protein uS12)

- **Claims**: 30S ribosomal protein S12 of Escherichia coli, part of the decoding centre of the small subunit. The wild-type allele is DOMINANT SENSITIVE to streptomycin: a streptomycin-resistant host carries a mutant rpsL, and supplying the wild-type protein in trans restores sensitivity. That inversion is what makes the gene a counter-selectable marker -- an rpsL-neo cassette is selected onto a target with kanamycin and selected off it again with streptomycin.
- **Sources**: UniProt `P0A7S3` -> ENA CDS `complement(U00096.3:3474178..3474552)`  (375 nt, 124 aa, initiator ATG)
- **Decision**: Ship rpsL, and keep the alias collision documented rather than resolved.
- **Basis**: NOT A RESISTANCE GENE, AND THE ALIAS SAYS THE OPPOSITE. UniProt lists 'strA' as a synonym of rpsL. 'StrA' is also the NAME of PLF:0023, a plasmid aminoglycoside phosphotransferase conferring streptomycin RESISTANCE where this gene confers SENSITIVITY. Both usages are real; a caller resolving the alias to one record gets one of two genes with opposite phenotypes.

  ```
  python features/build/build.py --show PLF:1027
  ```

---

## Worked up and deliberately NOT shipped

SOURCING.md section 6 budgets about 40 Class B items. Seven survived the
rules; five more were built and refused on the extent evidence (above). The rest are recorded in `stage_classb.HELD` with the reason, so that
nobody re-does the work and concludes it was never done:

- **T3 promoter** -- No consensus to record. The two leading conventions are 17 nt and 19 nt, they are OFFSET rather than nested -- they share a 16 nt core and neither contains the other -- and each has exactly one independent submission behind it. Picking one would be a coin toss dressed up as consensus_of_insdc. A third deposit annotates the reverse complement, i.e. it got the strand wrong. The bases are unambiguous; the boundary is not.
- **SV40 early promoter** -- The 330 nt convention is a contiguous circular interval that WRAPS the numbering origin of the SV40 record, which this schema's accession:lo-hi:strand boundary_evidence cannot express, and a rival 283 nt form does not place as a single interval at all. The region also carries a tandem repeat, so the two forms may differ in repeat copy number -- that was NOT counted and is not offered as a finding.
- **U6 promoter (human)** -- The cleanest anchor of everything examined -- 249 nt, exact in the primary record for human U6 -- and only ONE independent submission witnesses it. Fails on witnesses, not on evidence. The cheapest of these to rescue.
- **H1 promoter (human)** -- Three independent submissions agree on 216 nt, which is a genuine consensus, but the sequence does not occur in the human H1 RNA record and no genomic record carrying the upstream promoter could be located. Boundary witnessed, provenance absent; a row would have nothing to put in boundary_evidence.
- **EF-1alpha promoter (human)** -- The 1144 nt vector element is the primary record's 1148 nt MINUS a four-base internal deletion, with both flanks exact. It is therefore not a verbatim slice of anything: a reference taken from the gene will not match real vectors, and one taken from a vector cannot be cited to the gene by coordinates. Needs an explicit decision about which sequence ships.
- **PGK promoter (mouse)** -- Same shape as EF-1alpha and worse: exact for 67 nt, then a single-base shift, and roughly 48 substitutions across the rest. One submission. Two independent reasons to hold.
- **CAG promoter** -- NOT ONE ELEMENT. The widely deposited 1342 nt 'AG promoter' begins in the chicken beta-actin gene and ends in rabbit beta-globin and contains no cytomegalovirus sequence at all; a 935 nt 'CAG promoter' begins in the cytomegalovirus enhancer. Merging two different elements under one near-identical name would be the worst error available in this set.
- **araBAD / pBAD promoter** -- Three extents from three submissions, two of them SnapGene-annotated, and no Escherichia coli ara locus record fetched to anchor any of them. Insufficient on both legs. Note PLF:1002 already carries araC, the regulator; the promoter it works on is the hole.
- **tetO / TRE / Ptet** -- DROPPED, not held. The name covers at least four unrelated elements -- a bacterial PLtetO-1, a bacterial pTet, a mammalian bidirectional TRE, and a CMV-tetO2 hybrid -- with nothing in common but the word. It must be split into separately named rows before any part of it can be sourced at all.

The recurring reasons are worth naming: **one witness is not two**
(U6, PGK), **the element is not a verbatim slice of anything** (EF-1alpha,
PGK), **the schema cannot express the interval** (SV40 early promoter, which
wraps the numbering origin), and **the name covers more than one element**
(CAG, tetO/TRE). The last is the dangerous one and is why tetO was dropped
outright rather than held.

---

## If you reject a row

Delete nothing by hand. The rows are generated: remove the item from
`stage_classb.ITEMS` or `stage_uniprot.ITEMS`, add it to the stage's `HELD`
tuple **with the reason**, and rebuild. IDs are allocated from where a row is
*declared*, never from how many survived, so dropping one does not renumber
anything after it -- and the build re-reads the previous table and refuses to
write if any published id changed meaning.
