# The SnapGene `.dna` container format

An empirical specification, derived by black-box observation of a 41-file
corpus of real `.dna` documents (138 bp fragments through a 4.64 Mb bacterial
genome; files written by SnapGene versions spanning export/import version
pairs 10/5, 10/7, 11/10, 11/11, 13/12 and 15/18–19).

**Method and scope.** Everything below comes from reading the bytes of `.dna`
files. No vendor binary was disassembled or decompiled, and no vendor data
file is reproduced here. Recognition-site strings quoted as examples are
biological facts available from REBASE and any molecular-cloning textbook.

**Status.** Reading is fully solved: all 41 files parse, and every byte of
every file is accounted for by the framing below. Writing is solved for
everything except two cache blocks, which appear to be safely omissible.

---

## 1. Container framing

A `.dna` file is a flat, ordered stream of length-prefixed blocks. There is no
index, no directory, and no trailer.

```
block  :=  type:uint8   length:uint32be   payload:length bytes
file   :=  header_block  block*
```

All multi-byte integers are **big-endian**. Blocks are read until EOF. An
unknown block type is skipped by its declared length, which is what makes the
format forward-compatible — and what lets a third-party writer preserve
blocks it does not understand.

Validation across the corpus: `sum(5 + length)` over all blocks equalled the
file size exactly for **41 / 41** files.

### The header block (type 9)

Must be first. Payload is 14 bytes:

| offset | size | field | observed values |
|---|---|---|---|
| 0 | 8 | magic `"SnapGene"` | constant |
| 8 | 2 | file type (uint16be) | `1` = DNA |
| 10 | 2 | export version (uint16be) | 10, 11, 13, 15 |
| 12 | 2 | import version (uint16be) | 5, 7, 10, 11, 12, 18, 19 |

The two version numbers track the writing application and the minimum reader
required. They vary independently and a parser should not branch on them
without cause — the block payloads themselves were stable across every
version in the corpus.

---

## 2. Block catalogue

Counts are occurrences across the 41-file corpus.

| type | n | total bytes | contents |
|---:|---:|---:|---|
| 0 | 41 | 16,869,434 | **DNA sequence** — 1 flags byte + ASCII bases |
| 2 | 41 | 36,104,133 | **cut-site cache** — derived, see §4 |
| 3 | 41 | 9,744,510 | **enzyme table** — derived, see §4 |
| 5 | 12 | 20,756 | **primers** — XML |
| 6 | 41 | 10,939 | **notes** — XML |
| 7 | 3 | 33,987 | **history tree** — xz-compressed XML |
| 8 | 25 | 6,899 | **additional sequence properties** — XML |
| 9 | 41 | 574 | file header |
| 10 | 19 | 50,064 | **features** — XML |
| 11 | 10 | 56,702 | history node |
| 13 | 13 | 4,485 | small ancillary, unresolved |
| 14 | 7 | 287 | small ancillary, unresolved |
| 17 | 7 | 315 | trace/alignment data |
| 28 | 7 | 357 | small ancillary, unresolved |

Types 13, 14, 17 and 28 together account for 5,444 bytes out of 62.9 MB —
0.009% of the corpus. Passing them through verbatim costs nothing and loses
nothing.

---

## 3. Block 0 — sequence and topology

```
payload := flags:uint8   bases:ASCII[length-1]
```

Bases are plain uppercase ASCII, IUPAC-degenerate codes permitted. No
compression, no packing — one byte per base. (A 4.64 Mb genome therefore
carries a 4.64 MB sequence block.)

### Flags byte

| bit | mask | meaning |
|---:|---:|---|
| 0 | `0x01` | circular topology |
| 1 | `0x02` | double-stranded |
| 2 | `0x04` | Dam methylated |
| 3 | `0x08` | Dcm methylated |
| 4 | `0x10` | EcoKI methylated |

Observed values in the corpus: `0x1f` (circular, double-stranded, all three
methylations — 19 files), `0x1e` (the same but linear — 19 files), and `0x02`
(linear, double-stranded, no methylation — 3 files, all written by older
versions). The methylation bits gate which restriction sites are reported as
blocked, so they are semantically load-bearing, not cosmetic.

---

## 4. Blocks 2 and 3 — the derived caches

**These two blocks are the bulk of a typical `.dna` file, and they are
regenerable.** This is the single most useful thing to know about the format.

Across the corpus, discarding blocks 2 and 3 shrinks files by a **mean of 78%
(max 96%)** while losing no user-authored information.

### Block 3 — enzyme recognition-site table

```
payload := version:uint8            (observed: 1)
           asciiLen:uint32be
           sites:ASCII[asciiLen]    comma-separated IUPAC strings
           <packed match data>
```

The ASCII section is a literal comma-separated list of recognition sequences:

```
GACNNNNNNGTC,CCAGGCCTGG,GATCCGGATC,CCWGGTACCWGG,CCANNNNNTGG,...
```

100% of tokens are pure IUPAC. Site counts vary per file — 469, 473 and 485
were all observed — which indicates this is a snapshot of *the enzyme set
active when the file was saved*, not a fixed global table. Length
distribution in a representative file: 23 four-base sites, 58 five-base, 142
six-base, and a long tail out to 37 bases.

### Block 2 — cut-position index

Scales at roughly **2.13 bytes per base** above a fixed floor of ~1.8 kB, and
is high-entropy and near-incompressible (zlib ratio 0.96), consistent with
packed integers rather than text. Read as `uint32be`, the leading values are
small and non-monotonic — consistent with per-enzyme runs of positions or
deltas.

**Honest limitation:** the precise record layout is *not* resolved. A specific
hypothesis — a version byte followed by interleaved
`[count][positions...]` records, one per block-3 site — was tested against
14 files and **rejected** (it desynchronises partway through every file). The
correct layout is likely a counts table separated from a positions table, or a
variable-width encoding, but this has not been confirmed.

**Why it does not matter.** Both blocks are pure functions of
`(sequence × enzyme set)`. Block 3 stores the enzyme patterns; block 2 stores
where they hit. Neither can contain information a user typed. A writer should
recompute or omit them rather than reproduce them byte-for-byte.

> **The one open question worth an experiment:** does SnapGene *require*
> blocks 2 and 3 on read, or does it regenerate them? Every file in the corpus
> was written by SnapGene and every one contains them, so the corpus cannot
> answer this. Write a file without them and open it. If it opens, `.dna`
> write support is essentially complete.

---

## 5. Block 10 — features

Plain UTF-8 XML.

```xml
<Features nextValidID="9">
  <Feature recentID="3" name="phoE" type="CDS" directionality="1"
           translationMW="149.21" allowSegmentOverlaps="0"
           consecutiveTranslationNumbering="1">
    <Segment range="2163-2165" color="#993366" type="standard" translated="1"/>
    <Q name="codon_start"><V int="1"/></Q>
    <Q name="transl_table"><V int="1"/></Q>
    <Q name="translation"><V text="M"/></Q>
  </Feature>
</Features>
```

- **Segment coordinates are 1-based and inclusive**, written `start-end`.
  Do not generalise this to the rest of the file — `<BindingSite location>` in
  block 5 uses the *same syntax* with a *different origin*. See §6.1.
- A feature owns **one or more `<Segment>`s**, which is how joins, exon
  structures and origin-spanning features are represented. Any data model that
  assumes one interval per feature will lose information.
- `directionality`: `1` forward, `2` reverse, `3` both, absent = unoriented.
- `color` is a per-segment `#rrggbb`, so a multi-segment feature can be
  parti-coloured.
- Qualifiers have **two spellings, and a reader must accept both**:
  - short, seen on export 11 / import 10 and above —
    `<Q name=…><V text=… | int=… | predef=…/></Q>`;
  - long, seen on export 10 / import 5 — `<Qualifier name=…><QualifierValue
    textVal=… | intVal=… | predefVal=…/></Qualifier>`.

  Either maps cleanly onto GenBank qualifiers in both directions. This section
  documented only the short form until 2026-07-29, and that omission is the
  upstream cause of a real defect rather than a documentation nicety: the Rust
  reader *and* the Python cross-check oracle both matched `<Q>`/`<V>` alone, so
  on a file of the older vintage the two agreed perfectly about a feature set
  from which **both** had silently dropped every qualifier — `/locus_tag`,
  `/codon_start`, `/transl_table`, `/direction`, and whole protein
  `/translation` strings. Measured on `pKoV with His decR.dna`: 10 qualifiers
  lost, three of them full translations. A qualifier element carrying no value
  element is a *valueless* qualifier — GenBank's bare `/pseudo` — and not an
  absent one.

## 6. Block 5 — primers

```xml
<Primers nextValidID="5">
  <HybridizationParams minContinuousMatchLen="10" allowMismatch="1"
                       minMeltingTemperature="40"
                       showAdditionalFivePrimeMatches="1"
                       minimumFivePrimeAnnealing="15"/>
  <Primer recentID="1" name="Fab2_D_SalI"
          sequence="atatGTCGACTTAGAATATAACTCTTAGTCCTACTCCACC">
    <BindingSite location="13493-13552" boundStrand="1"
                 annealedBases="GTCGACTTAGAATATAACTCTTAGTCCTACTCCACC"
                 meltingTemperature="53">
      <Component bases="atat"/>
      <Component hybridizedRange="13544-13552" bases="GTCGACTTA"/>
      <Component hybridizedRange="13493-13519" bases="GAATATAACTCTTAGTCCTACTCCACC"/>
    </BindingSite>
    <BindingSite simplified="1" …/>
  </Primer>
</Primers>
```

### 6.1 `location` is 0-based — unlike `range`

**The single most dangerous detail in this format.** `<Segment range="a-b">` is
1-based inclusive. `<BindingSite location="a-b">` is **0-based** inclusive. They
share a syntax, sit in the same document, and mean different things.

The evidence, gathered over 344 files rather than assumed:

| attribute | smallest start seen | ends exactly at `len` | start codon under 1-based | under 0-based |
|---|---|---|---|---|
| `Segment range` | 1 | yes (4×) | 21 | 1 |
| `BindingSite location` | 0 | — | 0 | 32 |

The binding-site column needs no inference at all, because the format checks
itself: each site records the `annealedBases` it claims to cover. Slicing the
sequence 0-based reproduces that string; slicing it 1-based does not, in 32 of
32 unambiguous cases. Segments answer to biology instead — read 1-based, 21
translated forward CDSs begin with `ATG` and 18 end on a stop codon; read
0-based, one does.

Two traps follow:

- A reader that treats both alike is wrong by one base for **every primer**, and
  right about every feature — so the bug looks like a primer-specific mystery
  rather than a coordinate convention.
- It **survives round-trip testing invisibly**. A writer that re-emits the
  original block (as ours does) cancels the error on the way out, so byte-exact
  reproduction proves nothing here. Only comparing coordinates against the
  sequence catches it. This one was found by validating annotations against the
  bases they claim to describe, on real files.

`<Component hybridizedRange>` follows `location`, not `range`.

Two further things here are worth more than the rest of the format combined:

1. **`<HybridizationParams>` documents the binding-site search parameters**
   directly — minimum contiguous match of 10, mismatches allowed, minimum Tm
   40 °C, minimum 5′ annealing of 15. That is most of the specification for a
   compatible primer-binding-site finder.
2. **`<Component>` decomposes each binding site** into non-annealing 5′ tail
   versus annealed body, with a separate `hybridizedRange` per annealed run —
   which is exactly what is needed to render a primer with a restriction-site
   tail correctly.

Each site appears twice, once normally and once with `simplified="1"`; the
simplified entry is a collapsed view of the same site and must be de-duplicated
or every primer will appear to bind twice.

Lowercase bases in `sequence` are meaningful to users (conventionally the
added tail) and should be preserved verbatim, not normalised.

## 7. Block 6 — notes

```xml
<Notes>
  <UUID>bf2554c0-4517-48ab-9da1-522b92a73000</UUID>
  <Type>Synthetic</Type>
  <ConfirmedExperimentally>0</ConfirmedExperimentally>
  <Created UTC="22:0:0">2022.12.13</Created>
  <LastModified UTC="22:0:0">2022.12.13</LastModified>
  <SequenceClass>UNA</SequenceClass>
  <TransformedInto>DH5α™</TransformedInto>
</Notes>
```

Note the UUID: it is a stable document identity, useful for detecting that two
files are versions of the same construct. Dates are `YYYY.MM.DD` with a
separate `UTC` attribute carrying the time.

**The example above is a skeleton, not the whole vocabulary.** A later survey of
32 block-6 payloads written by SnapGene between 2018 and 2026 found seven more
elements — `Description`, `Comments`, `CustomMapLabel`, `UseCustomMapLabel`,
`ConfirmedExperimentally`, `Type` and `References` — and two shapes this section
did not describe at all:

```xml
<Description>Constitutive mNeonGreen-expression vector for Fusobacterium nucleatum.</Description>
<Comments>digestion with XhoI results in 2 fragments approximatly 6 KB long.&lt;br&gt;</Comments>
<References>
  <Reference title="Expanding the genetic toolkit ..." pubMedID="36161895"
             journal="Proc Natl Acad Sci U S A. 2022 Oct 4;119(40):e2201460119. ..."
             authors="Ponath F, Zhu Y, Cosi V, Vogel J"/>
</References>
```

Traps, all observed:

- **`<References>` nests.** 3 of the 32 payloads carry a `<Reference/>` a level
  below the note. Anything that reads block 6 as a flat name→text mapping loses
  the whole citation, and gets `"\n"` — the container's own whitespace — as the
  value of `<References>` if it takes `.text` at face value.
- **`<Reference>` attribute order is not stable.** The same citation is written
  `title,pubMedID,journal,authors` in one file and
  `authors,pubMedID,title,journal` in another, so attributes must be stored
  ordered if a rewrite is to be quiet.
- **`UTC` is not zero-padded.** `21:55:7`, `22:0:0` and `9:16:3` all occur.
  Parsing it into a time type and reformatting rewrites the bytes for nothing;
  store the string. 21 of the 32 payloads carry it on both `<Created>` and
  `<LastModified>`, 11 on neither, and none on one alone.
- **Entity escaping is asymmetric in the wild.** One `<Comments>` contains
  `&lt;br>` — `<` escaped, `>` bare. Unescaping and re-escaping normalises that
  to `&lt;br&gt;`: the same text spelled differently, so block 6 must be compared
  as structure and not as bytes.
- **`DH5α™` above is this document's own invention.** The values actually
  observed are `DH5α`, `Unspecified` and `unspecified`.

Two more traps that no file here exhibits, and which a writer therefore has to
be told about rather than shown:

- **Block 6 is XML, so anything writing it must be well-formed twice over.** An
  element name is an XML `Name` and no amount of escaping makes an arbitrary
  string into one; and an attribute name may appear at most once per element
  (XML 1.0 §3.1, "Unique Att Spec"). A lenient reader — this project's own
  `xml::scan`, and evidently SnapGene's, since it produced the `&lt;br>` above —
  will happily read a repeated attribute back, so a writer that copies what it
  read straight out again produces a file that a *strict* parser refuses
  outright. That is worse than dropping the attribute, because it costs the
  whole document.
- **Whitespace inside an attribute value does not survive being written raw.**
  XML requires attribute-value normalization (TAB, LF and CR become a space
  before the value reaches the application) and line-end normalization (a
  literal CR anywhere becomes LF). Only `&#9;`/`&#10;`/`&#13;` survive both. The
  place this will bite is `<Reference journal="...">`, whose value is pasted
  citation text.

**What Polylinker does with all of this.** `Molecule::notes` is a `Vec<Note>`,
where a `Note` is a key, its text and its attributes in file order, so a
`<Created UTC="22:0:0">` keeps its time of day through read, model and
`from_molecule`'s synthesised write. A note with no text — `<Empty/>`, or
`<Created UTC="..."/>` — is a note with an empty value, not an absence.

What is *not* modelled is anything block 6 can express that a flat note cannot.
All of it is reported by path through `LoadReport::unrepresentable_notes`,
printed by both `pl info` and `pl convert`, rather than dropped or flattened:

| shape | reported as |
| --- | --- |
| a subtree under a note, at every depth | `Notes/References/Reference` |
| a note's own text following a nested child | `Notes/Comments/text()` |
| an attribute on `<Notes>` itself | `Notes@version` |

The middle row is the subtle one. `<Comments>Grown at 37 <sup>o</sup>C
overnight</Comments>` has two text runs with a hole between them, and
concatenating them yields `Grown at 37 C overnight` — a sentence that is *not in
the file* and that reads as though it were. The value is the text before the
first child, which is also what `ElementTree.text` gives
`reference/python/snapdna.py`, so the two implementations agree; the tail is
named instead of guessed at.

## 8. Block 8 — additional sequence properties

```xml
<AdditionalSequenceProperties>
  <UpstreamStickiness>0</UpstreamStickiness>
  <DownstreamStickiness>0</DownstreamStickiness>
  <UpstreamModification>FivePrimePhosphorylated</UpstreamModification>
  <DownstreamModification>FivePrimePhosphorylated</DownstreamModification>
</AdditionalSequenceProperties>
```

Small, but essential for any cloning simulation: this is where a linear
fragment's overhangs and terminal phosphorylation state live. Ligation
compatibility cannot be computed without it.

## 9. Block 7 — history tree

**The payload is xz-compressed** (magic `FD 37 7A 58 5A 00`), inflating around
7× to UTF-8 XML. A parser that treats block 7 as text will see binary garbage.

```xml
<HistoryTree>
  <Node name="pACYC184-Ppho-fab2-6his.dna" type="DNA" seqLen="15647"
        strandedness="double" ID="8" circular="1" operation="insertFragment">
    <RegeneratedSite name="BamHI" pos="1870" siteCount="1"/>
    <RegeneratedSite name="SalI" pos="13548" siteCount="1"/>
    <HistoryColors>…</HistoryColors>
    <InputSummary manipulation="replace" name1="SalI" name2="BamHI"
                  val1="2146" val2="1870" siteCount1="1" siteCount2="1"/>
    <Node name="pACYC184.ape" type="DNA" seqLen="4245" circular="1" …>
      <Features>…</Features>
    </Node>
  </Node>
</HistoryTree>
```

This is a genuinely valuable structure and nothing else in the ecosystem has
an equivalent: a recursive record of every construct that contributed to this
one, each parent embedded whole with its own sequence and features, annotated
with the operation and enzymes that joined them. It is the provenance graph of
the plasmid.

Only 3 of 41 corpus files carried one, so it is written only for constructs
actually built inside SnapGene — but for those, it is the feature that is
hardest to replace and therefore the strongest reason a user stays.

---

## 10. Implementation notes

**Read order.** Do not assume block ordering. Observed order is commonly
9, 0, 2, 3, 5, 6, 8, 10 but this is not guaranteed; dispatch on type.

**Round-tripping.** Retaining every block verbatim and re-emitting them
reproduces the original file byte-for-byte. Verified on 41/41 files. This is
the safe default for any editor that does not yet model every block: mutate
the blocks you understand, pass the rest through.

**Synthesising, as opposed to round-tripping, is where the losses live.** A
writer that rebuilds the file from a parsed model — which is what converting
*into* `.dna` from any other format has to do — loses everything the reader did
not keep, and a byte-exact round-trip test cannot see any of it, because the
original blocks never went through the model. Block 6 is the worked example: a
reader that models a note as name→text drops `<Created UTC="22:0:0">`'s time of
day, the writer re-emits `<Created>2022.12.13</Created>`, and both a byte-exact
round-trip and a model→file→model comparison stay green, the latter because the
loss happens identically on both sides. Compare a synthesised block against the
**original payload**, not against a re-parse of your own output.

**Large files.** A 4.64 Mb genome is a 17.7 MB `.dna` file of which ~74% is
the derived cache. Streaming or memory-mapping matters; so does not rendering
4.6 million bases into a DOM.

**Reference implementations produced alongside this document**

| | language | reads | writes | verified |
|---|---|---|---|---|
| `snapdna.py` | Python, stdlib only | yes | yes, byte-exact | 41/41 corpus files |
| `dna-reader.html` | JavaScript, no dependencies | yes | yes, from scratch | cross-read by `snapdna.py` |

The JavaScript writer emits a valid file *without* blocks 2 and 3, which is
the experiment described in §4.
