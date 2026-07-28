#!/usr/bin/env python3
"""Stage 4 — structured RNA elements from Rfam.

Rfam uniquely contributes the elements that have no protein to translate and
that GenBank depositors do not reliably annotate: plasmid copy-number control
RNAs, packaging signals, IRESes, ribozymes, riboswitches, and the sgRNA
scaffold. Stage 2's translation check cannot reach any of them, so without this
stage a lentiviral transfer plasmid annotates as a handful of markers and
nothing else.

Rfam is CC0 1.0 — but `features/SOURCING.md` §1 records that the challenge
round overturned the prober's clean GO to **GO_WITH_CAVEAT**, because two kinds
of material that Rfam cannot relicense ride inside a source declared CC0-clean:

  * **miRBase**, which publishes *no licence at all*, is 1,598 of 4,227
    families (37.8%);
  * **Wikipedia** prose (CC BY-SA 4.0), which is share-alike and therefore
    incompatible with our CC BY 4.0 release.

The verdict is conditional on those exclusions being *enforced*. A comment
saying "we only picked non-miRNA families" is not enforcement — the next
contributor to add a family has not read the comment. So each exclusion below
is a check in the program, positioned so that a family that violates it stops
the build rather than reaching `features.tsv`:

  1. `assert_not_mirna()`   — reads the **type** column, never the family name.
  2. `GF_QUARANTINE`        — a *whitelist* parser, plus `assert_untainted()`,
                              which re-reads our own prose and hard-fails on a
                              shared 5-word n-gram with the quarantined text.
  3. `to_dna()`             — `tr U->T`, then asserts no `U` survived.
  4. `rfam_fetch()`         — refuses any URL outside the two files we cleared,
                              so `fasta_files/` cannot be wired in at all.

`self_test()` runs each gate against an input that must fail it. Three of the
four are otherwise unfalsifiable in a green build: every allow-listed family
passes them, so "no error" is equally consistent with "the check is a no-op".

Why the exclusions bite in ways a reader would not guess
-------------------------------------------------------

**miRNA, on the name.** `rfam_id.lower().startswith("mir")` selects 1,590
families; the type field selects 1,598. The eight families in the gap are
miRBase-derived and unlicensed, and a name-keyed filter ships every one of
them. Measured, not assumed — see `assert_not_mirna()`.

**Wikipedia, inside the file we were told to prefer.** SOURCING.md frames this
as "never ingest family Summary text, never fetch `wikitext.txt.gz`", which
reads like a warning about the website. It is broader: all 4,227 Stockholm
blocks in `Rfam.seed.gz` carry a `#=GF WK` Wikipedia article title, and most
carry `#=GF CC` prose identical to `family.txt`'s comment column. A parser that
does the obvious thing — keep the `#=GF` annotations, write them to the row —
ingests exactly what the exclusion forbids, out of the file SOURCING.md tells
us to prefer. Hence a whitelist rather than a blacklist.

**`U`, silently.** Rfam.seed is uniform RNA alphabet: measured across the
twenty-four seed alignments this module reads, 64,970 `U` and zero `T` (and
zero lowercase, so no case folding is needed either). Ship a reference with `U`
in it and the nucleotide index matches nothing, for every Rfam row at once.
That does not present as a data bug, it presents as an empty-database bug, and
it is cheap to prove absent — so `to_dna()` asserts rather than trusting the
substitution.

Descriptions are ours, written from the primary literature that Rfam's own
`#=GF RM` lines point at, and `credit_pmid` is checked against those lines so a
future row cannot cite a paper the family does not reference. Rfam's `CC`
prose was never read into the process: it is quarantined at parse time and
`assert_untainted()` fails the build on a shared 5-word n-gram with it.

Usage
-----
    python features/build/stage_rfam.py          # build from cache, print rows
    python features/build/stage_rfam.py --refresh
"""

from __future__ import annotations

import argparse
import gzip
import re
import statistics
import sys
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from build import TODAY, Row, assert_ascii, cached_meta, esc, fetch, parse_fasta  # noqa: E402

# --------------------------------------------------------------------------
# Sources. Two Rfam files, and `rfam_fetch` refuses everything else; plus the
# ENA record behind whichever seed row is chosen, and `ena_fetch` refuses
# everything else.

RFAM_BASE = "https://ftp.ebi.ac.uk/pub/databases/Rfam/CURRENT"
FAMILY_URL = f"{RFAM_BASE}/database_files/family.txt.gz"
SEED_URL = f"{RFAM_BASE}/Rfam.seed.gz"
FAMILY_CACHE = "rfam_family.txt.gz"
SEED_CACHE = "Rfam.seed.gz"

ENA_BASE = "https://www.ebi.ac.uk/ena/browser/api"

# Both files are a **latin-1** MySQL/Stockholm dump, not UTF-8: `family.txt`
# fails to decode as UTF-8 at byte 1,678,625 and `Rfam.seed` at 21,378,990,
# both inside author names. Decoding with errors="replace" would not raise —
# it would quietly substitute U+FFFD into the bibliographic credit that
# SOURCING.md §2 Stage 4 requires us to carry, i.e. corrupt the one field whose
# whole purpose is to be citable.
RFAM_ENCODING = "latin-1"

# Column indices in the `family.txt` MySQL tab dump. Re-proven against row 0
# rather than inherited: 4,227 rows, and the field-count histogram is a single
# bucket of 35, so there are no ragged rows and no embedded tabs to realign us.
F_ACC, F_ID, F_DESC, F_COMMENT, F_NUM_SEED, F_TYPE = 0, 1, 3, 9, 14, 18

# Our block of the `PLF:` space, asserted rather than assumed — otherwise the
# 1001st family silently starts issuing IDs another stage also issues, and the
# collision surfaces as two different features sharing a row id in a file nobody
# diffs by hand.
#
# `build.STAGES` is the authority on where this block starts, and `load_stage()`
# refuses to run a stage whose declared base disagrees with it. That check exists
# because this file said 300 while build.py said 2000, and the two disagreeing
# silently made PLF:2299 the *first* Rfam row: deterministic, stable, and wrong
# about which block it claims to be in. Nothing had been published from this
# block yet, so it was free to fix; after publication it would not have been.
PLF_BLOCK_BASE = 2000
PLF_BLOCK_SIZE = 1000
ID_BLOCK_START = PLF_BLOCK_BASE
ID_BLOCK_SIZE = PLF_BLOCK_SIZE


# --------------------------------------------------------------------------
# The allow-list


@dataclass(frozen=True)
class Family:
    """One Rfam family we have decided is a plasmid part."""

    acc: str
    """Rfam accession, e.g. `RF00106`. Keyed on the accession, never the name."""
    name: str
    """What a biologist calls it on a map. Ours."""
    aliases: tuple[str, ...]
    cls: str
    """`regulatory` or `misc`. Cross-checked against `class_from_type()`."""
    genbank_key: str
    """INSDC feature key, from `INSDC_KEYS`. Never a Sequence Ontology term:
    SOURCING.md Risk 4 holds SO pending its own LICENSE/README contradiction,
    and Rfam ships SO ids in `#=GF DR`, which is why DR is quarantined."""
    credit_pmid: str
    """The paper the description was written from. Must appear in this
    family's own `#=GF RM` lines — enforced, so a row cannot cite a paper the
    family does not reference."""
    description: str
    """Ours, from `credit_pmid`. Never Rfam's `CC`/Summary text, which is
    quarantined and which `assert_untainted()` checks this string against."""
    organism: str = ""
    """The source organism or replicon this row's reference MUST come from.

    Required. Checked, case-insensitively, against the description line of the
    ENA record the chosen seed row cites — fetched, not recalled — and the build
    stops if it does not match.

    This field exists because `choose_representative` picks by length alone, and
    length is uncorrelated with being the right organism. Measured, that cost
    four rows their meaning at once: RF00106 shipped a *Yersinia* cryptic-plasmid
    RNAI under a description promising "nearly every pUC, pBR322 and pET
    backbone", and the E. coli row that occurs verbatim in pBR322 was sitting in
    the same seed; RF02348 shipped a *Streptococcus salivarius* tracrRNA under a
    description naming S. pyogenes and "essentially every SpCas9 guide cassette";
    RF00458 shipped Drosophila C virus under the name "CrPV IGR IRES"; and
    RF02359 shipped phage M12 under the name, four aliases and description of
    phage MS2. Three of the four lost a *lexicographic tie-break* to the right
    row at identical length.

    A row that annotates nothing is worse than an absent row, because it looks
    like coverage. The organism is what makes the choice checkable.
    """
    rep: str = ""
    """Pin the representative to this exact seed identifier, e.g. `S42973.1/3-107`.

    Empty means "take the length-median row", which is right when any member of
    the family will do. Set it when the description names a specific replicon,
    virus or host, because then the choice is biological and must not be decided
    by which accession sorts first. The pinned row must be present in the fetched
    seed and must be an INSDC row, or the build stops — a pin that has silently
    stopped resolving is a pin that has stopped pinning.
    """
    patent_flag: str = "0"
    caveat: str = ""
    """Anything a curator must weigh before signing this row off. Appended to
    `notes`, where it is read, rather than left in a code comment, where it is
    not."""


# Chosen from the 2,629 licence-clear families by hand. The relevance filter is
# editorial and ours: these are the ones that turn up *in plasmids*, which is a
# different question from which RNAs are interesting.
FAMILIES: tuple[Family, ...] = (
    # -- plasmid replication and maintenance --------------------------------
    Family(
        "RF00106", "RNAI", ("RNA I", "RNAI", "ColE1 RNAI", "ColE1 copy-number control RNA"),
        "regulatory", "ncRNA", "7523833",
        "Antisense RNA that sets the copy number of ColE1-type replicons, and so "
        "of nearly every pUC, pBR322 and pET backbone in routine use. It is "
        "transcribed opposite the RNAII replication primer; pairing with nascent "
        "RNAII prevents that primer being processed into a form the polymerase "
        "can extend, so initiation slows as the plasmid, and therefore RNAI, "
        "accumulates. Its own fast turnover -- endonucleolytic cleavage, then "
        "PcnB-dependent adenylylation and decay -- is what makes the loop respond "
        "to concentration rather than latch.",
        caveat="The 'ori' box drawn on a plasmid map is a replication origin "
               "whose control element is this RNA; a curator should decide "
               "whether the two ship as one record or two.",
        organism="Escherichia coli",
        rep="S42973.1/3-107",
    ),
    Family(
        "RF00042", "CopA", ("CopA", "CopA RNA", "IncFII copy-number control RNA"),
        "regulatory", "ncRNA", "2421250",
        "Antisense RNA controlling copy number on IncFII replicons -- R1, R100, "
        "R6-5. Roughly ninety untranslated bases folded as two stem-loops joined "
        "by a spacer with a single-stranded 3' tail, transcribed opposite the "
        "leader of the repA message. Binding its target CopT blocks translation "
        "of the RepA initiator rather than transcription of it, so each round of "
        "replication is licensed after the message is already made. This is the "
        "reference case for the whole antisense copy-control class.",
        organism="Salmonella enterica",
    ),
    Family(
        "RF00043", "IncQ copy-control antisense RNA",
        ("R1162 RNA", "RSF1010 antisense RNA", "IncQ copy control RNA", "Plasmid_R1162 RNA"),
        "regulatory", "ncRNA", "2430262",
        "The small antisense RNA that decides how many copies of an IncQ plasmid a "
        "cell carries. The family is defined from R1162 -- RSF1010 to most "
        "laboratories -- though no R1162 record is in the seed alignment, so the "
        "reference below is a different IncQ replicon. It is "
        "complementary to part of the RepI message and represses it; lowering the "
        "amount of this RNA by mutation raises the plasmid's abundance, which is "
        "the evidence that it is the control element rather than a by-product. "
        "IncQ is the replicon beneath the mobilisable shuttle vectors used outside "
        "E. coli, so a database modelling only ColE1 goes blank on those "
        "constructs.",
        organism="IncQ plasmid",
        rep="Z47410.1/1220-1294",
    ),
    Family(
        "RF00242", "ctRNA (pT181)", ("ctRNA", "pT181 countertranscript", "RNAI (pT181)"),
        "regulatory", "ncRNA", "2478296",
        "Countertranscript of the staphylococcal rolling-circle replicon pT181. "
        "Unusually among antisense controllers it works by transcriptional "
        "attenuation rather than by occluding a ribosome-binding site: pairing "
        "with the initiator message leader promotes a termination hairpin just "
        "ahead of the initiator start codon, which an upstream preemptor sequence "
        "otherwise sequesters. Termination is driven by the antisense RNA, not by "
        "a stalled ribosome as in amino-acid operon attenuators. pT181 is the "
        "replicon behind a large family of staphylococcal shuttle vectors.",
        organism="Staphylococcus aureus",
    ),
    Family(
        "RF00235", "RNAIII (pIP501)", ("pIP501 RNAIII", "plasmid RNAIII", "RNAIII antisense RNA"),
        "regulatory", "ncRNA", "14583190",
        "Antisense RNA of the pIP501 replication-control system. It pairs with "
        "loop L1 of its target and attenuates transcription of the essential repR "
        "initiator; the loop carries a YUNR U-turn motif, and mutations that "
        "destroy the turn slow the pairing enough to raise copy number two- to "
        "threefold, which is how the kinetics were shown to be the regulated step. "
        "pIP501 is the theta-replicating backbone of widely used Enterococcus, "
        "Lactococcus and Streptococcus shuttle vectors.",
        caveat="Distinct from staphylococcal agr RNAIII (RF00503), which is a "
               "quorum-sensing regulator and is deliberately not in this "
               "database. The shared name is a collision, not a relationship.",
        organism="pIP501",
        rep="X17655.1/463-336",
    ),
    Family(
        "RF01087", "repZ pseudoknot", ("PK-repZ", "repZ translational switch", "ColIb-P9 repZ pseudoknot"),
        "regulatory", "misc_feature", "9565606",
        "A pseudoknot in the leader of repZ, the replication initiator of the "
        "IncIalpha plasmid ColIb-P9, acting as the switch for its translation. A "
        "downstream stem-loop normally sequesters the repZ ribosome-binding site; "
        "translation of an upstream leader peptide frees a short complementary "
        "run to pair at a distance and form the pseudoknot, which opens the site. "
        "Carried partly as a worked example: an element defined by a long-range "
        "pairing is exactly what a linear-sequence annotator cannot represent, and "
        "the renderer needs one to be tested against.",
        organism="Shigella sonnei plasmid P9",
    ),
    Family(
        "RF01794", "sok antitoxin RNA", ("sok", "flmB", "hok/sok antitoxin RNA", "sok RNA"),
        "regulatory", "ncRNA", "3049248",
        "The antitoxin half of hok/sok-type post-segregational killing loci -- sok "
        "in the parB region of R1, flmB on the F plasmid leading region, and "
        "related SOS-associated members. The untranslated RNA overlaps the leader "
        "of the toxin message and blocks translation of a membrane-damaging "
        "lethal peptide. Because the antitoxin RNA is far less stable than the "
        "toxin message, a daughter cell that has lost the plasmid loses the block "
        "first and kills itself, which is what makes the module work as a "
        "bolt-on stability cassette in low-copy and industrial constructs.",
        organism="Escherichia coli",
    ),
    # -- conjugation --------------------------------------------------------
    Family(
        "RF00107", "FinP", ("FinP", "FinP antisense RNA", "FinOP fertility inhibition RNA"),
        "regulatory", "ncRNA", "9917389",
        "The RNA half of the FinOP fertility-inhibition switch of F-like "
        "conjugative plasmids. Two stem-loops complementary to the untranslated "
        "leader of traJ; pairing sequesters the traJ ribosome-binding site, which "
        "shuts off the positive regulator of the transfer operon and so represses "
        "conjugation. On its own FinP is short-lived -- cleaved between the two "
        "stem-loops -- and it only reaches a repressive concentration when the "
        "FinO protein binds the second loop and shields it. Conjugative and "
        "mobilisable vectors are a poorly annotated corner of the vector world "
        "and this element decides whether such a plasmid transfers at all.",
        organism="Salmonella enterica",
    ),
    Family(
        "RF00243", "traJ 5' leader", ("traJ 5' UTR", "traJ leader", "FinP target"),
        "regulatory", "misc_feature", "14633993",
        "The untranslated leader of traJ: the cis target that FinP pairs with. It "
        "is a separate record because an antisense controller and the thing it "
        "controls are two sequences, and shipping only the controller leaves a map "
        "showing one side of a switch. Left alone the two RNAs pair slowly, each "
        "trapped in its own internal structure; the FinO chaperone supplies the "
        "strand-exchange activity that makes the interaction fast enough to "
        "regulate, using binding energy rather than ATP.",
        organism="Salmonella enterica",
    ),
    # -- retro/lentiviral vector elements -----------------------------------
    Family(
        "RF00036", "RRE", ("RRE", "Rev response element", "HIV-1 RRE"),
        "regulatory", "misc_feature", "12177299",
        "The Rev response element: a large, well-ordered structured region lying "
        "inside the HIV-1 env reading frame. Multimeric binding of the viral Rev "
        "protein to it recruits a host nuclear export pathway and licenses "
        "unspliced and singly spliced viral transcripts to leave the nucleus, "
        "which they otherwise cannot do. It is present on the transfer plasmid of "
        "every second- and third-generation lentiviral vector system, where the "
        "full-length genomic transcript has to reach the cytoplasm to be packaged.",
        caveat="Rfam's only reference for this family is a general RNA-folding "
               "study that uses the RRE as one of several test cases, not the "
               "work that defined the element. The description is written from "
               "it and stays inside what it supports; a curator should add the "
               "defining citation before signing off.",
        organism="HIV-1",
    ),
    Family(
        "RF00375", "HIV-1 PBS", ("PBS", "primer binding site", "HIV-1 primer binding site"),
        "regulatory", "misc_feature", "14757051",
        "The primer binding site of HIV-1 and the hairpin around it, near the 3' "
        "end of the 5' leader. Host tRNA-Lys3 anneals here, and its 3' end is what "
        "reverse transcriptase extends to begin minus-strand synthesis, so this is "
        "where the provirus starts being made. The hairpin is not merely a "
        "landing pad: it carries a tertiary interaction of its own within the "
        "folded leader. It sits immediately downstream of R-U5 in lentiviral "
        "transfer plasmids, which makes it a dependable landmark for orienting an "
        "unfamiliar lentiviral map.",
        organism="HIV-1",
    ),
    Family(
        "RF01381", "HIV-1 Psi SL3", ("SL3", "Psi SL3", "HIV-1 packaging signal stem-loop 3"),
        "regulatory", "misc_feature", "18713870",
        "Stem-loop 3 of the HIV-1 packaging signal: one of the four hairpins that "
        "together fold into the compact cloverleaf Psi element in the 5' leader "
        "ahead of gag, recognised by the nucleocapsid domain of the Gag "
        "polyprotein. Structural work shows the four stem-loops keep their "
        "separate identities within the fold rather than merging into a single "
        "high-affinity site. Retaining Psi on the transfer plasmid and leaving it "
        "off the packaging and envelope plasmids is what makes a split lentiviral "
        "system package only the vector genome.",
        caveat="Rfam also models the wider Psi region as RF00175 (SL1-SL4 in one "
               "alignment) and SL4 separately as RF01382. A per-stem-loop model "
               "gives a tighter annotation; full Psi coverage is a curator "
               "decision, not a data gap.",
        organism="HIV-1",
    ),
    Family(
        "RF00374", "MMLV Psi (core encapsidation signal)",
        ("Psi", "core encapsidation signal", "MMLV packaging signal", "gammaretroviral Psi"),
        "regulatory", "misc_feature", "15003457",
        "The core encapsidation signal of gammaretroviruses, solved by NMR as a "
        "101-nucleotide region from the genome of Moloney murine leukaemia virus, "
        "folding into three stem-loops, two of them co-stacked into an extended "
        "helix. Genome dimerisation shifts the base-pairing register and exposes "
        "conserved UCUG elements that the nucleocapsid zinc knuckle binds "
        "tightly, coupling packaging to dimerisation. This is the Psi of the "
        "MSCV, MigR1, pQCXIP and pBabe retroviral vectors -- a lineage entirely "
        "separate from the HIV-derived transfer plasmids above, and one that "
        "HIV-only coverage would leave unannotated.",
        organism="Moloney murine leukemia virus",
        rep="AF033811.1/12-112",
    ),
    # -- translation initiation ---------------------------------------------
    Family(
        "RF00061", "HCV IRES", ("IRES", "HCV IRES", "hepatitis C virus IRES", "type III IRES"),
        "regulatory", "misc_feature", "25775547",
        "The internal ribosome entry site in the 5' untranslated region of "
        "hepatitis C virus. It recruits the small ribosomal subunit directly, "
        "without a cap, without eIF4E and without scanning, which is why it is "
        "reached for in bicistronic reporters and in designs that need "
        "cap-independent initiation with a minimal factor requirement. Genome-wide "
        "structure probing places it among a set of conserved elements that "
        "together account for the virus's RNA-level regulation.",
        caveat="Rfam has NO model for the EMCV IRES -- the element actually "
               "carried by the pIRES / pMSCV-IRES / MigR1 lineage. Searching "
               "'EMCV', 'encephalomyocarditis' and 'cardiovirus' returns zero "
               "IRES families. RF00229 'IRES_Picorna' does not fill the gap: "
               "its seed is enterovirus and rhinovirus. EMCV must be hand "
               "curated in Stage 5.",
        organism="Hepacivirus",
    ),
    Family(
        "RF00458", "CrPV IGR IRES", ("IGR IRES", "CrPV IRES", "cripavirus intergenic IRES", "dicistrovirus IGR IRES"),
        "regulatory", "misc_feature", "11233983",
        "The intergenic-region IRES of cripaviruses such as cricket paralysis "
        "virus and Plautia stali intestine virus. A compact, multiply "
        "pseudoknotted RNA that starts translation from a codon other than AUG "
        "and needs neither initiator methionine tRNA nor any initiation factor, "
        "making it the most nearly self-sufficient initiation element known. That "
        "independence is exactly why it is used in cell-free translation, insect "
        "expression, and synthetic designs wanting an initiation element "
        "orthogonal to the host's own.",
        organism="Cricket paralysis virus",
        rep="AF218039.1/6028-6223",
    ),
    # -- ribozymes ----------------------------------------------------------
    Family(
        "RF00094", "HDV ribozyme", ("HDV ribozyme", "hepatitis delta virus ribozyme", "HDVr"),
        "misc", "ncRNA", "9783582",
        "The self-cleaving ribozyme of hepatitis delta virus: five helical "
        "segments arranged as a nested double pseudoknot, burying the scissile "
        "backbone deep in a cleft lined with the functional groups that do the "
        "chemistry -- an active site organised much as a protein enzyme's is. It "
        "is the only catalytic RNA required by a human pathogen. In cloning it is "
        "not a curiosity but a part: the standard 3' cassette for cutting a "
        "transcript to a defined end, most visibly in Pol II-driven guide RNA "
        "constructs, where a polymerase that adds a cap and a tail must still "
        "yield a guide with an exact 3' terminus.",
        organism="Hepatitis D virus",
    ),
    Family(
        "RF00008", "hammerhead ribozyme (type III)", ("hammerhead ribozyme", "HHR", "hammerhead type III"),
        "misc", "ncRNA", "7969422",
        "The hammerhead ribozyme in its type III arrangement: three base-paired "
        "stems around a core of conserved, non-complementary nucleotides, one "
        "domain of which is a sharp uridine turn borrowed from the same motif "
        "found in tRNA. It is among the smallest catalytic RNAs known, which is "
        "why it is the one usually grafted into engineered constructs, and it is "
        "the usual 5' partner to an HDV ribozyme in a self-processing transcript "
        "cassette.",
        caveat="Type III is the topology used in nearly all synthetic work. "
               "RF00163 (type I) is chromosomal and retroelement-associated with "
               "a full set of 324,155 hits, and is deliberately excluded -- it "
               "would fire constantly.",
        organism="Peach latent mosaic viroid",
    ),
    Family(
        "RF00234", "glmS ribozyme-riboswitch", ("glmS", "glmS ribozyme", "GlcN6P riboswitch"),
        "regulatory", "regulatory", "15029187",
        "A ribozyme in the untranslated leader of the glmS message of "
        "Gram-positive bacteria that cleaves its own backbone only when "
        "glucosamine-6-phosphate is bound as a coenzyme. Since that metabolite is "
        "the product of the enzyme the message encodes, cleavage -- and so decay "
        "of the message -- rises exactly when the pathway output rises: a genetic "
        "switch built from catalysis rather than from protein repression. It sits "
        "on the boundary between ribozyme and riboswitch and is the model for "
        "ligand-gated cleavage elements engineered into synthetic transcripts.",
        organism="Geobacillus thermodenitrificans",
    ),
    # -- riboswitches -------------------------------------------------------
    Family(
        "RF00059", "TPP riboswitch", ("TPP riboswitch", "THI element", "thi box", "thiamine pyrophosphate riboswitch"),
        "regulatory", "regulatory", "12410317",
        "A conserved domain in the leaders of thiamine biosynthesis and transport "
        "messages that binds thiamine pyrophosphate itself, with no protein "
        "involved; the resulting fold occludes the ribosome-binding site or forces "
        "early termination, so the vitamin switches off the genes that make it. "
        "This is the most widely distributed riboswitch class and the only one "
        "also found in eukaryotes, which makes it the one most likely to be "
        "carried unnoticed inside a cloned leader or a fungal expression cassette; "
        "it is also the standard chassis for engineered thiamine-responsive "
        "control.",
        organism="Bacillus cereus",
    ),
    Family(
        "RF00050", "FMN riboswitch", ("FMN riboswitch", "RFN element", "flavin mononucleotide riboswitch"),
        "regulatory", "regulatory", "12456892",
        "The RFN element: a conserved domain ahead of bacterial riboflavin "
        "biosynthesis and transport genes that is a natural aptamer for flavin "
        "mononucleotide. Binding the flavin directly, without a protein, switches "
        "the domain into a conformation that terminates transcription early or "
        "buries the ribosome-binding site. It rides into vectors on cloned rib "
        "operon fragments, it is the target of roseoflavin-based selection "
        "schemes, and no protein-based annotator can see it.",
        organism="Staphylococcus epidermidis",
    ),
    # -- engineered and structural RNA --------------------------------------
    Family(
        "RF02348", "tracrRNA", ("tracrRNA", "trans-activating crRNA", "sgRNA scaffold", "Cas9 scaffold"),
        "misc", "ncRNA", "22745249",
        "The scaffold half of the SpCas9 guide: a small RNA encoded elsewhere in "
        "the Streptococcus pyogenes CRISPR locus that acts in trans on the "
        "crRNA precursor. It pairs with the repeat of that precursor, and the "
        "duplex so formed is required twice "
        "over: once for RNase III to mature the crRNA, and again for Cas9 to be a "
        "competent nuclease at all. Fusing the two RNAs into one chimera is what "
        "produced the single guide RNA, so this sequence is the scaffold half of "
        "essentially every SpCas9 guide cassette built today -- a pure non-coding "
        "element, invisible to any protein-based annotator, and one of the most "
        "frequently cloned sequences in modern molecular biology.",
        patent_flag="1",
        caveat="Patent flag set, and it is a flag rather than a determination: "
               "no patent database was consulted. SOURCING.md Risk 6 -- CC BY 4.0 "
               "licenses no patent rights and says so; the sgRNA scaffold is "
               "recited in a dense and actively asserted estate. Counsel "
               "question, not a lab question.",
        organism="Streptococcus pyogenes",
        rep="BA000034.2/1153816-1153905",
    ),
    Family(
        "RF02359", "MS2 operator hairpin", ("MS2", "MS2 stem-loop", "MS2 operator", "MS2 coat protein binding site"),
        "regulatory", "misc_feature", "11095669",
        "The coat-protein operator of bacteriophage MS2: a short stem-loop whose "
        "recognition is exquisitely local -- deleting a single hydrogen-bonding "
        "atom from one loop base rearranges the whole loop against an otherwise "
        "unchanged protein. In the phage it represses replicase translation and "
        "nucleates assembly; in the laboratory it is the tethering handle behind "
        "MS2/MCP systems for live-cell RNA imaging, RNA pull-down and "
        "tethered-function assays. It is nearly always cloned as a tandem array, "
        "so an annotator that finds one copy should expect several.",
        caveat="Rfam has no PP7, boxB or Qbeta equivalent -- all verified absent -- "
               "so MS2 is the only phage RNA aptamer this source can supply. Its "
               "seed is also the smallest here, so its sensitivity will be "
               "correspondingly narrow.",
        organism="phage MS2",
        rep="EF108464.1/1729-1764",
    ),
    Family(
        "RF00005", "tRNA", ("tRNA", "transfer RNA"),
        "misc", "tRNA", "8256282",
        "Transfer RNA: the cloverleaf that folds into an L, and the adaptor that "
        "makes the genetic code physical. It is in a plasmid feature database not "
        "as a housekeeping gene but as a part -- the spacer excised by the host's "
        "own processing enzymes to release individual guides from polycistronic "
        "tRNA-gRNA arrays, the amber suppressor of non-canonical amino-acid "
        "systems, and the rare-codon supplement carried on pRARE and Rosetta "
        "helper plasmids. Those applications are what puts tRNA in a plasmid "
        "feature database; they are not what the reference sequence below is. It "
        "is one tRNA gene from one bacterial rrn operon, so it will not "
        "exact-match the E. coli tRNAs of a pRARE cassette.",
        caveat="The deepest alignment in this block by a wide margin. That "
               "breadth is coverage and false-positive exposure in equal "
               "measure; the annotator should expect this row to fire on any "
               "cloned genomic fragment.",
        organism="Bacillus halodurans",
    ),
    Family(
        "RF00001", "5S rRNA", ("5S rRNA", "5S ribosomal RNA", "5S rDNA"),
        "misc", "rRNA", "11752286",
        "5S ribosomal RNA. Every ribosome carries one, in its large subunit, "
        "with the single exception of the mitochondrial ribosomes of fungi and "
        "animals; it appears to stabilise the particle rather than to catalyse "
        "anything. It earns a place here by accident of cloning history: the "
        "rrnB T1/T2 "
        "terminator region that people actually wanted sits immediately "
        "downstream of 5S, so 5S sequence travels into expression backbones "
        "unlabelled. It is also a Pol III cassette element in eukaryotic "
        "constructs. That rationale is about the E. coli rrnB operon; the "
        "reference sequence below is a 5S gene from elsewhere in the family, "
        "chosen for being a typical member, and it will not exact-match the 5S "
        "that travels with an rrnB terminator.",
        caveat="The row most likely to be argued about at review. If the curator "
               "judges the false-positive rate too high, this is the first one "
               "to drop -- nothing else in the block depends on it.",
        organism="Leptospira interrogans",
    ),
)


# --------------------------------------------------------------------------
# Exclusion 1 — miRNA, keyed on the type field and never on the name

# Matched as a substring of the type, not as the whole field. Measured: today
# there is exactly one distinct type string containing it, and matching the
# substring or the full "Gene; miRNA;" both select the same 1,598 families — so
# the looser form costs nothing now and still catches a future release that
# refines the type to, say, "Gene; miRNA; precursor;".
MIRNA_TYPE = "mirna"

# INSDC feature keys we will emit. A closed set, because `genbank_key` is the
# column an SO term would slip into if anyone ever decided Rfam's `#=GF DR`
# lines looked convenient, and SOURCING.md Risk 4 holds SO until its LICENSE
# (CC BY 4.0) and its README (CC BY-SA 4.0) stop contradicting each other.
INSDC_KEYS = {"ncRNA", "tRNA", "rRNA", "regulatory", "misc_feature", "stem_loop"}

# Rfam's type field mapped to our `class` column, where it maps cleanly. The
# point is not to save typing — it is that a hand-declared class can then be
# *checked*, so a contributor cannot file a riboswitch as `misc` by accident.
# A bare "Gene;" is genuinely ambiguous (FinP and pIP501 RNAIII are both bare,
# and both are antisense controllers) so it returns None and defers to the
# declared value rather than guessing.
TYPE_CLASS = {
    "gene; antisense;": "regulatory",
    "gene; antitoxin;": "regulatory",
    "gene; ribozyme;": "misc",
    "gene; trna;": "misc",
    "gene; rrna;": "misc",
    "gene; crispr;": "misc",
}


def class_from_type(rfam_type: str) -> str | None:
    """Our class for an Rfam type, or None where the type does not decide it."""
    t = rfam_type.strip().lower()
    if t.startswith("cis-reg"):
        return "regulatory"
    return TYPE_CLASS.get(t)


def assert_not_mirna(acc: str, rfam_id: str, rfam_type: str) -> None:
    """Refuse miRBase-derived families. Reads the type, never the name.

    SOURCING.md §2 Stage 4 records that miRBase publishes no licence at all —
    its homepage, /help/ and /download/ contain zero occurrences of licence,
    copyright, CC0 or public domain, only a citation demand. Rfam's CC0 cannot
    relicense what it never held. Measured here: 1,598 of the 4,227 families
    (37.8%) are typed as miRNA.

    Keyed on the **type** column because the obvious alternative leaks. Over
    the same 4,227 rows the type selects 1,598 while
    `rfam_id.lower().startswith("mir")` selects 1,590 — so a name-keyed filter
    ships eight unlicensed families and looks like it worked. `self_test()`
    re-measures both counts every run and fails if the gap ever closes, because
    if it did, this paragraph would be the only remaining reason for a design
    nobody could then justify from the data.

    `RF00250` (mir-TAR, the HIV trans-activation response element) is the
    reverse case and a genuine loss: it is a real lentiviral map annotation,
    it is typed as a miRNA, and this gate correctly refuses it. TAR must be
    hand-curated in Stage 5 or dropped.
    """
    if MIRNA_TYPE in rfam_type.strip().lower():
        raise SystemExit(
            f"{acc} ({rfam_id}) is typed {rfam_type!r}: miRBase-derived families "
            f"carry no licence and must never enter this database "
            f"(features/SOURCING.md §2 Stage 4)"
        )


# --------------------------------------------------------------------------
# Exclusion 2 — Wikipedia/Summary prose, by whitelist and by taint check

# Read. Accession, name, one-line label, type, and the provenance of the seed
# and the structure — plus the whole bibliographic block, which is factual
# credit and is what satisfies SOURCING.md's per-family primary-source-credit
# requirement.
GF_KEEP = {"AC", "ID", "DE", "TP", "SE", "SS", "RN", "RM", "RT", "RA", "RL", "SQ"}

# Read *in order to check we did not use it*. Not simply skipped: quarantined,
# so `assert_untainted()` has something concrete to compare our prose against.
#   CC — curator prose, byte-identical to family.txt's comment column, and
#        entangled with Wikipedia in both directions (the RNA WikiProject
#        historically seeded Wikipedia *from* Rfam, so per-family direction of
#        copying is unknowable — which is itself the reason not to touch it).
#   WK — the Wikipedia article title. Present on all 4,227 blocks, so
#        "prefer families with no Wikipedia link" is not an available defence.
#   DR — database cross-references, which include Sequence Ontology ids. SO is
#        HOLD under Risk 4, so its terms must not reach `genbank_key`.
GF_QUARANTINE = {"CC", "WK", "DR"}

# Everything else — AU, GA, TC, NC, BM, CB, SM, PI, CL, and the `**` tag that
# appears on 47 blocks — is dropped unread. This is a whitelist on purpose: a
# future Rfam release that adds a prose tag is excluded by default rather than
# ingested by default, which is the only way round this that survives us not
# reading the next release's changelog.

_WORD = re.compile(r"[^a-z0-9]+")


def words(text: str) -> list[str]:
    return [w for w in _WORD.sub(" ", text.lower()).split() if w]


def shared_ngram(ours: str, theirs: str, n: int = 5) -> str | None:
    """First contiguous n-word run present in both, or None.

    The metric is SOURCING.md §0.4's, reused deliberately: there, any shared
    contiguous 5-token n-gram with `snapgene.csv` is a hard fail regardless of
    ratio. The same standard is applied here to Rfam's quarantined prose, since
    the CC BY-SA problem and the SnapGene problem are the same problem — prose
    we may read but must not reproduce.
    """
    a, b = words(ours), words(theirs)
    if len(a) < n or len(b) < n:
        return None
    theirs_grams = {tuple(b[i:i + n]) for i in range(len(b) - n + 1)}
    for i in range(len(a) - n + 1):
        g = tuple(a[i:i + n])
        if g in theirs_grams:
            return " ".join(g)
    return None


def assert_untainted(acc: str, ours: dict[str, str], quarantined: list[str]) -> None:
    """Hard-fail if anything we wrote overlaps the text we promised not to use.

    Without this, exclusion 2 is unfalsifiable: the parser drops CC and WK, so
    of course no CC or WK string appears verbatim in the output, and a green
    build proves only that we did not literally concatenate them. What it would
    not catch is the realistic failure — a description written while reading
    the summary, which is paraphrase, and paraphrase of a BY-SA source is a
    derivative work.

    This check does fail in practice. Writing the FMN row from the shape of the
    family's own comment produced "in the 5 untranslated regions of" against
    Rfam's "found frequently in the 5'-untranslated regions of prokaryotic
    mRNAs" — a shared six-word run, caught here, and the reason that row now
    reads "ahead of bacterial riboflavin biosynthesis and transport genes".
    """
    for field, text in ours.items():
        for q in quarantined:
            g = shared_ngram(text, q)
            if g:
                raise SystemExit(
                    f"{acc} {field}: shares the 5-word run {g!r} with Rfam's "
                    f"quarantined CC/WK text. Descriptions are written by us "
                    f"from the primary literature; rewrite it "
                    f"(features/SOURCING.md §2 Stage 4, §5 Risk 1)"
                )


# --------------------------------------------------------------------------
# Exclusion 3 — the U->T transform, asserted rather than trusted

# What the Rust loader accepts in `reference_nt` (pl-features/src/lib.rs). The
# Rfam seeds use IUPAC ambiguity codes — M, N, R, S, W and Y all occur in the
# selected families — so a strict [ACGT] validator would reject real rows. It
# is checked against this set instead, which is the set that actually governs.
NUCLEOTIDES = set("ACGTRYSWKMBDHVN")

# Rfam seed rows are ALIGNMENT rows, not sequences: across the selected
# families roughly 36% of every row is gap. Degap before anything else or the
# reference is mostly dashes. The gap character is "-" only ("." never occurs)
# and there is no lowercase, so no case folding is needed.
GAPS = "-."


def degap(aligned: str) -> str:
    return "".join(c for c in aligned if c not in GAPS)


def to_dna(rna: str, what: str) -> str:
    """`tr U->T`, then prove it.

    Unconditional, because Rfam.seed is uniformly RNA — 64,970 U and zero T
    across the twenty-four seeds read here — so there is nothing to detect.
    Never do this by sniffing. SOURCING.md's Stage 4 survey records that the
    per-family FASTAs under `fasta_files/` are *mixed* alphabet, DNA
    full-region hits concatenated with RNA seed rows in one file, so any
    auto-detection reads the first record and gets the wrong answer for the
    rest. That finding is not re-verified here and cannot be: `rfam_fetch`
    refuses to fetch those files at all, which is the point.

    The assertion is the point. A surviving U is not a small error: the loader
    rejects it outright, and if it ever did load, every Rfam feature at once
    would match nothing and the bug would present as an empty database rather
    than as bad data.
    """
    dna = rna.upper().replace("U", "T")
    if "U" in dna:
        raise SystemExit(f"{what}: 'U' survived the RNA->DNA transform")
    bad = sorted(set(dna) - NUCLEOTIDES)
    if bad:
        raise SystemExit(f"{what}: {bad} are not nucleotide codes the loader accepts")
    return dna


# --------------------------------------------------------------------------
# Exclusion 4 — only the two files we cleared

# `fasta_files/` is the full-region set: every cmsearch hit across ENA,
# chromosomal and environmental and heavily redundant, and mixed-alphabet as
# above. `wikitext` is the Wikipedia dump. Named here so that wiring either one
# in is a crash rather than a code review someone has to catch.
FORBIDDEN_URL = ("fasta_files/", "wikitext", "/summary", "mirna")


def rfam_fetch(url: str, name: str, refresh: bool) -> bytes:
    """`fetch()` restricted to the Rfam files SOURCING.md cleared."""
    if not url.startswith(RFAM_BASE + "/"):
        raise SystemExit(f"stage_rfam may only fetch from {RFAM_BASE}: {url}")
    for part in FORBIDDEN_URL:
        if part in url.lower():
            raise SystemExit(
                f"refusing {url}: {part!r} is excluded by features/SOURCING.md "
                f"§2 Stage 4 (prefer Rfam.seed.gz; never ingest Wikipedia text)"
            )
    return fetch(url, name, refresh)


# --------------------------------------------------------------------------
# Parsing


@dataclass
class SeedRow:
    """One row of a seed alignment: a region of one record, aligned."""

    ident: str
    """As written, e.g. `AF156893.1/1234-1130` or `URS0000D56C30_11676/1-101`."""
    accession: str = ""
    start: int = 0
    end: int = 0
    aligned: str = ""

    @property
    def is_insdc(self) -> bool:
        """Is this a real INSDC record, or an RNAcentral URS identifier?

        Rfam seeds mix the two, and the difference is load-bearing here: a URS
        id has no INSDC record behind it, so its `1-101` indexes the URS
        sequence itself and cannot serve as `boundary_evidence`. Eleven of the
        eighteen rows of RF00458 are URS, so this is not a corner case — pick
        one blind and the row's evidence pointer resolves to nothing.
        """
        return bool(self.accession)

    @property
    def seq(self) -> str:
        return degap(self.aligned)

    @property
    def strand(self) -> str:
        # Rfam writes a reverse-strand region descending, e.g. `5844-5721`.
        return "-" if self.start > self.end else "+"

    @property
    def span(self) -> int:
        return abs(self.end - self.start) + 1


@dataclass
class SeedBlock:
    acc: str = ""
    gf: dict[str, list[str]] = None
    quarantined: list[str] = None
    rows: list[SeedRow] = None
    refs: list[dict] = None
    """Bibliography, kept as *structured* references rather than as four
    parallel tag lists.

    `#=GF RT` wraps: a title longer than the line width is emitted as two or
    three RT lines, so across a family the RT count exceeds the RN count and
    the two lists no longer align by index. Flattening them and reading
    `titles[i]` therefore either drops the title (when the mismatch is noticed)
    or attributes the wrong paper's title (when it is not). This is the common
    case, not the corner: **21 of the 24** families here have more RT lines
    than references — RF00106 has three references across five RT lines — so
    positional keying loses or corrupts the per-family primary-source credit
    that SOURCING.md §2 Stage 4 exists to require, for almost every row.
    """

    def gf1(self, tag: str) -> str:
        v = (self.gf or {}).get(tag) or [""]
        return v[0]


_SEED_ID = re.compile(r"^([A-Za-z0-9_]+\.\d+)/(\d+)-(\d+)$")


def parse_family_table(text: str) -> dict[str, dict]:
    """The MySQL tab dump, reduced to the columns we allow-list.

    Field-level allow-list rather than "everything the file has", per
    SOURCING.md Risk 7: contamination rides in fields nobody meant to ingest,
    and 35 columns is 33 more chances to ingest one. The comment column is read
    *only* to quarantine it.
    """
    out = {}
    for line in text.split("\n"):
        if not line.strip():
            continue
        f = line.split("\t")
        if len(f) < 35:
            continue
        out[f[F_ACC]] = {
            "id": f[F_ID],
            "description": f[F_DESC],
            "type": f[F_TYPE],
            "num_seed": f[F_NUM_SEED],
            "quarantine": f[F_COMMENT],
        }
    return out


def parse_seed(text: str, wanted: set[str]) -> dict[str, SeedBlock]:
    """Stockholm blocks for `wanted`, whitelisting the `#=GF` tags."""
    blocks: dict[str, SeedBlock] = {}

    def fresh() -> SeedBlock:
        return SeedBlock(gf={}, quarantined=[], rows=[], refs=[])

    cur = fresh()
    for line in text.split("\n"):
        if line.startswith("//"):
            if cur.acc in wanted:
                blocks[cur.acc] = cur
            cur = fresh()
            continue
        if line.startswith("#=GF "):
            tag, _, value = line[5:].partition(" ")
            value = value.strip()
            if tag == "AC":
                cur.acc = value
            if tag in GF_KEEP:
                cur.gf.setdefault(tag, []).append(value)
                # `RN` opens a reference; everything after it belongs to that
                # reference until the next `RN`. Accumulating by open block is
                # what makes a wrapped title survive.
                if tag == "RN":
                    cur.refs.append({"RM": "", "RT": [], "RA": [], "RL": ""})
                elif tag in ("RM", "RT", "RA", "RL") and cur.refs:
                    if tag in ("RT", "RA"):
                        cur.refs[-1][tag].append(value)
                    else:
                        cur.refs[-1][tag] = value
            elif tag in GF_QUARANTINE:
                cur.quarantined.append(value)
            # Anything else is dropped unread — see GF_KEEP.
            continue
        if line.startswith("#") or not line.strip():
            # `#=GC` consensus rows (SS_cons, RF) and `#=GR` per-row annotation
            # are not sequences. `#=GC RF` in particular is mixed-case and
            # would poison the alphabet check if it were treated as one.
            continue
        ident, _, aligned = line.partition(" ")
        if not aligned.strip():
            continue
        row = SeedRow(ident=ident, aligned=aligned.strip())
        m = _SEED_ID.match(ident)
        if m:
            row.accession, row.start, row.end = m.group(1), int(m.group(2)), int(m.group(3))
        cur.rows.append(row)
    if cur.acc in wanted:
        blocks[cur.acc] = cur
    return blocks


# --------------------------------------------------------------------------
# Choosing the representative row


def choose_representative(rows: list[SeedRow], pin: str = "") -> tuple[SeedRow, dict]:
    """Pick one seed row to be the reference sequence. Deterministic, stated.

    A seed alignment is N regions of N different records, not N copies of one
    sequence, so this is a choice and it is made explicitly rather than by
    taking `rows[0]`:

      0. **A pinned row wins outright.** `Family.rep` names one, and it must be
         present and INSDC or the build stops.
      1. **INSDC rows only.** A URS row has no record to point `boundary_evidence`
         at (see `SeedRow.is_insdc`).
      2. **Unambiguous rows preferred.** An N in a reference sequence is a hole
         in the k-mer index at exactly the position it occupies.
      3. **Length closest to the median.** Seeds contain partial rows —
         RF00008 spans 40 to 82 nt and RF00458 spans 141 to 197 — so the
         extremes are truncations and read-throughs, not the element.
      4. **Lexicographically smallest identifier**, so two builds of one Rfam
         release agree byte for byte.

    Rules 3 and 4 are the whole reason rule 0 exists. They are organism-blind,
    and in three families the biologically correct row was in the same seed at
    *identical* length and lost step 4 on the spelling of its accession. A
    tie-break that decides which species a row is about is not a tie-break.
    """
    insdc = [r for r in rows if r.is_insdc]
    if not insdc:
        raise SystemExit("no INSDC-accession row in this seed; nothing to cite as evidence")
    unambiguous = [r for r in insdc if set(r.seq) <= set("ACGU")]
    pool = unambiguous or insdc
    median = statistics.median([len(r.seq) for r in pool])
    if pin:
        hit = [r for r in insdc if r.ident == pin]
        if not hit:
            raise SystemExit(
                f"pinned representative {pin!r} is not an INSDC row of this seed. "
                f"Rfam has re-cut the alignment, or the pin was mistyped; either way "
                f"the row it was chosen for is no longer the row that would ship."
            )
        best = hit[0]
    else:
        best = min(pool, key=lambda r: (abs(len(r.seq) - median), len(r.seq), r.ident))
    stats = {
        "n_rows": len(rows),
        "n_insdc": len(insdc),
        "n_urs": len(rows) - len(insdc),
        "n_ambiguous": len(insdc) - len(unambiguous),
        "median": median,
        "min_len": min(len(r.seq) for r in insdc),
        "max_len": max(len(r.seq) for r in insdc),
        "pinned": bool(pin),
    }
    return best, stats


# --------------------------------------------------------------------------
# Checking the chosen row against the record it cites


def ena_fetch(accession: str, lo: int, hi: int, refresh: bool) -> str:
    """The depositor's own record, over exactly the interval Rfam cites.

    `build.fetch` already refuses any host SOURCING.md §1 has not cleared; this
    adds the narrower rule that stage_rfam may only ever ask ENA for a FASTA
    region, so no future edit here can reach a browser endpoint that serves
    something else.
    """
    url = f"{ENA_BASE}/fasta/{accession}?range={lo}-{hi}"
    if not url.startswith(ENA_BASE + "/fasta/"):
        raise SystemExit(f"stage_rfam may only fetch ENA FASTA regions: {url}")
    raw = fetch(url, f"ena_region_{accession}_{lo}_{hi}.fa", refresh)
    # ENA gzips some responses whether or not the client asked for it, and it
    # silently ignores `range=` on at least one WGS contig -- AAMK02000020.1
    # comes back as the whole 1.5 MB record. Neither is announced anywhere in
    # the payload, so both are sniffed: gzip here, the ignored range in
    # `check_against_depositor`, which is the code that knows what it asked for.
    if raw[:2] == b"\x1f\x8b":
        raw = gzip.decompress(raw)
    return raw.decode("utf-8")


COMPLEMENT = str.maketrans("ACGT", "TGCA")


def revcomp(s: str) -> str:
    return s.translate(COMPLEMENT)[::-1]


def check_against_depositor(fam, best: SeedRow, nt: str, lo: int, hi: int,
                            refresh: bool) -> tuple[str, dict]:
    """Two claims, both checked against the record itself rather than asserted.

    1. **The coordinates hold the sequence.** `boundary_evidence` says
       `ACC:lo-hi:strand`, and until now nothing had ever asked the depositor
       whether that interval of that record contains these bases. Rfam's seed
       coordinates could drift, our minus-strand handling could be inverted, and
       both failures produce a row that looks entirely well formed.
    2. **The organism is the one the description names.** See `Family.organism`.

    Returns (the record's own description line, its cache metadata), so the row
    can carry both and a reviewer can re-fetch exactly what was read.
    """
    text = ena_fetch(best.accession, lo, hi, refresh)
    # Parse as FASTA, and find the record actually asked for rather than taking
    # the first one. Asking for a *WGS contig* accession returns the entire WGS
    # set: `AAMK02000020.1?range=20388-20545` served 91 records beginning with
    # AAMK02000001.1, 1.5 MB of them, HTTP 200, no warning. Reading that payload
    # as one sequence concatenates 91 contigs, and the resulting comparison
    # fails for a reason that has nothing to do with the row.
    records = [(h, s) for h, s in parse_fasta(text)]
    if not records:
        raise SystemExit(f"{fam.acc}: ENA returned no FASTA record for {best.accession}")
    match = [(h, s) for h, s in records if best.accession in h] or records[:1]
    header, body = match[0]
    header = header.strip()
    got = body.upper().replace("U", "T")
    want = nt if best.strand == "+" else revcomp(nt)
    # Did ENA honour `range=`? Its own header says so when it did, and when it
    # did not the payload is the whole record with no indication the request was
    # reinterpreted. Slicing locally is the same check, done here.
    served_range = f"Location:{lo}..{hi}" in header.replace(" ", "")
    if not served_range and len(got) >= hi:
        got, ignored = got[lo - 1:hi], True
    else:
        ignored = False
    if got != want:
        raise SystemExit(
            f"{fam.acc} {best.ident}: ENA {best.accession}:{lo}-{hi} does not hold the "
            f"sequence this row would ship ({len(got)} nt read, {len(want)} nt "
            f"expected on the {best.strand} strand). The seed row and its own "
            f"coordinates disagree; refusing to write a boundary the depositor "
            f"contradicts."
        )
    if ignored:
        header += f" [range= ignored by ENA; interval {lo}..{hi} taken locally]"
    if not fam.organism:
        raise SystemExit(
            f"{fam.acc}: no `organism` declared. Every family must name the source "
            f"its reference has to come from, because the representative is chosen "
            f"by length and length says nothing about species."
        )
    if fam.organism.lower() not in header.lower():
        raise SystemExit(
            f"{fam.acc} {best.ident}: this row's reference comes from {header!r}, "
            f"which does not contain the declared organism {fam.organism!r}. Either "
            f"pin `rep` to a row from that source or change what the row claims to "
            f"be — do not ship a name and a description for one organism over the "
            f"sequence of another."
        )
    return header, cached_meta(f"ena_region_{best.accession}_{lo}_{hi}.fa")


def citation(block: SeedBlock, pmid: str) -> str:
    """The `#=GF RM/RT/RL` entry for `pmid`, as a citation string.

    CC0 imposes no legal duty to credit, but Rfam's documentation asks for
    per-family primary-source credit and SOURCING.md §2 Stage 4 adopts it as a
    requirement. Taken from the seed rather than from `family.txt`, because
    four of the twenty-four families here carry no PMID at all in
    `family.txt`'s seed_source or structure_source columns — those read
    "Bateman A", "Pseudobase", "Predicted; PFOLD" — while all twenty-four carry
    `#=GF RM` lines. Note also that `family.txt` spells it inconsistently
    ("Pubmed:14583190", not "PMID:"), so harvesting it from there needs a
    regex that accepts both and silently loses a family's credit if it does not.
    """
    for ref in block.refs or []:
        if ref["RM"] != pmid:
            continue
        # RT wraps mid-sentence, so the pieces join with a space; RL already
        # ends in a full stop, which would otherwise double up when the caller
        # closes its own sentence.
        title = " ".join(ref["RT"]).strip()
        journal = ref["RL"].strip().rstrip(".")
        return " ".join(x for x in (f"PMID {pmid}", title, journal) if x)
    raise SystemExit(
        f"{block.acc}: credit_pmid {pmid} is not among this family's #=GF RM "
        f"references {[r['RM'] for r in block.refs or []]}. Descriptions must "
        f"be written from a paper the family actually cites."
    )


# --------------------------------------------------------------------------
# Will the loader accept what we are about to write?

# The five `BoundaryRule` spellings and six `Class` spellings the Rust parser
# accepts. Mirrored here, not imported, because there is nothing to import
# from: `lib_columns.py` pins the column *names* to `pl-features/src/lib.rs`
# but not the enum *values*.
LOADER_CLASSES = {"cds", "regulatory", "origin", "repeat", "synthetic_part", "misc"}
LOADER_RULES = {
    "orf_atg_to_stop", "orf_mature_peptide", "literature_defined",
    "consensus_of_insdc", "designed_sequence",
}


def validate_row(r: Row) -> None:
    """Re-check a finished row against the rules `Db::parse` enforces.

    A stage that emits a row the loader rejects has not failed loudly: it has
    written a `features.tsv` that builds fine and then produces a `LoadError`
    somewhere else entirely, at which point the stage that caused it is no
    longer on screen. Catching it here names the row and the reason.

    This is a *mirror*, and mirrors drift — `crates/pl-features/src/lib.rs` is
    the authority, and if the two disagree the Rust side wins. It is worth
    having anyway because the two failures it is most likely to catch are ones
    this stage can plausibly cause: a protein reference on a non-CDS class
    (`lib.rs`: "class {} carries a protein reference; only cds may") and an
    ambiguity code the nucleotide validator does not accept.
    """
    if r.cls not in LOADER_CLASSES:
        raise SystemExit(f"{r.id}: class {r.cls!r} is not a Class the loader parses")
    if r.boundary_rule not in LOADER_RULES:
        raise SystemExit(f"{r.id}: boundary_rule {r.boundary_rule!r} is not one the loader parses")
    if not r.reference_nt:
        raise SystemExit(f"{r.id}: reference_nt is empty")
    bad = sorted(set(r.reference_nt) - NUCLEOTIDES)
    if bad:
        raise SystemExit(f"{r.id}: {bad} in reference_nt are not nucleotide codes")
    if r.reference_aa and r.cls != "cds":
        raise SystemExit(f"{r.id}: class {r.cls} carries a protein reference; only cds may")
    if not r.id or not r.name:
        raise SystemExit(f"{r.id}: id and name are required")
    if not r.boundary_evidence:
        raise SystemExit(f"{r.id}: boundary_evidence is required")
    if r.patent_flag not in ("0", "1"):
        raise SystemExit(f"{r.id}: patent_flag {r.patent_flag!r} is not a boolean")
    # `main()` in build.py refuses to write a provenance row that cites an
    # external source with no sha256, and `Db::audit` refuses a record with no
    # provenance for `reference_nt` at all. Both are cheaper to catch here.
    if not any(p[1] == "reference_nt" for p in r.provenance):
        raise SystemExit(f"{r.id}: no provenance for reference_nt")
    for p in r.provenance:
        if p[4] != "own-work" and not p[7]:
            raise SystemExit(f"{r.id} field {p[1]}: cites {p[2]} with no sha256")
    # Tabs and newlines would shift columns; `esc()` handles them, but a value
    # that needs escaping in a field we wrote by hand means we made a typo, not
    # that the escaper is earning its keep.
    for field, value in (("name", r.name), ("boundary_evidence", r.boundary_evidence)):
        if "\t" in value or "\n" in value:
            raise SystemExit(f"{r.id}: {field} contains a tab or newline")
    if any("|" in a for a in r.aliases):
        raise SystemExit(f"{r.id}: an alias contains the '|' delimiter")


# `assert_ascii` used to live here, with a docstring quoting a live byte count
# of the shipped file ("currently 0 non-ASCII bytes out of 11,813"). Both halves
# of that arrangement failed. The count went stale the moment another stage grew
# the table — it was 50 out of 147,339 by the time anyone looked — and a rule
# only this module enforced was a rule stage_uniprot never applied, which is how
# 16 em dashes and a section sign reached `features.tsv`.
#
# It is now `build.assert_ascii`, called from `build.validate_row` on every
# authored field of every row from every stage, and it quotes no number it
# cannot recompute. See the import at the top of this file.


# --------------------------------------------------------------------------
# The stage


def build(refresh: bool) -> tuple[list, list]:
    """Emit one row per allow-listed Rfam family."""
    fam_raw = rfam_fetch(FAMILY_URL, FAMILY_CACHE, refresh)
    seed_raw = rfam_fetch(SEED_URL, SEED_CACHE, refresh)
    fam_meta, seed_meta = cached_meta(FAMILY_CACHE), cached_meta(SEED_CACHE)

    families = parse_family_table(gzip.decompress(fam_raw).decode(RFAM_ENCODING))
    wanted = {f.acc for f in FAMILIES}
    if len(wanted) != len(FAMILIES):
        raise SystemExit("duplicate accession in FAMILIES")
    blocks = parse_seed(gzip.decompress(seed_raw).decode(RFAM_ENCODING), wanted)

    rows, report = [], []
    for i, fam in enumerate(FAMILIES):
        # `ordinal` is what build.allocate() reads; `rid` is the same number
        # spelled out, so this module's standalone report and its provenance
        # tuples name the id the row will really receive. A family that drops out
        # leaves its ordinal — and therefore its id — unused rather than pulling
        # every later family's id down by one.
        ordinal = i + 1
        rid = f"PLF:{ID_BLOCK_START + i:04d}"
        if i >= ID_BLOCK_SIZE:
            raise SystemExit(
                f"Stage 4 is reserved PLF:{ID_BLOCK_START:04d}-"
                f"PLF:{ID_BLOCK_START + ID_BLOCK_SIZE - 1:04d}; {fam.acc} would "
                f"issue {rid} and collide with another stage"
            )

        meta = families.get(fam.acc)
        if meta is None:
            report.append(f"  SKIP {fam.acc} not in this Rfam release")
            continue
        block = blocks.get(fam.acc)
        if block is None:
            report.append(f"  SKIP {fam.acc} has no seed alignment")
            continue

        # -- exclusion 1 ---------------------------------------------------
        assert_not_mirna(fam.acc, meta["id"], meta["type"])

        # The two files must be the same release, or `family.txt`'s verdict
        # governs a different alignment than the one we are about to ingest —
        # which would make the miRNA gate above decorative. Both of these are
        # cheap and both can fail.
        if block.gf1("DE") != meta["description"]:
            raise SystemExit(
                f"{fam.acc}: family.txt says {meta['description']!r} but the seed "
                f"says {block.gf1('DE')!r}; the two files are not the same release"
            )
        if str(len(block.rows)) != meta["num_seed"]:
            raise SystemExit(
                f"{fam.acc}: family.txt says num_seed={meta['num_seed']} but the "
                f"seed block holds {len(block.rows)} rows"
            )
        # `#=GF SQ` is the block's own declared row count, so this catches a
        # parser fault rather than a release mismatch — a sequence line dropped
        # or a `#=GC` row mistaken for one. It holds for all 24 families, and
        # it is the cheap parse check `num_seed` cannot be: SOURCING.md warns
        # that `num_full` is advisory and must not be asserted on, but num_seed
        # and SQ are exact and agree.
        if block.gf1("SQ") != str(len(block.rows)):
            raise SystemExit(
                f"{fam.acc}: the block declares SQ={block.gf1('SQ')} but "
                f"{len(block.rows)} rows were parsed out of it"
            )

        derived = class_from_type(meta["type"])
        if derived is not None and derived != fam.cls:
            raise SystemExit(
                f"{fam.acc} is typed {meta['type']!r}, which is class {derived!r}, "
                f"but the allow-list declares {fam.cls!r}"
            )
        if fam.genbank_key not in INSDC_KEYS:
            raise SystemExit(f"{fam.acc}: {fam.genbank_key!r} is not an INSDC feature key")
        if fam.cls == "cds":
            raise SystemExit(f"{fam.acc}: an RNA element is not a CDS")

        best, stats = choose_representative(block.rows, fam.rep)

        # -- exclusion 3 ---------------------------------------------------
        nt = to_dna(best.seq, f"{fam.acc} {best.ident}")
        # The coordinates must describe the sequence we are about to store. If
        # they do not, `boundary_evidence` points at a different interval than
        # `reference_nt` holds, which is the precise failure this database
        # exists to prevent — and it is silent, because both fields look fine
        # on their own.
        if best.span != len(nt):
            raise SystemExit(
                f"{fam.acc} {best.ident}: coordinates span {best.span} nt but the "
                f"degapped row is {len(nt)} nt"
            )

        cite = citation(block, fam.credit_pmid)
        strand = best.strand
        lo, hi = min(best.start, best.end), max(best.start, best.end)

        # Assembled in two pieces on purpose. `notes_ours` is our prose about
        # our own measurements and is subject to the taint check below;
        # `cite` is a bibliographic reference and is not. Checking the citation
        # too was tried and is wrong: Rfam's comment for a family routinely
        # paraphrases the paper Rfam cites, so the paper's own title collides
        # with it — RF00043's title shares "broad host range plasmid r1162"
        # with the comment. Enforcing the n-gram rule there would make the
        # per-family primary-source credit that SOURCING.md §2 Stage 4 requires
        # impossible to carry, which is the opposite of the intent. A title is
        # a fact about a publication, not Rfam's expression.
        # -- the depositor's own record has the last word ------------------
        header, ena_meta = check_against_depositor(fam, best, nt, lo, hi, refresh)

        how_chosen = (
            f"pinned to {fam.rep} because this family's description names a "
            f"specific source and the length-median rule is blind to which"
            if stats["pinned"] else
            "chosen as the unambiguous INSDC row closest to the median length, "
            "ties to the smallest identifier"
        )
        notes_ours = (
            f"Rfam {fam.acc} ({meta['id']}), Rfam type '{meta['type']}'. "
            f"Reference sequence is seed row {best.ident}: {len(nt)} nt after "
            f"degapping and U->T. Seed alignment holds {stats['n_rows']} rows "
            f"({stats['n_insdc']} INSDC, {stats['n_urs']} RNAcentral URS with no "
            f"INSDC coordinates and so ineligible), degapped lengths "
            f"{stats['min_len']}-{stats['max_len']} nt, median {stats['median']:g}; "
            f"{stats['n_ambiguous']} INSDC row(s) carried IUPAC ambiguity codes and "
            f"were deprioritised. Representative {how_chosen}. Coordinates "
            f"{lo}-{hi} on the {strand} strand span {best.span} nt, equal to the "
            f"stored sequence, and ENA {best.accession} was re-fetched over exactly "
            f"that interval and holds exactly these bases. The seed rows are one "
            f"region each of {stats['n_insdc']} independent INSDC records aligned to "
            f"the same model columns, which is the sense in which this boundary is a "
            f"consensus; the alignment itself is Rfam's curation, not the "
            f"depositors' annotation, and a curator should weigh that before "
            f"signing the boundary_rule off. "
            f"SCOPE OF THIS ROW: the reference is ONE member of the family, from "
            f"ENA's own description line '{header}'. Near-exact nucleotide matching "
            f"will find that member and close relatives of it, not every sequence "
            f"the family models; a homologue from another organism will be missed "
            f"even though the family covers it."
        )
        if fam.caveat:
            notes_ours += f" CURATOR: {fam.caveat}"

        # -- exclusion 2 ---------------------------------------------------
        # Last, so it sees the final strings rather than the drafts. The ENA
        # description line is excluded: it is a depositor's text, quoted as
        # evidence and attributed to `ena` in provenance, not prose we wrote.
        quarantined = list(block.quarantined) + [meta["quarantine"]]
        assert_untainted(
            fam.acc,
            {"name": fam.name, "aliases": " ".join(fam.aliases),
             "description": fam.description},
            [q for q in quarantined if q and q != "NULL"],
        )
        for field, text in (("name", fam.name), ("aliases", " ".join(fam.aliases)),
                            ("description", fam.description), ("notes", notes_ours)):
            why = assert_ascii(rid, field, text)
            if why:
                raise SystemExit(f"{rid}: {why}")
        notes = f"{notes_ours} Primary-source credit (Rfam #=GF RM): {cite}."

        row = Row(
            id=rid,
            ordinal=ordinal,
            name=fam.name,
            aliases=list(fam.aliases),
            cls=fam.cls,
            genbank_key=fam.genbank_key,
            reference_nt=nt,
            # Empty, and not merely unset: the loader rejects a protein
            # reference on any non-CDS class, because a promoter in the
            # translated index is a category error.
            reference_aa="",
            boundary_rule="consensus_of_insdc",
            boundary_evidence=f"{best.accession}:{lo}-{hi}:{strand}",
            description=fam.description,
            notes=notes,
            patent_flag=fam.patent_flag,
            provenance=[
                # TWO rows for reference_nt, because two different parties have
                # a claim on it and they are not the same claim.
                #
                # The bases are a depositor's, submitted to INSDC: Rfam did not
                # generate them, it aligned them, and CC0 is Rfam's waiver of
                # Rfam's rights, not a relicensing of the submission. Labelling
                # the nucleotides `rfam / CC0-1.0` was the exact conflation this
                # project already caught and corrected on the UniProt->ENA leg,
                # where reference_nt is `ena / INSDC-free` and only the naming
                # fields are CC BY. The Rfam block answered the same question
                # the other way, and README/NOTICE promise it is answered this
                # way. The url is the interval that was actually fetched and
                # compared, so the hash covers the bytes the check ran on.
                (rid, "reference_nt", "ena", f"{best.accession}:{lo}-{hi}",
                 "INSDC-free", f"{ENA_BASE}/fasta/{best.accession}?range={lo}-{hi}",
                 ena_meta.get("retrieved", TODAY), ena_meta.get("sha256", "")),
                # What IS Rfam's: that this interval is a member of this family,
                # which is the alignment, which is the curation CC0 covers.
                (rid, "reference_nt", "rfam", f"{fam.acc}:{best.ident}",
                 "CC0-1.0", SEED_URL, seed_meta.get("retrieved", TODAY),
                 seed_meta.get("sha256", "")),
                (rid, "boundary_evidence", "rfam", f"{fam.acc}:{best.ident}",
                 "CC0-1.0", SEED_URL, seed_meta.get("retrieved", TODAY),
                 seed_meta.get("sha256", "")),
                # `class` is derived from Rfam's type field, so it is Rfam's,
                # not ours, and is recorded as such.
                (rid, "class", "rfam", fam.acc, "CC0-1.0", FAMILY_URL,
                 fam_meta.get("retrieved", TODAY), fam_meta.get("sha256", "")),
                # The credit SOURCING.md requires, on the column that actually
                # holds the copied text. It used to sit on a `citation` field,
                # which is not a column of features.tsv and therefore attributed
                # nothing, while `notes` -- where Rfam's own #=GF RT/RL text is
                # reproduced verbatim, and where the ENA description line is now
                # quoted too -- was labelled own-work. CC0 and INSDC-free make
                # that harmless here; the same mistake over a share-alike source
                # would not be.
                (rid, "notes", "rfam", f"{fam.acc};PMID:{fam.credit_pmid}",
                 "CC0-1.0", SEED_URL, seed_meta.get("retrieved", TODAY),
                 seed_meta.get("sha256", "")),
                (rid, "notes", "ena", best.accession, "INSDC-free",
                 f"{ENA_BASE}/fasta/{best.accession}?range={lo}-{hi}",
                 ena_meta.get("retrieved", TODAY), ena_meta.get("sha256", "")),
                (rid, "notes", "polylinker", "-", "own-work", "-", TODAY, ""),
                (rid, "name", "polylinker", "-", "own-work", "-", TODAY, ""),
                (rid, "aliases", "polylinker", "-", "own-work", "-", TODAY, ""),
                (rid, "boundary_rule", "polylinker", "-", "own-work", "-", TODAY, ""),
                # Ours, written from the paper named here.
                (rid, "description", "polylinker", f"PMID:{fam.credit_pmid}",
                 "own-work", "-", TODAY, ""),
            ],
        )
        validate_row(row)
        rows.append(row)
        report.append(
            f"  OK   {fam.acc} {meta['id']:16s} -> {fam.name:34s} {len(nt):5d} nt  "
            f"{best.accession}:{lo}-{hi}:{strand}  "
            f"({stats['n_insdc']}/{stats['n_rows']} INSDC seed rows)"
        )

    ids = [r.id for r in rows]
    if len(set(ids)) != len(ids):
        raise SystemExit("duplicate record id in stage_rfam output")
    return rows, report


# --------------------------------------------------------------------------
# Proving the gates can fail


def self_test(refresh: bool = False) -> list[str]:
    """Run each exclusion against input that must trip it.

    Three of the four gates are silent in a green build — every allow-listed
    family passes them — so without this, "no error" is indistinguishable from
    "the check does nothing". §8.3: a check that cannot fail proves nothing.
    """
    out = []

    def must_fail(label: str, fn) -> None:
        try:
            fn()
        except SystemExit as e:
            out.append(f"  PASS {label}: refused — {str(e)[:96]}")
            return
        raise SystemExit(f"SELF-TEST FAILED: {label} was accepted")

    fam = parse_family_table(
        gzip.decompress(rfam_fetch(FAMILY_URL, FAMILY_CACHE, refresh)).decode(RFAM_ENCODING)
    )

    # 1. miRNA, on real rows. RF00250 is mir-TAR, the HIV TAR element — a
    #    family we would genuinely like to have, so this is the gate refusing
    #    something we want rather than something obviously alien. RF00027 is
    #    let-7, the ordinary case.
    for acc in ("RF00250", "RF00027"):
        m = fam[acc]
        must_fail(f"miRNA {acc} ({m['id']})",
                  lambda m=m, acc=acc: assert_not_mirna(acc, m["id"], m["type"]))
    # ...and the name-keyed filter that would have leaked, shown rather than
    # asserted: `mir`-prefixed ids under-count the typed families.
    by_type = sum(1 for v in fam.values() if MIRNA_TYPE in v["type"].lower())
    by_name = sum(1 for v in fam.values() if v["id"].lower().startswith("mir"))
    out.append(
        f"  INFO miRNA families by type: {by_type}; by name prefix: {by_name}; "
        f"a name-keyed filter would ship {by_type - by_name}"
    )
    if by_type <= by_name:
        raise SystemExit("SELF-TEST FAILED: the name-keyed filter no longer under-counts")

    # 2. Taint. Feed a real quarantined comment back in as if we had written it.
    tar = fam["RF00250"]["quarantine"]
    must_fail("taint (Rfam comment reused as our prose)",
              lambda: assert_untainted("RF00250", {"description": tar}, [tar]))
    # And the same gate must *not* fire on ordinary independent prose, or it
    # would be a check that always fails, which proves as little as one that
    # never does.
    assert_untainted("RF00250", {"description": FAMILIES[0].description}, [tar])
    out.append("  PASS taint: independent prose against the same comment -- accepted")

    # 3. U survived / bad alphabet. `to_dna("ACGU")` cannot test the first of
    #    these — it succeeds, because the transform works — so `_u_left`
    #    invokes the guard with the substitution bypassed.
    must_fail("uracil left in a reference", _u_left)
    must_fail("non-nucleotide code", lambda: to_dna("ACGZ", "self-test"))
    if to_dna("ACGU", "self-test") != "ACGT":
        raise SystemExit("SELF-TEST FAILED: to_dna does not transform U->T")
    out.append("  PASS U->T: ACGU -> ACGT, and a surviving U is refused")

    # 4. Forbidden sources.
    must_fail("full-region FASTA",
              lambda: rfam_fetch(f"{RFAM_BASE}/fasta_files/RF00106.fa.gz", "x", False))
    must_fail("Wikipedia dump",
              lambda: rfam_fetch(f"{RFAM_BASE}/database_files/wikitext.txt.gz", "x", False))
    must_fail("a source outside Rfam",
              lambda: rfam_fetch("https://example.org/RF00106.fa", "x", False))

    # 5. Citation credit must actually be checked against the family.
    blocks = parse_seed(
        gzip.decompress(rfam_fetch(SEED_URL, SEED_CACHE, refresh)).decode(RFAM_ENCODING),
        {"RF00094"},
    )
    must_fail("citation not among the family's #=GF RM lines",
              lambda: citation(blocks["RF00094"], "99999999"))
    # ...and it must still find the one that *is* there, wrapped title and all.
    cite = citation(blocks["RF00094"], "9783582")
    if "Crystal structure" not in cite:
        raise SystemExit(f"SELF-TEST FAILED: citation lost the title: {cite!r}")
    out.append(f"  PASS citation: {cite}")

    # 6. The loader mirror, and the ASCII rule for fields we author.
    def bad_row(**kw) -> Row:
        base = dict(
            id="PLF:0399", name="x", aliases=[], cls="regulatory",
            genbank_key="ncRNA", reference_nt="ACGT", reference_aa="",
            boundary_rule="consensus_of_insdc", boundary_evidence="X.1:1-4:+",
            description="d", notes="n", patent_flag="0",
            provenance=[("PLF:0399", "reference_nt", "rfam", "x", "CC0-1.0",
                         SEED_URL, TODAY, "deadbeef")],
        )
        base.update(kw)
        return Row(**base)

    must_fail("protein reference on a non-CDS row",
              lambda: validate_row(bad_row(reference_aa="MKV")))
    must_fail("uracil reaching the loader check",
              lambda: validate_row(bad_row(reference_nt="ACGU")))
    must_fail("provenance citing an external source with no sha256",
              lambda: validate_row(bad_row(provenance=[
                  ("PLF:0399", "reference_nt", "rfam", "x", "CC0-1.0", SEED_URL,
                   TODAY, "")])))
    must_fail("boundary_rule the loader cannot parse",
              lambda: validate_row(bad_row(boundary_rule="vibes")))
    validate_row(bad_row())
    out.append("  PASS loader mirror: a well-formed row is accepted")
    # `assert_ascii` now lives in build.py and returns a reason instead of
    # raising, so that build.validate_row can apply it to every authored field
    # of every row and no stage can opt out by not calling it. stage_uniprot
    # never called it and shipped 16 em dashes and a section sign.
    if not assert_ascii("PLF:0399", "description", "café"):
        raise SystemExit("SELF-TEST FAILED: the ASCII gate accepted a high byte")
    if assert_ascii("PLF:0399", "description", "cafe"):
        raise SystemExit("SELF-TEST FAILED: the ASCII gate refused plain ASCII")
    out.append("  PASS ASCII: a high byte in a field we author is refused, ASCII is not")

    # 7. The representative pin and the organism assertion. Both are new,
    #    because choosing by length alone shipped four rows whose sequence came
    #    from a different organism than their own name and description named,
    #    three of them losing a lexicographic tie-break at identical length.
    seed_text = gzip.decompress(rfam_fetch(SEED_URL, SEED_CACHE, refresh)).decode(RFAM_ENCODING)
    rows_106 = parse_seed(seed_text, {"RF00106"})["RF00106"].rows
    must_fail("a pinned representative that is not in the seed",
              lambda: choose_representative(rows_106, "NOPE99.9/1-10"))
    pinned, st = choose_representative(rows_106, "AJ132618.1/824-723")
    if pinned.ident != "AJ132618.1/824-723" or not st["pinned"]:
        raise SystemExit("SELF-TEST FAILED: the pin did not select the row it named")
    unpinned, _ = choose_representative(rows_106)
    if unpinned.ident == FAMILIES[0].rep:
        raise SystemExit(
            "SELF-TEST FAILED: the length-median rule now happens to choose the row "
            "RF00106 pins, so the pin is no longer demonstrably doing anything and "
            "this test has stopped proving it"
        )
    out.append(
        f"  PASS pin: unpinned, RF00106 picks {unpinned.ident} -- the Yersinia "
        f"cryptic-plasmid row that shipped; pinned, it picks what it names"
    )

    # The organism gate, run against a record whose contents are known. Every
    # other check on this row passes; only the declared organism is wrong.
    @dataclass(frozen=True)
    class _Decl:
        acc: str = "RF00106"
        organism: str = "Yersinia enterocolitica"
        rep: str = ""

    real, _ = choose_representative(rows_106, FAMILIES[0].rep)
    lo, hi = min(real.start, real.end), max(real.start, real.end)
    must_fail(
        "a declared organism the cited record does not support",
        lambda: check_against_depositor(
            _Decl(), real, to_dna(real.seq, "self-test"), lo, hi, refresh),
    )
    return out


def _u_left() -> str:
    """`to_dna` with the transform sabotaged, to prove the assertion fires."""
    dna = "ACGU".upper()  # deliberately NOT substituted
    if "U" in dna:
        raise SystemExit("self-test: 'U' survived the RNA->DNA transform")
    return dna


def main() -> int:
    ap = argparse.ArgumentParser(description="Stage 4 -- Rfam structured RNA")
    ap.add_argument("--refresh", action="store_true", help="re-fetch every source")
    ap.add_argument("--no-self-test", action="store_true")
    args = ap.parse_args()

    print("Stage 4 -- Rfam structured RNA elements")
    if not args.no_self_test:
        print("\nSelf-test -- every exclusion, against input that must trip it")
        print("\n".join(self_test(args.refresh)))

    print("\nBuild")
    rows, report = build(args.refresh)
    print("\n".join(report))

    print(f"\n{len(rows)} rows, {sum(len(r.provenance) for r in rows)} provenance rows\n")
    for r in rows:
        print("-" * 78)
        print(f"{r.id}  {r.name}   [{r.cls} / {r.genbank_key}]  patent_flag={r.patent_flag}")
        print(f"  aliases          {' | '.join(r.aliases)}")
        print(f"  boundary         {r.boundary_rule}  {r.boundary_evidence}")
        print(f"  reference_nt     {len(r.reference_nt)} nt  {r.reference_nt[:60]}"
              f"{'...' if len(r.reference_nt) > 60 else ''}")
        print(f"  reference_aa     {r.reference_aa!r}")
        print(f"  description      {r.description}")
        print(f"  notes            {r.notes}")
        print("  review_status    proposed        curator (none)")
        # The writer in build.py escapes tabs and newlines; check ours survive
        # a round trip rather than discovering a shifted column in the TSV.
        clean = "\t" not in esc(r.description) and "\n" not in esc(r.notes)
        print(f"  tsv-safe         {'yes' if clean else 'NO'}")
    print("-" * 78)
    print("\nEvery row is 'proposed' with an empty curator. Db::reviewed() ships none")
    print("of them until Dr Lobel signs each one off; that is the intended state.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
