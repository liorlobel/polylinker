#!/usr/bin/env python3
"""Stage 5 — hand-curated designed parts: epitope tags, protease sites, 2A, linkers.

SOURCING.md Gap 1. These are *Class C* features: "synthetic/designed (boundaries
are stipulated by a paper)". There is no catalogue to harvest — UniProt returns
nothing for FLAG or His6 — so the allow-list below is written by hand, one paper
per row, and that citation *is* the provenance.

Two kinds of row, decided 2026-07-28
------------------------------------

Until 2026-07-28 the loader refused a protein-only Class C row twice over: every
row had to carry `reference_nt`, and only class `cds` could carry
`reference_aa`. A tag is a peptide -- FLAG is `DYKDDDDK` -- so the only way one
could become a loadable row was with nucleotides, and there were exactly two
ways to get those:

  * **Back-translate the peptide.** Forbidden outright, then and now. Choosing
    codons is writing a sequence that no record contains, which is the precise
    failure this whole build exists to prevent. It would also be useless: it
    would match only the vectors that happened to make the same codon choices.

  * **Take the codons out of the natural gene the peptide came from**, verified
    by translation. Legitimate, and that is what the nucleotide route below
    does -- but it exists only for the tags that HAVE a natural parent. FLAG,
    His6, Strep-tag, SBP, AviTag, ALFA and the GS/EAAAK linkers were designed or
    selected; there is no gene to read them out of. Twenty parts sat declared,
    cited, verified and unissued.

The PI resolved it: *"Yes -- add these sequences, but make sure they are fused
to an ORF, otherwise ignored."* `synthetic_part` may now carry `reference_aa`,
with or without nucleotides, so this stage emits two shapes:

  NUCLEOTIDE ROUTE   the part has a verified UniProt parent and clears MIN_NT.
                     `reference_nt` is codons sliced out of that gene and
                     re-translated; `reference_aa` stays empty. Eight rows,
                     unchanged from before, byte for byte in their sequences.

  PEPTIDE ROUTE      the part has no usable gene, and clears the
                     measured-occurrence gate (see `Part.occurrences`).
                     `reference_aa` is the residue string, verified at build
                     time against a fetched record -- a wwPDB polymer entity,
                     or the UniProt parent for a part that has one but is too
                     short for MIN_NT; `reference_nt` is empty. Nineteen rows.

Never both. A row carrying nucleotides is matched by the tier-1 index and, if it
had a peptide, by the ungated tier-2 scan as well -- and the annotator's
exact-match and ORF-fusion rules apply only to peptide-ONLY rows. Giving the
eight nucleotide rows a peptide as well would therefore make a nine-residue
epitope matchable with no ORF requirement at all, which is a behaviour change
nobody asked for. It is a separate decision with its own tests; see
features/README.md, "Known gaps".

One of the twenty-eight still emits nothing, and the reason is a measurement
---------------------------------------------------------------------------

It was "there is no gene to take codons from" until 2026-07-28, then a flat
peptide length floor of eight residues, which held six parts including His6 and
both TEV sites. Both reasons are gone. The floor measured a proxy: DDDDK at five
residues is perfectly specific and IEGR at four is unusable, so length does not
carry the answer and no value of it could. Specificity does, and specificity can
be measured -- see `Part.occurrences` for what was measured and the gate that
reads it.

Under the measurement five of those six ship and one is held: factor Xa, whose
four residues occurred 154 times in the corpus, in 16.4% of the files, none of
them read by anybody. It ships the day somebody reads them.

Every declared part keeps its ordinal whether or not it emits
-------------------------------------------------------------

All twenty-eight are declared, in a fixed order, and `ordinal` comes from that
order. A part that cannot be built leaves its PLF id unissued rather than
letting the next part slide into it. When the schema learns to carry a protein
reference on `synthetic_part`, FLAG lands on the id that has been reserved for
it since this file was written, and nothing that already shipped moves. That is
the entire reason ids are allocated from the declaration and not from the
output.

The residue strings are checked, not trusted
--------------------------------------------

Each `aa` below was verified against fetched records by the curation pass that
produced this allow-list. This file does not take that on faith, and the gate is
the same shape on both routes: **the peptide must be located, exactly once, in a
sequence fetched at build time.**

  NUCLEOTIDE ROUTE   located in a UniProt canonical, then the codons sliced out
                     of the ENA CDS must re-translate to it.
  PEPTIDE ROUTE      located in the deposited one-letter sequence of the wwPDB
                     polymer entity named in `pdb_entity`, or -- for a part that
                     has a verified parent but is too short for MIN_NT to take
                     codons from it -- in that parent's UniProt canonical.

The second half of that clause is new, and it exists for exactly one row.
Enterokinase declares parent P00760 and is witnessed on it; the parent cannot
supply fifteen base pairs of reference, but it can and does supply the check.
Refusing the row for want of a *wwPDB* witness when a fetched witness is right
there would have been the letter of the rule against its point.

A single wrong residue in this table drops the row; it cannot ship. That is what
makes a hand-written sequence table safe to have at all — and it is why the
peptide route was not simply switched on when the schema allowed it. Without a
fetchable witness the peptide rows would go from "declared but unissued" to
"shipped, unverified", which is worse than the status quo it replaced.

Usage
-----
    python features/build/stage_curated.py            # standalone
    python features/build/stage_curated.py --self-test
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass, replace
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

# Reused, never reimplemented — same reason stage_uniprot gives: an independent
# translate() could agree with a record exactly where build.py's disagrees, and
# the verification would then be measuring the wrong code.
from build import (  # noqa: E402
    TODAY,
    Row,
    cached_meta,
    cds_matches_protein,
    fetch,
    translate,
)
from stage_uniprot import (  # noqa: E402
    ENA_FASTA,
    UNIPROT_JSON,
    ena_cds_nt,
    pick_uniprot,
)

# Stage 5's reserved block. build.STAGES is the authority; load_stage() refuses
# to run this module if these disagree with it.
PLF_BLOCK_BASE = 3000
PLF_BLOCK_SIZE = 1000

# A nucleotide reference shorter than this is not a feature, it is noise. The
# curation pass computed the *protein* false-positive rates and flagged four
# parts as below budget (IEGR at k=4 is ~60,000x over); the nucleotide numbers
# are kinder but the practical objection is worse — a 12 bp reference is shorter
# than any k-mer seed a tier-1 index would use, so it is not merely noisy, it is
# unindexable. Factor Xa (IEGR, 12 bp) and enterokinase (DDDDK, 15 bp) are held
# back by this rule even though both have a clean natural parent.
MIN_NT = 27

# THERE IS NO PEPTIDE LENGTH FLOOR ANY MORE, and this comment is what used to
# be one. `MIN_PEPTIDE_AA = 8` held six parts, including His6 and both TEV
# sites, on two arguments that both looked sound:
#
#   (a) A FALSE-POSITIVE BUDGET computed as 20^-L against the residue positions
#       of a translated 5 kb plasmid.
#   (b) A SEEDING CLIFF: K_PROTEIN = 5 with Config::min_seeds = 3 needs 7
#       residues to make three windows, `Index::build` listed a record in
#       `short()` only when it indexed ZERO words, and so a 6-residue peptide
#       was seeded, unchainable, unreported and never found. The floor sat one
#       residue above that cliff.
#
# (b) IS FIXED IN THE CODE, which is where it always belonged.
# `Index::unchainable` routes any record with fewer indexed words than the
# caller's `min_seeds` to an exact substring scan, so "too few words to chain"
# is a route rather than a hole, and it stays a route as `min_seeds` rises. No
# constant here could do that: at min_seeds = 5 the eight-residue FLAG row --
# already shipped and signed -- fell down the same cliff.
#
# (a) IS THE WRONG QUESTION, and the measurement says so. Every peptide in this
# table was counted against 73 real plasmid and contig files from the PI's own
# machine, 17,061,931 residues of ORFs, counting only occurrences the shipped
# fusion gate would report. Against 20^-L over that corpus:
#
#                    expected by chance   observed   O/E
#     IEGR      4          106.6            154      1.4
#     DDDDK     5            5.33             0      0
#     HHHHHH    6            0.267            8     30
#     LVPRGS    6            0.267            0      0
#     ENLYFQG   7            0.0133           0      0
#     DYKDDDDK  8            0.00067          0      0    (shipped, control)
#     (GGGGS)3 15          ~0                 0      0    (shipped, control)
#
# The model is not badly calibrated -- within 1.5x on IEGR, correctly ~0 from
# six residues up. It fails for a different reason and the reason is fatal: it
# estimates CHANCE OCCURRENCE, and what a curator needs is the fraction of the
# hits the tool actually produces that are wrong. It labels the row with 8 hits
# (His6, O/E 30) as the marginal one and the row with 0 hits (DDDDK) as twenty
# times cleaner. Reading the hits says the opposite on both counts: all eight
# His6 occurrences are real tags -- C-terminal at -0 residues from the stop,
# behind a GG linker, in files named for the tag -- and DDDDK did not occur at
# all. No calibration of 20^-L produces that answer, because the model has no
# term for design, and design is what a tag IS.
#
# The old comment already half-knew this. It said His6 "deserves a refusal
# INDEPENDENT of length" because a homopolymer is not modelled by 20^-L, citing
# 6,783 PDB entities carrying HHHHHHHH. That observation is correct about
# composition and produced the wrong shipping decision: composition predicts
# noise, and the measurement found eight true positives and zero false ones.
#
# So length stops being a gate. What replaces it is a per-part record of what
# was measured and what was read -- see `Part.occurrences` and the gate in
# `build_peptide` -- and one STRUCTURAL floor that is not about specificity at
# all:

# The shortest peptide the annotator will accept, and it must agree with
# MIN_PART_AA in crates/pl-features/src/annotate.rs, which refuses anything
# below it out loud.
#
# Not a specificity rule. ORF_MIN_AA = MIN_PART_AA + PARTNER_MIN = 25 is the
# `Params::min_aa` handed to `find_orfs`, while the fusion predicate itself only
# asks for `aa_len >= tag_aa + PARTNER_MIN`. A shorter part would therefore be
# findable inside a 25-residue ORF and silently invisible inside a 24-residue
# one, because no ORF that short is ever searched. Admitting one means moving
# ORF_MIN_AA with it, in that file, and saying so here.
MIN_PART_AA = 5

# The floor that was retired above, kept for one job: deciding which rows the
# measurement ADMITS, as opposed to which rows it merely describes.
#
# Two things forced this to be a constant rather than a paragraph applied to
# every peptide row.
#
# It is a SIGNATURE boundary. `notes` is in SIGNED_COLUMNS, so a sentence
# appended to a row already signed lapses that signature. Appending the
# occurrence paragraph to all twenty peptide rows lapsed fourteen of Dr Lobel's
# eighty-four, dropped FLAG, Strep-tag, AviTag, SBP-tag, both GGGGS linkers and
# eight others out of the default search, and turned six tests red. The rows the
# measurement admits are exactly the rows the retired floor held, so the same
# number that used to hold them now scopes the sentence that releases them.
#
# It is also a TRUTH boundary, and that is the reason it must not simply be
# `False`. The sentence says the measurement "is what admits the row, in place
# of the peptide length floor this table used to carry". That is true of His6,
# both TEV sites, thrombin and enterokinase, all of which the floor held. It is
# false of FLAG at eight residues and of SBP-tag at thirty-eight: those cleared
# the floor and were admitted by it, and writing otherwise would put a false
# claim about a row's grounds into the column a curator signs.
#
# A part of eight residues or more that is added from here on is admitted the
# same way FLAG was and records its occurrences the same way FLAG does: in
# `Part.occurrences`, which `occurrence_verdict` reads for every peptide row
# regardless of length. Only the prose is scoped, never the gate.
RETIRED_PEPTIDE_FLOOR = 8

# RCSB's data API. One endpoint, one field: the deposited one-letter sequence of
# a named polymer entity. SOURCING.md §1 clears `wwpdb / CC0-1.0` narrowly — the
# CC0 dedication is over the PDB ARCHIVE, while RCSB's own website layer is
# separately CC BY 4.0 — so this build reads the deposited sequence and no
# annotation. The entity's deposited description is printed in the build report
# so a reviewer can see the entity is what the table says it is, and is
# deliberately NOT written into any row.
RCSB_ENTITY = "https://data.rcsb.org/rest/v1/core/polymer_entity/{}/{}"


# --------------------------------------------------------------------------
# The allow-list


@dataclass(frozen=True)
class Part:
    """One designed part: what it is, who published it, and how to source it."""

    name: str
    aliases: tuple[str, ...]
    aa: str
    """The peptide, as verified against fetched records. Re-checked at build
    time against a freshly fetched UniProt canonical — see the module docstring.
    Never the source of a shipped sequence on its own."""
    cls: str
    genbank_key: str
    boundary_rule: str
    boundary_evidence: str
    """For Class C the boundary is stipulated by a paper, so the evidence is the
    citation, which SOURCING.md §3 explicitly provides for: "accession.version +
    coords + strand, OR DOI + table/figure". We do not invent coordinates for a
    peptide whose extent was decided in prose."""
    citation: str
    description: str
    """Ours, written from the cited paper. Never from a vendor manual, a
    SnapGene list or an Addgene map."""
    witness: str
    """What was actually fetched to verify the residues, named so a reviewer can
    re-fetch it. A tag whose only witness is a product manual is not in this
    table at all — see `dropped_from_the_allow_list()`."""
    parent_uniprot: str = ""
    """UniProt accession of the natural protein this peptide is a piece of, or
    "" if it was designed and has no gene. Only parts with one take the
    nucleotide route."""
    pdb_entity: str = ""
    """`ENTRY_N` for the wwPDB polymer entity named in `witness`, machine
    readable, e.g. `8RMO_1`.

    This is the difference between a witness that was checked once by a human
    and a witness the build checks every run. `witness` is prose; this is a
    fetch. A part taking the peptide route without one is refused: it would ship
    the `aa=` literal above straight into features.tsv, which turns a declared
    row into an unverified one — worse than leaving it unissued."""
    no_gene: str = "designed or selected peptide; there is no gene to take codons from"
    """Why the NUCLEOTIDE route is unavailable, in one line, printed every run.

    This field was called `hold` until 2026-07-28, when it stopped being the
    reason a row was held: fourteen of the parts it described now ship a peptide
    reference instead, and the six that are still held are held by
    MIN_PEPTIDE_AA. Renaming it rather than leaving it is the point — the text
    is still true and still worth printing, but a field called `hold` on a row
    that is no longer held is a stale assertion sitting in the data model.

    Three different situations hide behind an empty `parent_uniprot` and they
    are not interchangeable: a peptide that never had a gene, a peptide whose
    gene exists but whose accession nobody established from a fetched record,
    and an engineered variant of a natural junction. Only the middle one is one
    lookup away from a nucleotide reference, and a generic message would bury
    that."""
    patent_flag: str = "0"
    caveat: str = ""
    """Something the curator must decide or must not assume. Appended to
    `notes`, where it will be read."""

    # ----------------------------------------------------------------
    # The measured-occurrence record. Required for anything on the peptide
    # route; `build_peptide` refuses a part that has none. This is what
    # replaced MIN_PEPTIDE_AA, and the six fields are separate on purpose: a
    # count, a count of how many of them a human read, and a count of how many
    # of those were wrong are three different claims, and collapsing them is
    # how "nobody looked" comes to read as "nothing was wrong".

    occurrence_corpus: str = ""
    """What was searched, named precisely enough to be re-run.

    Including the gate it was counted under. Without that the number is
    unfalsifiable the first time anything changes: the annotator scans all six
    frames of the doubled text, most of which is outside any ORF, so any future
    diagnostic that reports pre-gate hits produces numbers far larger than
    these with nothing marking them stale."""

    occurrences: int = -1
    """Exact in-frame occurrences inside an ORF with at least PARTNER_MIN
    residues of partner -- i.e. only where the shipped fusion gate would REPORT
    the part. -1 means no measurement exists, which is not the same as zero and
    is why the default is not 0."""

    occurrence_files: str = ""
    """How the occurrences were distributed, e.g. "8 of 73 files (11.0%)".
    A count concentrated in one file is a different fact from the same count
    spread over the corpus."""

    adjudicated: int = -1
    """How many of `occurrences` a human actually read. The gate requires this
    to equal `occurrences`: a part is held because the reading has not been
    done, not because anyone has shown it noisy."""

    spurious: int = -1
    """How many of the adjudicated occurrences were NOT this part. Must be 0 to
    ship. -1 means nothing was adjudicated, so the question is open."""

    occurrence_note: str = ""
    """What the reading found, in prose, for the curator. Goes into `notes`."""


# The population every occurrence count below was taken over, named so it can be
# re-run. Named in the row's notes too, because a count without its denominator
# and its gate is not a measurement.
#
# ATG-to-stop under the standard code is NARROWER than the annotator's shipped
# default, which is table 11 and admits seven initiators. Stated rather than
# smoothed over: the corpus therefore under-counts slightly relative to what the
# tool would report, in the direction that makes a shipped row look better than
# it is. It does not change any verdict here -- the only two parts that occurred
# at all are one that ships on adjudication and one that is held -- but the next
# person to re-run this should widen the ORF caller before comparing.
CORPUS = (
    "73 plasmid and contig files from the PI's own machine, 17,061,931 residues "
    "of ATG-to-stop ORFs of >= 25 aa on both strands under the standard code; an "
    "occurrence is an exact in-frame match with >= 20 residues of the ORF outside "
    "the part, i.e. only where the shipped fusion gate would report it "
    "(K_PROTEIN=5, ORF_MIN_AA=25, PARTNER_MIN=20, exact and whole). "
    "Measured 2026-07-28."
)


def measured(occurrences: int, files: str, adjudicated: int, spurious: int,
             note: str = "") -> dict:
    """The six occurrence fields as keyword arguments.

    One line in a Part literal rather than six, and — the reason it is a
    function and not a tuple — positional order that a reader can check against
    a signature instead of against a comment.
    """
    return {
        "occurrence_corpus": CORPUS,
        "occurrences": occurrences,
        "occurrence_files": files,
        "adjudicated": adjudicated,
        "spurious": spurious,
        "occurrence_note": note,
    }


# Seventeen of the twenty peptide-route parts. Zero occurrences passes the gate
# vacuously and correctly: nothing to read, nothing spurious.
NONE_FOUND = measured(
    0, "0 of 73 files (0.0%)", 0, 0,
    "Did not occur anywhere in the corpus, so there was nothing to adjudicate.",
)


# ORDER IS IDENTITY. `ordinal` is this list's index, and a PLF id is a permanent
# name. Append; never insert, never reorder, never delete a line — retire a part
# by leaving it here and giving it a caveat.
PARTS: tuple[Part, ...] = (
    Part(
        name="FLAG tag",
        aliases=("FLAG", "FLAG epitope", "DYKDDDDK tag"),
        aa="DYKDDDDK",
        **NONE_FOUND,
        pdb_entity="8RMO_1",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="DOI:10.1038/nbt1088-1204 (1988, Bio/Technology 6:1204-1210)",
        citation="Hopp TP, Prickett KS, Price VL, Libby RT, March CJ, Cerretti DP, "
                 "Urdal DL, Conlon PJ (1988) A short polypeptide marker sequence useful "
                 "for recombinant protein identification and purification. "
                 "Bio/Technology 6:1204-1210.",
        description="Eight-residue hydrophilic epitope designed, rather than borrowed from "
                    "a natural protein, to be antigenic, surface-exposed on a fusion "
                    "partner, and removable by enterokinase - its last five residues are "
                    "the enterokinase recognition site, so the tag cleaves itself off. "
                    "The most widely used epitope tag in molecular biology.",
        witness="PDB 8RMO polymer entity 1, pdbx_description 'FLAG-tag', "
                "sequence DYKDDDDK (the entire entity); corroborated by 6U0O entity 3.",
        patent_flag="1",
        caveat="PATENT/TRADEMARK FLAG (not a determination): FLAG is a Sigma-Aldrich "
               "brand. Trademark status was not verified and CC BY 4.0 licenses no "
               "trademark rights (SOURCING.md Risk 6). Counsel must clear the NAME "
               "before it is used as a row label in a shipped product; the eight "
               "residues are not the exposure.",
    ),
    Part(
        name="3xFLAG tag",
        aliases=("3XFLAG", "triple FLAG", "3x FLAG epitope"),
        aa="DYKDHDGDYKDHDIDYKDDDDK",
        **NONE_FOUND,
        pdb_entity="21VV_8",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:10769759 (2000, BioTechniques 28:789-793)",
        citation="Hernan R, Heuermann K, Brizzard B (2000) Multiple epitope tagging of "
                 "expressed proteins for enhanced detection. BioTechniques 28:789-793.",
        description="Tandem array of three FLAG-like epitopes, the first two carrying "
                    "substitutions that break the enterokinase site so only the third and "
                    "final unit is cleavable. Detection sensitivity is roughly an order of "
                    "magnitude better than a single FLAG, which is the reason it exists.",
        witness="RCSB seqmotif search for the exact 22-mer returns 320 polymer entities; "
                "e.g. PDB 21VV entity 8 (SMARCA4 isoform 2) carries it at residue 1690.",
        patent_flag="1",
        caveat="TWO THINGS THE CURATOR MUST WEIGH. (1) Same trademark position as FLAG. "
               "(2) The sequence-to-paper link rests on usage, not on a quoted sentence: "
               "the Hernan 2000 abstract establishes the 3xFLAG concept but does not "
               "print the residue string, and the full text was not obtained. The 22-mer "
               "is verified by 320 independent depositions and by nothing else.",
    ),
    Part(
        name="HA tag",
        aliases=("HA", "haemagglutinin tag", "influenza HA epitope", "YPYDVPDYA"),
        aa="YPYDVPDYA",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:6204768 (1984, Cell 37:767-778) - the "
                          "nine-residue immunodominant determinant of influenza A HA1",
        citation="Wilson IA, Niman HL, Houghten RA, Cherenson AR, Connolly ML, Lerner RA "
                 "(1984) The structure of an antigenic determinant in a protein. "
                 "Cell 37:767-778. Adopted as a general-purpose tag by Field J, Nikawa J, "
                 "Broek D, MacDonald B, Rodgers L, Wilson IA, Lerner RA, Wigler M (1988) "
                 "Mol Cell Biol 8:2159-2165.",
        description="Nine-residue epitope from a surface loop of the HA1 subunit of "
                    "influenza A haemagglutinin. Wilson and colleagues raised antibodies "
                    "against overlapping peptides spanning the protein and found that the "
                    "ones which also recognised the folded protein converged on this "
                    "single nine-residue stretch; Field and colleagues then appended it to "
                    "an unrelated yeast protein to purify a complex, which is the move "
                    "that turned an antigenic determinant into an epitope tag.",
        witness="UniProt P03438 (influenza A A/X-31 H3N2 haemagglutinin, 566 aa) carries "
                "the 9-mer at precursor residues 114-122; with the annotated 1-16 signal "
                "peptide that is HA1 98-106, the classical numbering, which PDB 1EO8 "
                "entity 1 reproduces independently on its deposited HA1 chain.",
        parent_uniprot="P03438",
    ),
    Part(
        name="Myc tag",
        aliases=("myc", "c-Myc epitope", "9E10 epitope", "EQKLISEEDL"),
        aa="EQKLISEEDL",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:3915782 (1985, Mol Cell Biol 5:3610-3616) - "
                          "the epitope of monoclonal 9E10",
        citation="Evan GI, Lewis GK, Ramsay G, Bishop JM (1985) Isolation of monoclonal "
                 "antibodies specific for human c-myc proto-oncogene product. "
                 "Mol Cell Biol 5:3610-3616.",
        description="Ten-residue epitope from the C-terminal region of human c-Myc, "
                    "defined by the monoclonal antibody 9E10 that Evan and colleagues "
                    "raised against a bacterially expressed fragment of the oncoprotein. "
                    "Usable at either terminus, and small enough that it rarely perturbs "
                    "folding, which is why it outlived its origins as a c-Myc reagent.",
        witness="PDB 2OR9 entity 3, pdbx_description 'synthetic epitope peptide of 9E10', "
                "sequence EQKLISEEDLN - the tag is its first ten residues. UniProt P01106 "
                "(MYC_HUMAN, 454 aa) carries it at residues 425-434; the frequently "
                "quoted 410-419 is older numbering and does not match the current entry.",
        parent_uniprot="P01106",
    ),
    Part(
        name="Polyhistidine tag",
        aliases=("His tag", "His6", "6xHis", "His8", "His10", "10xHis",
                 "hexahistidine", "polyHis"),
        aa="HHHHHH",
        **measured(
            8, "8 of 73 files (11.0%)", 8, 0,
            "Eight occurrences, all eight read, none of them chance. Every one is "
            "C-terminal at exactly -0 residues from the stop, behind a GG linker, in "
            "files whose names say the construct carries a His tag; two distinct "
            "constructs, whose ORFs pl reports as 258 aa and 3796 aa under table 11 "
            "(the shorter one opens on an ATT initiator 98 codons upstream of its first "
            "ATG, so an ATG-only reading of it gives 160). So this row has zero measured "
            "false positives and eight true positives the shipped tool could not find - "
            "which is the opposite of what its length, and the 20^-L model, predicted.",
        ),
        pdb_entity="1KTR_2",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="DOI:10.1038/nbt1188-1321 (1988, "
                          "Bio/Technology 6:1321-1325)",
        citation="Hochuli E, Bannwarth W, Doebeli H, Gentz R, Stueber D (1988) Genetic "
                 "approach to facilitate purification of recombinant proteins with a "
                 "novel metal chelate adsorbent. Bio/Technology 6:1321-1325.",
        description="A run of histidines appended to a recombinant protein so that it can "
                    "be captured on immobilised nickel or cobalt and eluted with imidazole "
                    "or low pH. The smallest useful affinity tag there is, and the only "
                    "one that works under denaturing conditions, which is why it dominates "
                    "structural biology.",
        witness="PDB 1KTR entity 2, pdbx_description 'Oligohistidine peptide Antigen', "
                "sequence HHHHHH (the entire entity). Longer runs verified the same way: "
                "6,783 entities for HHHHHHHH, 174 for HHHHHHHHHH.",
        caveat="DESIGN DECISION, deliberately one row and not three: His6, His8 and His10 "
               "are the same feature at different run lengths, no paper stipulates 8 or "
               "10, and three rows would all match the same locus and compete in the "
               "annotator's output. "
               "WHAT THAT COSTS ON A LONGER TRACT, and it is visible on a map: this "
               "record means SIX histidines and its extent is the record's length, not "
               "the tract's. A tract of N histidines contains N-5 overlapping exact "
               "matches, and the annotator reports the leftmost of each overlapping "
               "group - so a His8 is drawn as an 18 bp Polyhistidine tag inside a 24 bp "
               "run, at identity 1.000 and coverage 1.000, a wrong extent wearing a "
               "perfect score, and a His12 is drawn as the two disjoint His6 it really "
               "contains. Measured over tract lengths 6 to 14 in the annotator's own test "
               "suite; before that collapse was written, His10 through His13 were drawn "
               "as TWO OVERLAPPING boxes and His14 as three, which is the failure that "
               "put this paragraph here. All eight measured corpus occurrences are "
               "exactly six residues, so the corpus does not exercise any of it; the "
               "witness above records 6,783 PDB entities carrying HHHHHHHH and 174 "
               "carrying HHHHHHHHHH, so it is not rare. Extending the run greedily and "
               "reporting the length observed is the fix, and it is a matcher change with "
               "its own tests, not a table change.",
    ),
    Part(
        name="V5 tag",
        aliases=("V5", "V5 epitope", "SV5 P/V epitope", "GKPIPNPLLGLDST"),
        aa="GKPIPNPLLGLDST",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:1713260 (1991, J Gen Virol 72:1551-1557) - "
                          "the epitope shared by the PIV5 P and V proteins",
        citation="Southern JA, Young DF, Heaney F, Baumgaertner WK, Randall RE (1991) "
                 "Identification of an epitope on the P and V proteins of simian virus 5 "
                 "that distinguishes between two isolates with different biological "
                 "characteristics. J Gen Virol 72(Pt 7):1551-1557.",
        description="Fourteen-residue epitope from parainfluenza virus 5, named for the V "
                    "protein but not unique to it: P and V are translated from the same "
                    "gene by RNA editing and share an N-terminal domain, so both carry the "
                    "epitope at the same position. Southern and colleagues mapped it while "
                    "trying to tell two virus isolates apart; its value as a tag is that "
                    "the antibody works well on Western blots and in "
                    "immunofluorescence.",
        witness="PDB 8SKJ entity 2, pdbx_description 'V5 Epitope Tag Peptide', sequence "
                "GKPIPNPLLGLDST (the entire entity). UniProt P11208 (PHOSP_PIV5) and "
                "P11207 (V_PIV5) both carry it at residues 95-108, which is the paper's "
                "shared-epitope claim checked rather than assumed.",
        parent_uniprot="P11208",
    ),
    Part(
        name="Strep-tag",
        aliases=("Strep-tag I", "AWRHPQFGG", "original Strep-tag"),
        aa="AWRHPQFGG",
        **NONE_FOUND,
        pdb_entity="1RST_2",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:8636976 (1996, J Mol Biol 255:753-766)",
        citation="Schmidt TGM, Koepke J, Frank R, Skerra A (1996) Molecular interaction "
                 "between the Strep-tag affinity peptide and its cognate target, "
                 "streptavidin. J Mol Biol 255:753-766.",
        description="Nine-residue peptide selected from a library for binding streptavidin "
                    "in the biotin pocket, so the fusion can be eluted competitively with "
                    "desthiobiotin under entirely native conditions. The crystal structure "
                    "showed the terminal Gly-Gly carboxylate salt-bridging to streptavidin "
                    "Arg84, which is why this version only works as a C-terminal fusion.",
        witness="PDB 1RST entity 2, pdbx_description 'STREP-TAG PEPTIDE', sequence "
                "AWRHPQFGG (the entire entity). The paper's abstract prints the nine "
                "residues verbatim and states the C-terminal restriction and its cause.",
        patent_flag="1",
        caveat="TRADEMARK FLAG (not a patent determination): Strep-tag is an IBA "
               "Lifesciences registered mark - the Europe PMC abstract text for the "
               "Twin-Strep paper renders 'Strep-tag(R)II' and 'Strep-Tactin(R)' with "
               "registration symbols. Counsel question, and it attaches to the name.",
    ),
    Part(
        name="Strep-tag II",
        aliases=("StrepII", "Strep II", "WSHPQFEK"),
        aa="WSHPQFEK",
        **NONE_FOUND,
        pdb_entity="1KL3_2",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:8636976 (1996, J Mol Biol 255:753-766)",
        citation="Schmidt TGM, Koepke J, Frank R, Skerra A (1996) Molecular interaction "
                 "between the Strep-tag affinity peptide and its cognate target, "
                 "streptavidin. J Mol Biol 255:753-766.",
        description="Eight-residue redesign of Strep-tag, screened on a peptide spot array "
                    "specifically to remove the free-carboxylate requirement of the "
                    "original. It binds streptavidin from either terminus and from an "
                    "internal position, which is why it, and not Strep-tag I, is what "
                    "modern vectors carry.",
        witness="PDB 1KL3 entity 2, 'strep-tag II peptide'; also 6QW4 and 6QBB entity 2. "
                "RCSB seqmotif returns 4,446 entities for WSHPQFEK. The 1996 abstract is "
                "where the variant is introduced.",
        patent_flag="1",
        caveat="TRADEMARK FLAG: IBA Lifesciences registered mark. See the Strep-tag row.",
    ),
    Part(
        name="Twin-Strep-tag",
        aliases=("Twin Strep", "2xStrep-tag II", "One-STrEP-tag"),
        aa="WSHPQFEKGGGSGGGSGGSAWSHPQFEK",
        **NONE_FOUND,
        pdb_entity="6SOS_2",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:24012791 (2013, "
                          "Protein Expr Purif 92:54-61)",
        citation="Schmidt TGM, Batz L, Bonet L, Carl U, Holzapfel G, Kiem K, "
                 "Matulewicz K, Niermeier D, Schuchardt I, Stanar K (2013) Development of "
                 "the Twin-Strep-tag and its application for purification of recombinant "
                 "proteins from cell culture supernatants. Protein Expr Purif 92:54-61.",
        description="Two Strep-tag II units joined by a defined twelve-residue "
                    "glycine-serine spacer, so that one fusion protein engages two "
                    "streptavidin sites at once. The avidity gain is what makes it usable "
                    "for pulling a protein out of dilute cell-culture supernatant, which a "
                    "single Strep-tag II cannot do.",
        witness="PDB 6SOS entity 2, pdbx_description 'Twin-Strep-tag peptide', a 32-residue "
                "sequence containing exactly this 28-mer. RCSB seqmotif returns 865 "
                "entities for the 28-mer.",
        patent_flag="1",
        caveat="TRADEMARK FLAG: IBA registered mark - the Europe PMC title itself renders "
               "'Twin-Strep-tag(R)' - and IBA licenses the system commercially.",
    ),
    Part(
        name="S-tag",
        aliases=("S-peptide", "RNase S peptide", "KETAAAKFERQHMDS"),
        aa="KETAAAKFERQHMDS",
        **NONE_FOUND,
        pdb_entity="1A2W_1",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        # THE 1959 PAPER DOES NOT STIPULATE THIS BOUNDARY, and citing it as
        # though it did was this row's actual error. Subtilisin cuts RNase A
        # once, between residues 20 and 21; its products are S-peptide
        # (residues 1-20) and S-protein (21-124). Three independently fetched
        # abstracts say so -- PMID:8453373, PMID:3076449, PMID:6260244 -- and
        # RCSB polymer entity 1Z3M_2, 'Ribonuclease pancreatic, S-protein', is
        # 104 residues, i.e. 21-124. The FIFTEEN-residue tag is S15, defined by
        # Kim & Raines, whose abstract prints "the first 15 residues of
        # S-peptide (S15)" and shows it still binds S-protein. So the citation
        # is theirs: the paper that made the fragment is not the paper that
        # chose this boundary within it.
        boundary_evidence="PMID:8453373 (Kim & Raines 1993, Protein Sci 2:348-356) - S15, "
                          "the first fifteen residues of the subtilisin-generated S-peptide "
                          "of bovine RNase A",
        citation="Kim JS, Raines RT (1993) Ribonuclease S-peptide as a carrier in fusion "
                 "proteins. Protein Sci 2:348-356. Origin of the parent fragment: Richards "
                 "FM, Vithayathil PJ (1959) The preparation of subtilisin-modified "
                 "ribonuclease and the separation of the peptide and protein components. "
                 "J Biol Chem 234:1459-1465.",
        description="Subtilisin cuts bovine pancreatic ribonuclease A once, between residues "
                    "20 and 21, giving S-peptide and S-protein; neither is active alone, but "
                    "they reassociate and restore activity. This tag is S15, the first "
                    "fifteen residues of that S-peptide, which Kim and Raines showed still "
                    "binds S-protein - so a fusion carrying it can be both captured and "
                    "assayed. Among the oldest fragment-complementation systems in use.",
        witness="PDB 1A2W entity 1, 'RIBONUCLEASE A', whose 124-residue chain begins "
                "KETAAAKFERQHMDSSTSAASSSNYCNQMM, so these fifteen residues are that chain's "
                "first fifteen. That confirms the RESIDUES and cannot confirm the BOUNDARY: "
                "taking the first fifteen of a 124-residue chain is a choice, and the choice "
                "is Kim & Raines's.",
        no_gene="parent is bovine pancreatic RNase A, but no accession for it was "
             "established from a fetched record; one lookup away",
        caveat="NO NUCLEOTIDE REFERENCE, and one lookup away from having one: the parent "
               "is bovine pancreatic RNase A, but no UniProt accession for it was "
               "established from a fetched record in the session that produced this table "
               "- only the PDB entity above. Sourcing codons means first looking the parent "
               "up, not recalling it.",
    ),
    Part(
        name="AviTag",
        aliases=("Avi tag", "BAP", "biotin acceptor peptide", "BirA substrate peptide",
                 "GLNDIFEAQKIEWHE"),
        aa="GLNDIFEAQKIEWHE",
        **NONE_FOUND,
        pdb_entity="11ZV_1",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:10211839 (1999, Protein Sci 8:921-929); "
                          "peptide selected in PMID:7764094 (Schatz 1993, "
                          "Bio/Technology 11:1138-1143)",
        citation="Beckett D, Kovaleva E, Schatz PJ (1999) A minimal peptide substrate in "
                 "biotin holoenzyme synthetase-catalyzed biotinylation. "
                 "Protein Sci 8:921-929. Origin of the peptide: Schatz PJ (1993) "
                 "Bio/Technology 11:1138-1143.",
        # NEITHER CITED PAPER STIPULATES FIFTEEN RESIDUES, and the old
        # description's "fifteen-residue peptide selected from a library" ran
        # two different lengths together as though they were one. Schatz 1993's
        # own title says "a 13 residue consensus peptide specifies
        # biotinylation"; Beckett 1999's abstract starts from "a 23-residue
        # peptide previously identified by combinatorial methods" and reports
        # "identification of a 14-residue peptide as the minimum required
        # sequence". The fifteen-residue form is what the field and the vendor
        # use, and this row's own witness already admits that the NAME rests on
        # the papers rather than on a database label. Its LENGTH deserves the
        # same honesty.
        description="Acceptor peptide for E. coli biotin ligase BirA, which attaches biotin "
                    "to the single lysine at position ten. That gives site-specific, "
                    "stoichiometric, enzymatically installed biotin on a recombinant "
                    "protein, where chemical biotinylation hits every surface lysine at "
                    "random. Schatz defines a thirteen-residue consensus by library "
                    "selection and Beckett a fourteen-residue minimum substrate; these "
                    "fifteen residues are the form in common use, and no cited paper "
                    "stipulates that extent.",
        witness="RCSB seqmotif returns 537 entities for the 15-mer; PDB 11ZV entities 1 "
                "and 2 both carry it in the canonical GGS-flanked cassette. Schatz 1993's "
                "abstract confirms the library-selection provenance of the motif. NOTE: no "
                "PDB entity is *named* AviTag, so the name-to-sequence link rests on the "
                "two papers, not on a database label - and neither paper stipulates FIFTEEN "
                "residues either: Schatz gives 13, Beckett 14. The extent is conventional, "
                "not stipulated, and this row says so rather than implying otherwise.",
        patent_flag="1",
        caveat="PATENT/TRADEMARK FLAG (not a determination): AviTag is an Avidity LLC "
               "brand and site-specific BirA biotinylation has been patented. Not assessed "
               "- no patent database was searched.",
    ),
    Part(
        name="SBP-tag",
        aliases=("SBP", "streptavidin-binding peptide"),
        aa="MDEKTTGWRGGHVVEGLAGELEQLRARLEHHPQGQREP",
        **NONE_FOUND,
        pdb_entity="4JO6_2",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:11722181 (2001, "
                          "Protein Expr Purif 23:440-446)",
        citation="Keefe AD, Wilson DS, Seelig B, Szostak JW (2001) One-step purification "
                 "of recombinant proteins using a nanomolar-affinity streptavidin-binding "
                 "peptide, the SBP-Tag. Protein Expr Purif 23:440-446.",
        # THE COMPARISON THAT USED TO BE HERE WAS SOURCED FROM NOTHING. It read
        # "about a hundred-fold more tightly than Strep-tag II", and Keefe's
        # abstract -- this row's only source -- gives one number and no
        # comparison: "binds to streptavidin with an equilibrium dissociation
        # constant of 2.5 nM", with Strep-tag II not mentioned at all. Neither
        # PMID:8636976 (the Strep-tag II row's source) nor PMID:9415448 prints a
        # constant that would let the ratio be computed. So the ratio was
        # recalled, which is the one thing this table may never do. Say the
        # number the cited paper prints, and nothing else.
        description="Thirty-eight-residue peptide isolated by mRNA display, which binds "
                    "streptavidin with an equilibrium dissociation constant of 2.5 nM and "
                    "elutes with free biotin under native conditions. Long enough to be a "
                    "real domain rather than a linear epitope, which is the cost of the "
                    "affinity.",
        witness="PDB 4JO6 entity 2, pdbx_description 'SBP-Tag', 38-residue sequence, the "
                "entire entity. The length agrees with the paper's own abstract.",
        caveat="NO NUCLEOTIDE REFERENCE, and there can never be one: selected by mRNA "
               "display, so there is no natural gene to read codons out of. That is the "
               "clearest case for the peptide route existing at all. Patent status not "
               "assessed (Szostak lab).",
    ),
    Part(
        name="Calmodulin-binding peptide",
        aliases=("CBP", "CBP tag", "MLCK M13 peptide"),
        aa="KRRWKKNFIAVSAANRFKKISSSGAL",
        **NONE_FOUND,
        pdb_entity="2BBM_2",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:1318232 (1992, "
                          "FEBS Lett 302:274-278) - the third unit of the kfc cassette",
        citation="Stofko-Hahn RE, Carr DW, Scott JD (1992) A single step purification for "
                 "recombinant proteins. Characterization of a microtubule associated "
                 "protein (MAP 2) fragment which associates with the type II "
                 "cAMP-dependent protein kinase. FEBS Lett 302:274-278.",
        # PHRASED THE WAY IT IS FOR A MEASURED REASON, not for style. The first
        # draft read "The calmodulin-binding helix of skeletal-muscle myosin
        # light-chain kinase. It binds calmodulin only in the presence of
        # calcium..." and taint_gate.py FAILED it: after stopword removal the
        # sentence break vanished and it shared an eight-token contiguous run,
        # "skeletal muscle myosin light chain kinase binds calmodulin", with
        # pLannotate's snapgene.csv. Nothing was copied -- that is the
        # vocabulary of the subject arriving in the obvious order -- but the
        # rule is mechanical on purpose, and the project's answer to a tripped
        # gate is to rewrite from the primary source rather than to argue with
        # the measurement. The enzyme's own name is an irreducible four-token
        # run and is left as such.
        description="Twenty-six residues from the calcium-dependent regulatory helix of "
                    "MLCK, the myosin light-chain kinase found in skeletal muscle. "
                    "Calmodulin engages that helix only while calcium is bound, so a "
                    "fusion carrying the peptide is captured on calmodulin resin and then "
                    "eluted simply by chelating the calcium away with EGTA. An elution "
                    "that mild leaves assembled complexes together, which is the property "
                    "that made this peptide the second affinity handle of the tandem "
                    "purification tag.",
        witness="PDB 2BBM entity 2, 'MYOSIN LIGHT CHAIN KINASE', a 26-residue entity that "
                "is an exact match to the tag. The 1992 abstract describes the three-unit "
                "kfc cassette this peptide terminates.",
        no_gene="parent is myosin light-chain kinase, but no accession for it was "
             "established from a fetched record; one lookup away",
        caveat="NO NUCLEOTIDE REFERENCE, same shape as S-tag. The parent is myosin "
               "light-chain kinase, but the session that produced this table established "
               "only the PDB entity, not a UniProt accession, and picking one from recall "
               "is exactly what this build forbids.",
    ),
    Part(
        name="ALFA-tag",
        aliases=("ALFA", "SRLEEELRRRLTE"),
        aa="SRLEEELRRRLTE",
        **NONE_FOUND,
        pdb_entity="6I2G_2",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:31562305 (2019, Nat Commun 10:4403), "
                          "Results paragraph 1, which prints the minimal tag",
        citation="Goetzke H, Kilisch M, Martinez-Carranza M, Sograte-Idrissi S, Rajavel A, "
                 "Schlichthaerle T, Engels N, Jungmann R, Stenmark P, Opazo F, Frey S "
                 "(2019) The ALFA-tag is a highly versatile tool for nanobody-based "
                 "bioscience applications. Nat Commun 10:4403.",
        description="Thirteen-residue de novo epitope designed to fold as a short alpha "
                    "helix, paired with a nanobody raised against it. Because the epitope "
                    "is synthetic it is absent from every proteome, so the reagent has no "
                    "endogenous background - the property a natural-protein epitope such "
                    "as Myc or HA cannot offer.",
        witness="Open-access full text, Results paragraph 1, verbatim: 'The sequence of "
                "the minimal ALFA-tag (SRLEEELRRRLTE; Fig. 1a) is inspired by an "
                "artificial peptide ...'. PDB 6I2G entity 2 is the tag flanked by proline "
                "on each side.",
        patent_flag="1",
        caveat="COMMERCIAL FLAG (not a determination): ALFA-tag and NbALFA are "
               "commercialised by NanoTag Biotechnologies. Patent status not assessed.",
    ),
    Part(
        name="Protein C tag",
        aliases=("HPC4 tag", "HPC4", "protein C activation peptide epitope",
                 "EDQVDPRLIDGK"),
        aa="EDQVDPRLIDGK",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:1283093 (1992, "
                          "Protein Expr Purif 3:453-460); antibody from PMID:2447082 "
                          "(1988, J Biol Chem 263:826-832)",
        citation="Rezaie AR, Fiore MM, Neuenschwander PF, Esmon CT, Morrissey JH (1992) "
                 "Expression and purification of a soluble tissue factor fusion protein "
                 "with an epitope for an unusual calcium-dependent antibody. "
                 "Protein Expr Purif 3:453-460.",
        description="Twelve residues from the activation-peptide region of human protein "
                    "C, recognised by the monoclonal HPC4 only when calcium is bound. That "
                    "calcium dependence is the whole point: the fusion is eluted by "
                    "chelation rather than by low pH or a competing peptide, so it comes "
                    "off the column folded.",
        witness="PDB 4DT7 entity 3, from a structure titled for the protein C activation "
                "domain, carries Q + this 12-mer + MTRRGDS. UniProt P04070 (PROC_HUMAN) "
                "carries it at residues 205-216.",
        parent_uniprot="P04070",
    ),
    Part(
        name="TEV protease cleavage site",
        aliases=("TEV site", "ENLYFQG", "ENLYFQ/G", "TEV recognition sequence"),
        aa="ENLYFQG",
        **NONE_FOUND,
        pdb_entity="10GW_1",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:3285343 (Carrington & Dougherty 1988, "
                          "PNAS 85:3391-3395) - the E-X-X-Y-X-Q/(S or G) consensus; the "
                          "scissile bond is between Q6 and G7",
        citation="Carrington JC, Dougherty WG (1988) A viral cleavage site cassette: "
                 "identification of amino acid sequences required for tobacco etch virus "
                 "polyprotein processing. Proc Natl Acad Sci USA 85:3391-3395.",
        description="The recognition sequence of the tobacco etch virus NIa protease, "
                    "which cuts between the glutamine and the residue after it. It is the "
                    "standard way to remove an affinity tag because the protease is "
                    "unusually specific, works in the cold, and can itself be supplied as "
                    "a His-tagged enzyme that is removed on the same resin afterwards.",
        witness="Carrington 1988 abstract, verbatim: 'All known or predicted cleavage "
                "sites in the TEV polyprotein are flanked by the conserved sequence motif "
                "Glu-Xaa-Xaa-Tyr-Xaa-Gln-Ser or Gly, with the scissile bond located "
                "between the Gln-Ser or Gly dipeptide.' PDB 10GW entity 1 carries the "
                "His6-TEV cassette.",
        no_gene="parent is the TEV polyprotein, but no accession for it was established "
             "from a fetched record; one lookup away",
        caveat="SHIPS ON A MEASUREMENT, having been held twice on other grounds. It was "
               "held first for a missing parent - which the peptide route does not need - "
               "and then by a flat 8-residue floor. Seven residues is exactly three seed "
               "windows, so it chains today with no margin at all: raise Config::min_seeds "
               "to 4 and this row moves to the annotator's exact-scan route by itself. "
               "That migration is the point of routing on indexed words rather than on "
               "length. Separately, no UniProt accession for the TEV polyprotein was ever "
               "established from a fetched record, so it still has no nucleotide route.",
    ),
    Part(
        name="TEV protease cleavage site (Ser variant)",
        aliases=("ENLYFQS", "ENLYFQ/S", "TEV site S variant"),
        aa="ENLYFQS",
        **NONE_FOUND,
        pdb_entity="10HA_1",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:3285343 (Carrington & Dougherty 1988, PNAS 85:3391-3395); "
                          "scissile bond between Q and S",
        citation="Carrington JC, Dougherty WG (1988) A viral cleavage site cassette: "
                 "identification of amino acid sequences required for tobacco etch virus "
                 "polyprotein processing. Proc Natl Acad Sci USA 85:3391-3395.",
        description="The serine form of the TEV site. It is a separate record rather than "
                    "a wildcard because the paper names Ser and Gly as the two alternative "
                    "P1' residues and nothing else, and a wildcard at P1' would break the "
                    "exact-match rule that a seven-residue feature depends on.",
        witness="PDB 10HA entity 1 (pyrimidodiazepine synthase) carries ...ENLYFQS + "
                "GSHHHHHH. RCSB seqmotif returns 3,891 entities.",
        no_gene="same as the Gly variant: the TEV polyprotein accession was not "
             "established from a fetched record",
        caveat="SHIPS: same position as the Gly variant - seven residues, zero occurrences "
               "in the corpus, and the same no-margin three seed windows.",
    ),
    Part(
        name="HRV 3C protease cleavage site",
        aliases=("PreScission site", "3C site", "LEVLFQGP", "LEVLFQ/GP"),
        aa="LEVLFQGP",
        **NONE_FOUND,
        pdb_entity="10HM_1",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:2160953 (1990, "
                          "J Biol Chem 265:9062-9065) for the natural consensus; the "
                          "vector octamer is a P4 variant of it and is published by no "
                          "paper located in this work",
        citation="Cordingley MG, Callahan PL, Sardana VV, Garsky VM, Colonno RJ (1990) "
                 "Substrate requirements of human rhinovirus 3C protease for peptide "
                 "cleavage in vitro. J Biol Chem 265:9062-9065.",
        description="The site cut by human rhinovirus 3C protease, between the glutamine "
                    "and the glycine, leaving Gly-Pro on the downstream product. Its "
                    "attraction over TEV is that the protease is efficient at 4 degrees, "
                    "so a tag can be removed on the column during a cold purification.",
        witness="RCSB seqmotif for LEVLFQGP returns 3,518 entities, e.g. PDB 10HM entity 1 "
                "carrying MHHHHHHSSG + the octamer.",
        no_gene="engineered P4 variant of the natural LETLFQ/GP junction, so no gene "
             "encodes this octamer",
        caveat="DISCREPANCY THE CURATOR MUST SEE, found by checking rather than assuming. "
               "The vector sequence is LEVLFQ/GP, but the natural HRV14 2C/3A junction is "
               "LETLFQ/GP: UniProt P03303 carries LETLFQGP at 1424-1431 and annotates the "
               "3C cleavage there, and Cordingley's abstract names ETLFQ/GP. LEVLFQ/GP is "
               "a P4 Thr-to-Val engineered variant and no paper located here publishes it. "
               "It is also why this row carries a PEPTIDE and no nucleotides: an "
               "engineered variant has no natural gene to take codons from, which is "
               "precisely the shape the peptide route exists for. Do not let this row "
               "imply Cordingley published this octamer.",
    ),
    Part(
        name="Thrombin cleavage site",
        aliases=("thrombin site", "LVPRGS", "LVPR/GS"),
        aa="LVPRGS",
        **NONE_FOUND,
        pdb_entity="10EE_1",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:2863141 (Chang 1985, Eur J Biochem 151:217-224) for the "
                          "apolar P4-P1 requirement; scissile bond between R and G",
        citation="Chang JY (1985) Thrombin specificity. Requirement for apolar amino acids "
                 "adjacent to the thrombin cleavage site of polypeptide substrate. "
                 "Eur J Biochem 151:217-224. Vector deployment: Smith DB, Johnson KS "
                 "(1988) Gene 67:31-40.",
        description="The hexamer thrombin cuts between arginine and glycine, and the "
                    "cleavage site carried by pGEX and by the pET-28 N-terminal cassette. "
                    "Chang established that thrombin needs apolar residues flanking the "
                    "recognised arginine, which is the requirement this hexamer satisfies.",
        witness="RCSB seqmotif returns 11,063 entities; PDB 10EE entity 1 carries the "
                "pET-28a cassette MGSSHHHHHHSSG + LVPRGS + HM.",
        no_gene="not a natural junction - checked absent from five human thrombin "
             "substrates - so no gene encodes it",
        caveat="WEAKEST ATTRIBUTION IN THIS TABLE, stated plainly. No paper was found that "
               "first publishes the exact hexamer LVPR/GS: Chang 1985 establishes the "
               "apolar requirement, and Smith & Johnson 1988 mention thrombin without "
               "printing residues. It is also NOT a natural junction - LVPR is absent from "
               "human prothrombin (P00734), fibrinogen alpha (P02671), factor XIII A "
               "(P00488), PAR1 (P25116) and factor V (P12259), all checked. Sequence "
               "verified; attribution unresolved; and with no natural parent there are no "
               "codons to take. THAT is what the curator has to weigh here, and it is the "
               "only thing left to weigh: the row was previously held on length as well, "
               "with a note calling LVPRGS 'the noisiest item this table would admit' at "
               "roughly one spurious hit per 2,300 random 5 kb plasmids. That estimate was "
               "20^-L arithmetic and the corpus disagrees with it - zero occurrences in "
               "17,061,931 ORF residues of real sequence, where the same model predicted "
               "0.27. Six residues is two seed windows, so this row reaches the annotator "
               "by the exact-scan route rather than by chaining.",
    ),
    Part(
        name="Factor Xa cleavage site",
        aliases=("factor Xa site", "IEGR", "Ile-Glu-Gly-Arg"),
        aa="IEGR",
        **measured(
            154, "12 of 73 files (16.4%)", 0, -1,
            "154 occurrences and not one of them read. That is why the row is held: not "
            "because anybody has shown the 154 are chance, but because the work has not "
            "been done. It ships the day somebody reads them.",
        ),
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:6330564 (Nagai & Thoegersen 1984, Nature 309:810-812); "
                          "scissile bond immediately after R, leaving no residual residues",
        citation="Nagai K, Thoegersen HC (1984) Generation of beta-globin by "
                 "sequence-specific proteolysis of a hybrid protein produced in "
                 "Escherichia coli. Nature 309:810-812.",
        description="Four residues cut by blood coagulation factor Xa immediately after "
                    "the arginine, which is what makes it valuable: it is one of the few "
                    "sites that leaves the downstream protein with its own native "
                    "N-terminus and no scar. Nagai and Thoegersen introduced it precisely "
                    "so that a fusion could yield authentic beta-globin.",
        witness="Nagai 1984 abstract, verbatim: 'we have inserted the sequence "
                "Ile-Glu-Gly-Arg between the 31 amino-terminal residues of lambda cII "
                "protein and Val 1 of human beta-glob[in]'. UniProt P00734 (prothrombin) "
                "carries IEGR at 311-314 with an annotated factor Xa cleavage site at "
                "314-315.",
        parent_uniprot="P00734",
        caveat="THE ONLY PART IN THIS TABLE STILL HELD, and the reason is now a "
               "measurement rather than a length. 154 in-frame occurrences across 73 real "
               "plasmids, in 12 of them (16.4%), every one of which would have been "
               "reported under the shipped fusion gate - against zero for the five parts "
               "that ship alongside it. Note what that does NOT claim: nobody has shown "
               "the 154 are chance. The gate is 'every occurrence read, none of them "
               "spurious', so this row is held because the reading has not been done, and "
               "it ships the day somebody does it. 'IEGR is noisy' is not a claim this "
               "evidence supports; 'IEGR has 154 unexamined hits' is. "
               "Separately and independently, four residues is twelve base pairs, below "
               "MIN_NT, so the clean parent P00734 cannot supply a nucleotide reference "
               "either; and four residues is below MIN_PART_AA, so the annotator would "
               "refuse it outright. Whoever adjudicates the 154 must move ORF_MIN_AA in "
               "crates/pl-features/src/annotate.rs as well, or index the SSGHIEGRHM-style "
               "cassette context instead.",
    ),
    Part(
        name="Enterokinase cleavage site",
        aliases=("enteropeptidase site", "DDDDK", "Asp4-Lys"),
        aa="DDDDK",
        **NONE_FOUND,
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:5570436 (1971, J Biol Chem 246:5031-5039); "
                          "scissile bond immediately after K, leaving no residual residues",
        citation="Maroux S, Baratti J, Desnuelle P (1971) Purification and specificity of "
                 "porcine enterokinase. J Biol Chem 246:5031-5039.",
        description="The physiological signal enteropeptidase reads to activate "
                    "trypsinogen, and, like factor Xa, a site that cuts after its own last "
                    "residue and so leaves no scar on the downstream protein.",
        witness="UniProt P00760 (TRY1_BOVIN) annotates the activation peptide at residues "
                "18-23 as VDDDDK, immediately followed by the mature chain beginning IVGG "
                "at 24 - the scissile bond read off the record rather than recalled.",
        parent_uniprot="P00760",
        caveat="SHIPS, and it is the shortest row in this table: five residues, which is "
               "exactly MIN_PART_AA. Fifteen base pairs is below MIN_NT, so the clean "
               "parent P00760 cannot supply a nucleotide reference - it supplies the "
               "verification instead, which is why this is the one row witnessed on a "
               "UniProt canonical rather than a wwPDB entity. Five residues is ONE seed "
               "window, so it reaches the annotator only by the exact-scan route. "
               "THE CONTAINMENT PROBLEM, unchanged by the measurement and not solved by "
               "it: DDDDK is the C-terminal half of FLAG and of 3xFLAG, so in a "
               "FLAG-tagged construct both records fire on one locus. Where both are "
               "reported the annotator resolves it correctly and keeps FLAG. Where it does "
               "NOT is an ORF of 25, 26 or 27 residues: DDDDK clears the fusion gate at 25 "
               "and FLAG only at 28, so such a construct is annotated 'Enterokinase "
               "cleavage site' and no FLAG. Declared in DECLARED_CONTAINMENT and read from "
               "the code, not from a run - and the corpus cannot settle it, because it "
               "contains no FLAG-tagged construct at all (FLAG measured zero occurrences), "
               "which is the one construct class where this row is known to misfire.",
    ),
    Part(
        name="P2A self-cleaving peptide",
        aliases=("P2A", "porcine teschovirus-1 2A", "PTV-1 2A"),
        aa="ATNFSLLKQAGDVEENPGP",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:21602908 (2011, PLoS ONE 6:e18556); the skip "
                          "occurs between the terminal G and P, so the P is the first "
                          "residue of the downstream protein",
        citation="Kim JH, Lee SR, Li LH, Park HJ, Park JH, Lee KY, Kim MK, Shin BA, "
                 "Choi SY (2011) High cleavage efficiency of a 2A peptide derived from "
                 "porcine teschovirus-1 in human cell lines, zebrafish and mice. "
                 "PLoS ONE 6:e18556.",
        description="Nineteen-residue 2A element from the porcine teschovirus-1 "
                    "polyprotein. Not a protease and not a cleavage: the ribosome forms "
                    "the last peptide bond only inefficiently and skips to the next codon, "
                    "releasing the upstream protein and continuing translation, so one "
                    "open reading frame yields two separate chains in a fixed ratio. Kim "
                    "and colleagues measured it as the most efficient of the commonly used "
                    "2A peptides across human cell lines, zebrafish and mice, which is why "
                    "it displaced the others in multicistronic vectors.",
        witness="UniProt Q99HP0 (teschovirus A1 polyprotein, 2204 aa, EMBL AF231767) "
                "carries the 19-mer at residues 947-965. PDB 8E99 entity 2 carries GSG + "
                "the 19-mer, which also shows the spacer.",
        parent_uniprot="Q99HP0",
        caveat="The GSG immediately upstream in most vectors is NOT part of 2A: Kim 2011's "
               "own figure legend says GSG 'were added to improve cleavage efficiency'. "
               "Annotating GSG as part of the element would extend every 2A feature by "
               "three residues against the paper that defines it.",
    ),
    Part(
        name="T2A self-cleaving peptide",
        aliases=("T2A", "Thosea asigna virus 2A", "TaV 2A"),
        aa="EGRGSLLTCGDVEENPGP",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:15064769 (2004, "
                          "Nat Biotechnol 22:589-594); skip between the terminal G and P",
        citation="Szymczak AL, Workman CJ, Wang Y, Vignali KM, Dilioglou S, Vanin EF, "
                 "Vignali DAA (2004) Correction of multi-gene deficiency in vivo using a "
                 "single 'self-cleaving' 2A peptide-based retroviral vector. "
                 "Nat Biotechnol 22:589-594.",
        description="Eighteen-residue 2A element from Thosea asigna virus, and the one "
                    "that made multi-gene vectors ordinary: Szymczak and colleagues used "
                    "2A peptides to express four proteins from a single retroviral reading "
                    "frame and reconstitute a multi-gene deficiency in vivo. Same "
                    "ribosomal-skip mechanism as P2A.",
        witness="UniProt Q9YK87 (Thosea asigna virus, 757 aa, EMBL AF062037) carries the "
                "18-mer at residues 139-156. RCSB seqmotif returns zero entities for it, "
                "so the viral record is the only witness - and it is a direct one.",
        parent_uniprot="Q9YK87",
        caveat="As with P2A, vector constructs usually carry an upstream GSG spacer that "
               "is not part of the element.",
    ),
    Part(
        name="E2A self-cleaving peptide",
        aliases=("E2A", "equine rhinitis A virus 2A", "ERAV 2A"),
        aa="QCTNYALLKLAGDVESNPGP",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:11297676 (2001, "
                          "J Gen Virol 82:1013-1025); skip between the terminal G and P",
        citation="Donnelly MLL, Luke G, Mehrotra A, Li X, Hughes LE, Gani D, Ryan MD "
                 "(2001) Analysis of the aphthovirus 2A/2B polyprotein 'cleavage' "
                 "mechanism indicates not a proteolytic reaction, but a novel "
                 "translational effect: a putative ribosomal 'skip'. "
                 "J Gen Virol 82:1013-1025.",
        description="Twenty-residue 2A element from equine rhinitis A virus. Donnelly and "
                    "colleagues used the aphthovirus 2A junction to show that what had "
                    "been described as autoproteolysis is not a proteolytic reaction at "
                    "all but a translational skip - the mechanism every element in this "
                    "family shares, and the reason 'self-cleaving' is in quotation marks "
                    "in the literature.",
        witness="UniProt Q66774 (equine rhinitis A virus polyprotein, 2248 aa, EMBL "
                "X96870) carries the 20-mer at residues 991-1010.",
        parent_uniprot="Q66774",
        caveat="STRAIN CAVEAT: a second ERAV entry, O39818 (EMBL L43052), reads CTNYSLLK "
               "at the same position. The commonly used E2A matches Q66774/X96870. "
               "Choosing an arbitrary ERAV accession would have planted one wrong residue.",
    ),
    Part(
        name="F2A self-cleaving peptide",
        aliases=("F2A", "FMDV 2A", "foot-and-mouth disease virus 2A"),
        aa="VKQTLNFDLLKLAGDVESNPGP",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:11297676 (2001, "
                          "J Gen Virol 82:1013-1025); skip between the terminal G and P",
        citation="Donnelly MLL, Luke G, Mehrotra A, Li X, Hughes LE, Gani D, Ryan MD "
                 "(2001) Analysis of the aphthovirus 2A/2B polyprotein 'cleavage' "
                 "mechanism indicates not a proteolytic reaction, but a novel "
                 "translational effect: a putative ribosomal 'skip'. "
                 "J Gen Virol 82:1013-1025.",
        description="The foot-and-mouth disease virus 2A element, the founding member of "
                    "the family and the one whose mechanism was worked out. Donnelly and "
                    "colleagues state the natural element is eighteen residues; the "
                    "twenty-two-residue form used in vectors extends it by three upstream "
                    "residues and by the downstream proline the skip leaves on the next "
                    "protein.",
        witness="UniProt P03305 (POLG_FMDVO, 2332 aa, EMBL X00871) annotates chain "
                "'Protein 2A' at residues 936-953, eighteen residues; the 22-mer occupies "
                "933-954. Donnelly 2001's abstract independently states the natural length "
                "is 18 aa.",
        parent_uniprot="P03305",
        caveat="BOUNDARY DECISION THE CURATOR OWNS: this row stores the 22-mer used in "
               "vectors, which is four residues longer than the chain UniProt annotates. "
               "Both extensions are deliberate and are stated here rather than silently "
               "included. If the curator prefers the annotated 18-mer, that is a different "
               "row, not an edit to this one.",
    ),
    Part(
        name="(GGGGS)3 flexible linker",
        aliases=("G4S linker", "(G4S)3", "15-residue scFv linker", "212 linker"),
        aa="GGGGSGGGGSGGGGS",
        **NONE_FOUND,
        pdb_entity="1DZB_1",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:3045807 (1988, PNAS 85:5879-5883), which "
                          "stipulates a 15-residue linker joining VH to VL",
        citation="Huston JS, Levinson D, Mudgett-Hunter M, Tai MS, Novotny J, "
                 "Margolies MN, Ridge RJ, Bruccoleri RE, Haber E, Crea R, Oppermann H "
                 "(1988) Protein engineering of antibody binding sites: recovery of "
                 "specific activity in an anti-digoxin single-chain Fv analogue produced "
                 "in Escherichia coli. Proc Natl Acad Sci USA 85:5879-5883.",
        description="Three Gly-Gly-Gly-Gly-Ser repeats: glycine for backbone freedom, "
                    "serine for solubility, and no side chains to pack against either "
                    "domain. Huston and colleagues used a fifteen-residue linker to tether "
                    "an antibody heavy-chain variable domain to its light-chain partner and "
                    "recover binding from a single polypeptide, which is where the "
                    "single-chain Fv - and this linker - come from.",
        witness="PDB 1DZB entity 1 (scFv fragment 1F9) carries the 15-mer exactly at the "
                "VH/VL junction. RCSB seqmotif returns 1,228 entities. The 15-residue "
                "length is stated by the paper's abstract.",
        caveat="MATCHING NOTE: repeat counts 1 to 5 all occur, so per-count rows would "
               "duplicate hits on one locus. Model this as a repeat pattern with a "
               "15-residue minimum; a bare GGGGS at 5 residues would fire constantly in "
               "glycine-rich proteins.",
    ),
    Part(
        name="(GGGGS)4 flexible linker",
        aliases=("(G4S)4", "20-residue GS linker"),
        aa="GGGGSGGGGSGGGGSGGGGS",
        **NONE_FOUND,
        pdb_entity="1H8N_1",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:3045807 (1988, PNAS 85:5879-5883), which "
                          "establishes the (Gly4Ser)n family rather than this count",
        citation="Huston JS, Levinson D, Mudgett-Hunter M, Tai MS, Novotny J, "
                 "Margolies MN, Ridge RJ, Bruccoleri RE, Haber E, Crea R, Oppermann H "
                 "(1988) Proc Natl Acad Sci USA 85:5879-5883.",
        description="The four-repeat form of the same linker, twenty residues, used where "
                    "the two tethered domains need more separation than fifteen residues "
                    "gives. A separate record only because this count is common enough to "
                    "be worth naming.",
        witness="PDB 1H8N entity 1 (anti-ampicillin scFv) carries the 20-mer at the VL/VH "
                "junction. RCSB seqmotif returns 147 entities.",
        caveat="The citation establishes the (Gly4Ser)n family, not this specific repeat "
               "count. No paper stipulates n=4.",
    ),
    Part(
        name="A(EAAAK)3A rigid helical linker",
        aliases=("EAAAK linker", "alpha-helical linker", "rigid linker"),
        aa="AEAAAKEAAAKEAAAKA",
        **NONE_FOUND,
        pdb_entity="8E4C_1",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:11579220 (2001, Protein Eng 14:529-532), whose "
                          "abstract stipulates the form A(EAAAK)nA with n = 2-5",
        citation="Arai R, Ueda H, Kitayama A, Kamiya N, Nagamune T (2001) Design of the "
                 "linkers which effectively separate domains of a bifunctional fusion "
                 "protein. Protein Eng 14:529-532.",
        description="A helix-forming linker, the deliberate opposite of a GS linker: the "
                    "glutamate-lysine spacing supports i,i+4 salt bridges that stabilise "
                    "an alpha helix, so the two fused domains are held apart at a roughly "
                    "fixed distance instead of tumbling freely. Arai and colleagues "
                    "measured the effect by inserting them between two fluorescent "
                    "proteins and watching energy transfer fall as n increased.",
        witness="The paper's abstract, verbatim: 'We introduced helix-forming peptide "
                "linkers A(EAAAK)nA (n = 2-5) between two green fluorescent protein "
                "variants, EBFP and EGFP, and investigated their spectral properties.' "
                "RCSB seqmotif returns 15 entities for the unflanked 15-mer and 1 for the "
                "flanked 17-mer (PDB 8E4C entity 1).",
        caveat="THIN STRUCTURAL EVIDENCE, stated as such: a single deposited witness for "
               "the flanked form. The abstract quote is the primary evidence for this row, "
               "and the flanking alanines are part of the published design, not padding.",
    ),
)


# Every pair where one part's residues are a substring of another's, DECLARED,
# with what happens when both fire on one locus. Checked in self_test() against
# the pairs actually present, so a new part cannot introduce an undeclared one.
#
# Why declared rather than banned: three of these five already ship and are
# signed. A ban would retroactively condemn them, and it would be wrong to —
# FLAG really is inside 3xFLAG, and a tool that refused to say so would be
# hiding a true thing about the construct.
#
# The measurement is silent on all of this. It counts occurrences of one part at
# a time; it cannot see two records competing, and the corpus contains no
# FLAG-tagged construct at all (FLAG measured zero occurrences), which is
# precisely the case where the DDDDK pair fires.
DECLARED_CONTAINMENT: tuple[tuple[str, str, str], ...] = (
    ("FLAG tag", "3xFLAG tag",
     "Ships and is signed. The two compete rather than nest: after the 15% "
     "overlap trim, FLAG's core runs past the end of 3xFLAG's, so `contained_in` "
     "is false and `resolve_overlaps` keeps the higher-scoring 22-residue row."),
    ("Strep-tag II", "Twin-Strep-tag",
     "Ships and is signed. Twin-Strep carries TWO copies of Strep-tag II, one at "
     "each end, so the same trim argument applies at both and the 28-residue row "
     "wins the span."),
    ("(GGGGS)3 flexible linker", "(GGGGS)4 flexible linker",
     "Ships and is signed. Same shape, and the reason the repeat family is "
     "represented by two rows rather than a wildcard."),
    ("Enterokinase cleavage site", "FLAG tag",
     "NEW, and the one with an unresolved edge. On a locus where both are "
     "reported the pair resolves correctly: DDDDK is FLAG's last five residues, "
     "its trimmed core hangs off FLAG's 3' end, so the two compete and FLAG wins "
     "on score (24 against 15). BUT they clear the fusion gate at different ORF "
     "sizes -- DDDDK needs aa_len >= 25 and FLAG needs >= 28 -- so in an ORF of "
     "25, 26 or 27 residues a FLAG-tagged construct is annotated 'Enterokinase "
     "cleavage site' and no FLAG. That window is declared here rather than fixed "
     "here: fixing it means a containment rule inside the annotator, which is a "
     "behaviour change with its own tests. Read from the code, not demonstrated "
     "by a run, and the corpus cannot demonstrate it either."),
    ("Enterokinase cleavage site", "3xFLAG tag",
     "NEW, same relationship one level out: 3xFLAG's third unit is a whole FLAG, "
     "so it ends in DDDDK too. 3xFLAG needs an ORF of >= 42 residues, so the "
     "window above is wider here, not narrower."),
)


# --------------------------------------------------------------------------
# The gates. Pure functions, so the self-test can prove they fail.


# An INSDC protein_id: letters, digits, and a version suffix. UniProt writes "-"
# in the ProteinId property when a cross-referenced record has no translation
# accession, and P03438 has one. Left unfiltered it becomes a request for
# `.../ena/browser/api/fasta/-`, which returns HTTP 500, and build.fetch() turns
# a failed fetch into SystemExit — so one placeholder in one cross-reference list
# takes down the entire build. Filtered here rather than caught later, because
# "-" is not a fetch that failed, it is a fetch that should never have been made.
PROTEIN_ID = re.compile(r"^[A-Za-z]{2,4}\d{4,9}(\.\d+)?$")


def usable_protein_ids(entry: dict) -> tuple[list[str], list[str]]:
    """Split an entry's EMBL ProteinIds into fetchable and unfetchable."""
    ids = {x.get("protein_id", "") for x in entry.get("uniProtKBCrossReferences", [])}
    good = sorted(i for i in ids if PROTEIN_ID.match(i))
    bad = sorted(i for i in ids if not PROTEIN_ID.match(i))
    return good, bad


def locate_unique(canonical: str, pep: str) -> int:
    """Index of `pep` in `canonical`, requiring exactly one occurrence.

    Both failure modes matter and they fail differently. Zero occurrences means
    the residue string in PARTS is wrong, or is not from this protein at all —
    which is the one thing a hand-written sequence table can silently get wrong,
    so it must stop the row rather than warn. Two or more means the offset is a
    coin toss, and a coin toss picks the nucleotides.
    """
    n = canonical.count(pep)
    if n == 0:
        raise ValueError(
            f"peptide not found in the canonical sequence ({len(canonical)} aa); "
            f"the residue string in PARTS does not come from this entry"
        )
    if n > 1:
        raise ValueError(
            f"peptide occurs {n} times in the canonical; the codon offset would be "
            f"an arbitrary choice among {n} of them"
        )
    return canonical.index(pep)


def rcsb_entity(entity: str, refresh: bool) -> tuple[str, str, dict]:
    """The deposited one-letter sequence of a wwPDB polymer entity.

    Returns (sequence, deposited description, cache metadata). `entity` is
    `ENTRY_N`, e.g. `8RMO_1`.

    Reads `entity_poly.pdbx_seq_one_letter_code_can` and the deposited
    description, and nothing else. That narrowness is the licence position, not
    tidiness: SOURCING.md §1 clears `wwpdb / CC0-1.0` over the PDB *archive*,
    while RCSB's own website layer is separately CC BY 4.0. The description is
    printed in the build report so a reviewer can see the entity is the one the
    table names; it is deliberately not written into any row.

    `_can` is the canonical one-letter form, in which a modified residue reads
    as its parent amino acid. That is the right field here and the choice
    matters: PDB 6I2G's ALFA-tag entity is deposited with N-terminal
    pyrrolidone and C-terminal amide groups, so the non-canonical field spells
    them out and the thirteen residues of the tag cannot be located in it.
    """
    entry, n = entity.split("_")
    name = f"rcsb_{entry}_{n}.json"
    raw = json.loads(fetch(RCSB_ENTITY.format(entry, n), name, refresh))
    seq = (raw.get("entity_poly", {}).get("pdbx_seq_one_letter_code_can") or "")
    seq = "".join(seq.split()).upper()
    desc = raw.get("rcsb_polymer_entity", {}).get("pdbx_description", "") or ""
    return seq, desc, cached_meta(name)


def codon_slice(cds_nt: str, offset_aa: int, n_aa: int) -> str:
    """The codons encoding residues [offset_aa, offset_aa + n_aa).

    Only correct if codon 1 of `cds_nt` is residue 1 of the protein — no 5'
    leader, no /codon_start other than 1. `build()` proves that by requiring
    len(nt) == 3*(len(protein)+1) before calling this, and then re-translates the
    slice anyway. A record with a leader would otherwise yield a frame-shifted
    slice that is a real subsequence of a real gene and encodes something else
    entirely.
    """
    return cds_nt[3 * offset_aa: 3 * (offset_aa + n_aa)]


def dropped_from_the_allow_list() -> list[str]:
    """Named candidates deliberately NOT in PARTS, so they are not re-litigated.

    Each was considered and rejected on evidence. Recording them here costs
    nothing and stops the next person rediscovering the same dead end — or worse,
    adding one of them from recall because "it is obviously a real tag".
    """
    return [
        "SUMO / Smt3 -- a recalled 36-residue C-terminus matched 28 PDB entities, but the "
        "top hit was human SUMO-3 wearing the obsolete name SMT3A, and the string is "
        "absent from UniProt Q12306 (SMT3_YEAST). It was human SUMO-2/3 under a yeast "
        "name. Separately: SUMO is a natural protein with a real CDS, so it belongs in "
        "Stage 2 with a translation check, not in this table.",
        "GST (Sj26) and MBP (malE) -- already on the Stage 2 allow-list. Duplicating them "
        "here would give one feature two rows with two different boundary rules.",
        "T7-Tag (MASMTGGQQMG) -- sequence verified (UniProt P19726 begins with it; 1,483 "
        "PDB entities), but the 11-residue boundary traces only to a vendor manual. Class "
        "C requires that the citation IS the provenance, and a product manual is neither "
        "citable nor licence-clean. Available if the curator will stipulate 'residues 1-11 "
        "of P19726' under his own name.",
        "HSV-Tag (QPELAPEDPED) -- same shape of failure. Present in UniProt Q69091 at "
        "290-300 and in PDB 1JMA/1L2G, but four literature searches found no paper "
        "defining it as a tag.",
        "Spot-Tag -- unverified outright. The sequence recalled for it returns ZERO RCSB "
        "seqmotif entities, so there is nothing to propose. It may well be a real tag with "
        "a different sequence; writing the remembered one is exactly the forbidden move.",
        "Sortase A motif (LPETG) -- only 2 entities returned under a sortase text filter "
        "and no primary citation was obtained. At 5-6 residues it is also far above the "
        "false-positive budget.",
        "(GGGGS)1 and (GGGGS)2 -- not a verification failure but a matchability one. At 5 "
        "and 10 residues they would fire constantly; the repeat family is represented by "
        "the 15-residue minimum instead.",
    ]


# --------------------------------------------------------------------------
# Stage 5


def takes_peptide_route(p: Part) -> bool:
    """Will this stage try to build a peptide row for `p`?

    One definition, because three places used to spell it out and a fourth was
    about to. A part takes the peptide route when it has no verified parent, or
    when it has one but is too short for MIN_NT to slice codons out of.
    """
    return not p.parent_uniprot or 3 * len(p.aa) < MIN_NT


def on_peptide_route(parts) -> list:
    """`parts`, filtered by [`takes_peptide_route`]."""
    return [p for p in parts if takes_peptide_route(p)]


def occurrence_verdict(p: Part) -> str:
    """Why this part may or may not ship, on the measured record. "" means ship.

    THE GATE THAT REPLACED MIN_PEPTIDE_AA, and it is three clauses because it is
    three different claims:

        a measurement exists at all       occurrences >= 0
        every occurrence was read         adjudicated == occurrences
        none of them was a chance hit     spurious == 0

    `occurrences == 0` passes vacuously and correctly: nothing to read, nothing
    spurious. That is the whole reason this is not a length rule — DDDDK at five
    residues occurred zero times and IEGR at four occurred 154, and no floor can
    tell those apart because length is not the property being tested.

    A pure function of the part, so `self_test` can prove each clause fails.
    """
    if p.occurrences < 0:
        return ("no occurrence measurement, so nothing is known about how often it "
                "would fire; run features/build's corpus count and record it")
    if p.adjudicated != p.occurrences:
        return (f"{p.occurrences} occurrence(s) in {p.occurrence_files or 'the corpus'}, "
                f"of which {max(p.adjudicated, 0)} were read. A part ships when every "
                f"occurrence has been read, not when somebody has argued it is rare")
    if p.spurious != 0:
        return (f"{max(p.spurious, 0)} of {p.adjudicated} adjudicated occurrence(s) were "
                f"not this part")
    return ""


def admitted_by_measurement(p: Part) -> bool:
    """Is the occurrence record what lets this part into the table?

    True only for a part the retired `MIN_PEPTIDE_AA = 8` held. Every peptide
    row is *gated* by `occurrence_verdict`, but only these five are *admitted*
    by it; the rest cleared the floor and were admitted by that. See
    RETIRED_PEPTIDE_FLOOR for why the distinction is load-bearing rather than
    pedantic — one half of it is a false claim in a signed column and the other
    half is fourteen lapsed signatures.
    """
    return len(p.aa) < RETIRED_PEPTIDE_FLOOR


def measurement_paragraph(p: Part) -> str:
    """The occurrence record as it goes into `notes`, or "" for a row that does
    not need it.

    Empty for every part the retired floor already admitted, and that emptiness
    is what keeps their `notes` byte-identical and their signatures alive. The
    measurement itself is not lost for those rows: `occurrence_verdict` reads it
    on every peptide row, and the build report prints `occ=/adj=/spurious=` for
    all of them.
    """
    if not admitted_by_measurement(p):
        return ""
    return (
        f"HOW OFTEN IT WOULD FIRE ON SEQUENCE NOBODY TAGGED, measured rather than "
        f"modelled - this is what admits the row, in place of the peptide length floor "
        f"this table used to carry. {p.occurrences} occurrence(s), "
        f"{p.occurrence_files}, of which {p.adjudicated} were read by a human and "
        f"{p.spurious} turned out not to be this part. Corpus: {p.occurrence_corpus} "
        f"{p.occurrence_note} "
    )


def build_peptide(p: Part, rid: str, ordinal: int, refresh: bool,
                  report: list) -> "Row | None":
    """The peptide route: a residue string verified against a fetched record.

    Taken by a part with no gene to slice codons out of, or one whose gene is
    too short for MIN_NT. The gate is the same shape as the nucleotide route's —
    locate the peptide, exactly once, in a sequence fetched at build time —
    because without it the `aa=` literal in PARTS would go straight into
    features.tsv and the row would be shipped, unverified. That is worse than
    the unissued state it replaces.
    """
    tag = f"{rid} {p.name}"
    # THE MEASUREMENT FIRST, and the order is the point. A row held on evidence
    # about what it would do should say so, not report whichever mechanical
    # floor happens to be checked earliest -- that is how "shorter than the
    # floor" came to be the recorded reason for holding His6, which the
    # measurement says was the most valuable row in the table.
    held = occurrence_verdict(p)
    if held:
        report.append(f"  HOLD {rid} {p.name:40s} {held}")
        if len(p.aa) < MIN_PART_AA:
            report.append(
                f"       ...and, independently, {len(p.aa)} aa is below MIN_PART_AA = "
                f"{MIN_PART_AA}, so the annotator would refuse it outright. Whoever "
                f"clears the occurrences must move ORF_MIN_AA in "
                f"crates/pl-features/src/annotate.rs too."
            )
        return None
    # STRUCTURAL, and not the specificity gate. See MIN_PART_AA: a shorter part
    # is findable in a 25-residue ORF and silently invisible in a 24-residue
    # one, and the annotator refuses it outright.
    if len(p.aa) < MIN_PART_AA:
        report.append(
            f"  HOLD {rid} {p.name:40s} {len(p.aa)} aa, below MIN_PART_AA = "
            f"{MIN_PART_AA}; the annotator refuses it. Move ORF_MIN_AA in "
            f"crates/pl-features/src/annotate.rs to admit it."
        )
        return None

    # WHERE THE RESIDUES ARE CHECKED. A wwPDB polymer entity for a designed part
    # with no gene; the UniProt parent for a part that HAS one but is too short
    # for MIN_NT to slice codons out of it. The second exists for exactly one row
    # (enterokinase), and it is not a loosening: the requirement was always "a
    # sequence fetched at build time", and refusing a fetched UniProt canonical
    # in favour of no witness at all would have been the letter of the rule
    # against its point.
    if p.pdb_entity:
        try:
            deposited, desc, meta = rcsb_entity(p.pdb_entity, refresh)
        except Exception as e:  # noqa: BLE001 — one bad part must not kill the stage
            report.append(f"  DROP {tag}: wwPDB fetch failed for {p.pdb_entity}: {e}")
            return None
        if not deposited:
            report.append(f"  DROP {tag}: {p.pdb_entity} carries no one-letter sequence")
            return None
        witness_kind = f"wwPDB polymer entity {p.pdb_entity}"
        # BYTE-IDENTICAL to what this template rendered before the UniProt
        # branch existed, and that is a requirement rather than a preference.
        # `notes` is in SIGNED_COLUMNS, so rewording it moves
        # `Db::content_digest` and lapses the signature on every row it touches.
        # Folding this phrase into the shared `{witness_kind}` dropped the words
        # "deposited one-letter" and took fourteen of Dr Lobel's eighty-four
        # signatures with it: `the_shipped_database_parses_and_ships_only_what_
        # is_signed` and all five corpus tests went red, and FLAG, Strep-tag and
        # twelve others stopped being searched by default.
        witness_phrase = (
            f"deposited one-letter sequence of wwPDB polymer entity {p.pdb_entity}"
        )
        witness_prov = (rid, "reference_aa", "wwpdb", p.pdb_entity, "CC0-1.0",
                        RCSB_ENTITY.format(*p.pdb_entity.split("_")),
                        meta.get("retrieved", TODAY), meta.get("sha256", ""))
    elif p.parent_uniprot:
        try:
            raw = json.loads(fetch(
                UNIPROT_JSON.format(p.parent_uniprot),
                f"uniprot_{p.parent_uniprot}.json",
                refresh,
            ))
        except Exception as e:  # noqa: BLE001
            report.append(f"  DROP {tag}: UniProt fetch failed for "
                          f"{p.parent_uniprot}: {e}")
            return None
        entry = pick_uniprot(raw)
        deposited = entry.get("sequence", {}).get("value", "").upper()
        desc = entry.get("primaryAccession", p.parent_uniprot)
        meta = cached_meta(f"uniprot_{p.parent_uniprot}.json")
        if not deposited:
            report.append(f"  DROP {tag}: {p.parent_uniprot} carries no canonical sequence")
            return None
        witness_kind = f"UniProt canonical {p.parent_uniprot}"
        witness_phrase = f"canonical sequence of UniProt {p.parent_uniprot}"
        witness_prov = (rid, "reference_aa", "uniprot", p.parent_uniprot, "CC-BY-4.0",
                        UNIPROT_JSON.format(p.parent_uniprot),
                        meta.get("retrieved", TODAY), meta.get("sha256", ""))
    else:
        report.append(
            f"  DROP {tag}: neither a pdb_entity nor a parent_uniprot, so the residue "
            f"string could only be taken on trust. A declared row is better than an "
            f"unverified one."
        )
        return None
    try:
        offset = locate_unique(deposited, p.aa)
    except ValueError as e:
        report.append(f"  DROP {tag}: in {witness_kind}: {e}")
        return None

    for a in (p.name, *p.aliases):
        if "|" in a or "\t" in a:
            raise SystemExit(f"{tag}: name/alias {a!r} contains a cell delimiter")

    notes = (
        f"CLASS C, PEPTIDE REFERENCE. The boundary is stipulated by the citation in "
        f"boundary_evidence, not derived from a record, and this row carries NO "
        f"nucleotides on purpose: the peptide has dozens of synonymous encodings and "
        f"any one of them would be an arbitrary choice that misses every re-coded copy "
        f"(features/SOURCING.md section 3). "
        f"reference_aa is {len(p.aa)} residues, located by exact search - found once - "
        f"at residue {offset + 1} of the {len(deposited)}-residue {witness_phrase}, "
        f"fetched at build time. "
        f"Verified for this table against: {p.witness} "
        f"HOW THIS ROW IS MATCHED, which is not how the nucleotide rows are matched: "
        f"only by six-frame translation, only at zero edit distance over the whole "
        f"peptide regardless of the identity threshold, and only when the hit lies in "
        f"frame inside an open reading frame of the query with at least 20 residues of "
        f"that ORF outside the tag. A hit that fails the fusion rule is dropped, not "
        f"reported as a fragment - the PI's decision of 2026-07-28, in his words: "
        f"'add these sequences, but make sure they are fused to an ORF, otherwise "
        f"ignored'. So this row will NOT fire on an empty tagging vector whose "
        f"polylinker meets a stop within 20 codons, and will NOT fire on a 5'-truncated "
        f"fragment with no initiator. "
        + measurement_paragraph(p)
        + f"Citation: {p.citation}"
    )
    if p.caveat:
        notes += " " + p.caveat

    for field, text in (("name", p.name), ("description", p.description),
                        ("notes", notes)):
        bad = [c for c in text if ord(c) > 126]
        if bad:
            raise SystemExit(
                f"{tag} {field}: non-ASCII {sorted(set(bad))!r}. This table is "
                f"hand-written, so a smart quote pasted from a PDF gets in silently "
                f"and comes out of the TSV as mojibake."
            )

    report.append(
        f"  OK   {rid} {p.name:34s} {len(p.aa):4d} aa  {witness_kind} residue "
        f"{offset + 1}  ({desc[:38]!r})  occ={p.occurrences} adj={p.adjudicated} "
        f"spurious={p.spurious}"
    )
    return Row(
        id=rid,
        ordinal=ordinal,
        name=p.name,
        aliases=list(p.aliases),
        cls=p.cls,
        genbank_key=p.genbank_key,
        # Empty, and the whole point. See the notes above.
        reference_nt="",
        reference_aa=p.aa,
        boundary_rule=p.boundary_rule,
        boundary_evidence=p.boundary_evidence,
        description=p.description,
        notes=notes,
        patent_flag=p.patent_flag,
        provenance=[
            # The bytes the residues were read out of. For a wwPDB witness that
            # is `wwpdb` and not `rcsb`: the CC0 dedication is the wwPDB's, over
            # the archive, and RCSB's own website layer is separately CC BY 4.0.
            witness_prov,
            # For Class C the citation IS the provenance of the boundary.
            (rid, "boundary_evidence", "polylinker", p.boundary_evidence,
             "own-work", "-", TODAY, ""),
            (rid, "name", "polylinker", "-", "own-work", "-", TODAY, ""),
            (rid, "aliases", "polylinker", "-", "own-work", "-", TODAY, ""),
            (rid, "boundary_rule", "polylinker", "-", "own-work", "-", TODAY, ""),
            (rid, "description", "polylinker", p.boundary_evidence,
             "own-work", "-", TODAY, ""),
            (rid, "notes", "polylinker", "-", "own-work", "-", TODAY, ""),
        ],
    )


def build(refresh: bool) -> tuple[list, list]:
    """Return (rows, report), the shape every stage in this build returns."""
    rows, report = [], []
    built_nt = built_aa = blocked = 0

    for i, p in enumerate(PARTS):
        ordinal = i + 1
        rid = f"PLF:{PLF_BLOCK_BASE + i:04d}"
        tag = f"{rid} {p.name}"

        # WHICH ROUTE, and never both. A row carrying nucleotides is matched by
        # the tier-1 index and, if it also carried a peptide, by the UNGATED
        # tier-2 scan -- the annotator's exact-match and ORF-fusion rules apply
        # only to peptide-only rows. Giving the eight nucleotide rows a peptide
        # as well would make a nine-residue epitope matchable with no ORF
        # requirement at all, which is a behaviour change nobody asked for.
        if takes_peptide_route(p):
            if p.parent_uniprot:
                report.append(
                    f"       {rid} {p.name:40s} {len(p.aa)} aa = {3 * len(p.aa)} bp is "
                    f"below the {MIN_NT} bp floor, so parent {p.parent_uniprot} cannot "
                    f"supply a nucleotide reference; trying the peptide route"
                )
            row = build_peptide(p, rid, ordinal, refresh, report)
            if row is None:
                blocked += 1
            else:
                built_aa += 1
                rows.append(row)
            continue

        try:
            raw = json.loads(fetch(
                UNIPROT_JSON.format(p.parent_uniprot),
                f"uniprot_{p.parent_uniprot}.json",
                refresh,
            ))
        except Exception as e:  # noqa: BLE001 — one bad part must not kill the stage
            blocked += 1
            report.append(f"  DROP {tag}: UniProt fetch failed: {e}")
            continue

        entry = pick_uniprot(raw)
        up_meta = cached_meta(f"uniprot_{p.parent_uniprot}.json")
        canonical = entry.get("sequence", {}).get("value", "").upper()
        if not canonical:
            blocked += 1
            report.append(f"  DROP {tag}: entry carries no canonical sequence")
            continue

        # Gate 1 — is the residue string in PARTS actually in this protein?
        try:
            offset = locate_unique(canonical, p.aa)
        except ValueError as e:
            blocked += 1
            report.append(f"  DROP {tag}: {e}")
            continue
        if offset == 0:
            # Residue 1 is read as Met whatever the codon is, so a peptide that
            # starts at the N-terminus cannot be round-tripped by plain
            # translation. None of these do; if one ever did, it needs the
            # alternative-initiation rule and a decision, not a silent pass.
            blocked += 1
            report.append(
                f"  DROP {tag}: peptide starts at residue 1, where the initiation codon "
                f"is read as Met regardless of what it is"
            )
            continue

        # Gate 2 — a cross-reference whose CDS translates to the canonical
        # EXACTLY. This is the same gate Stage 2 uses, and for the same reason:
        # a UniProt entry's EMBL cross-references routinely point at different
        # alleles, fragments and alternative starts, and one of them is
        # sometimes an engineered variant filed under the wild type.
        xrefs, unusable = usable_protein_ids(entry)
        chosen = None
        tried = [f"{u!r}: not an INSDC protein_id, never fetched" for u in unusable]
        for pid in xrefs:
            try:
                full_nt, nt_info = ena_cds_nt(pid, refresh)
            except Exception as e:  # noqa: BLE001
                tried.append(f"{pid}: fetch failed ({e})")
                continue
            # `cds_matches_protein`, not `translate_cds(...) == canonical`. The
            # latter reads like an exact-match test and is not one: it rewrites
            # residue 1 to Met for six alternative initiation codons, so a CDS
            # that disagrees with its protein at position 1 passes it silently.
            # It did. P01106 (MYC) is reached through AAH00141.2, a human
            # transcript whose CDS begins CTG, and this row shipped a `notes`
            # field asserting an unqualified whole-CDS match that had not been
            # performed -- via a rule whose justifying comment names six
            # *bacterial* AMR markers. `how` is the sentence describing what was
            # actually accepted, and it goes into the row.
            ok, how = cds_matches_protein(full_nt, canonical)
            if not ok:
                tried.append(f"{pid}: does not translate to the canonical")
                continue
            # Codon i must live at 3*i. A record with a 5' leader or a
            # /codon_start other than 1 breaks that silently, and the slice would
            # still be real nucleotides from a real gene encoding the wrong
            # thing. nt == 3*(aa+1) also proves the CDS carries its stop.
            if len(full_nt) != 3 * (len(canonical) + 1):
                tried.append(
                    f"{pid}: {len(full_nt)} nt != 3*({len(canonical)}+1), so codon "
                    f"positions are not 3*i and the slice would be frame-shifted"
                )
                continue
            chosen = (pid, full_nt, nt_info, how)
            break

        if chosen is None:
            blocked += 1
            report.append(
                f"  DROP {tag}: no EMBL cross-reference gave a usable CDS "
                f"({len(xrefs)} tried)"
            )
            for t in tried:
                report.append(f"         {t}")
            continue

        pid, full_nt, nt_info, how = chosen
        nt = codon_slice(full_nt, offset, len(p.aa))

        # Gate 3 — the belt to gate 2's braces. It cannot fail if the length
        # check above passed, which is exactly why it is here: if it ever does
        # fail, an assumption this file makes about ENA records has stopped
        # holding, and that must surface as a dropped row rather than as a
        # shipped sequence.
        back = translate(nt)
        if back != p.aa:
            blocked += 1
            report.append(
                f"  DROP {tag}: codons {offset * 3 + 1}-{(offset + len(p.aa)) * 3} of "
                f"{pid} translate to {back!r}, not to the declared peptide"
            )
            continue

        for a in (p.name, *p.aliases):
            if "|" in a or "\t" in a:
                raise SystemExit(f"{tag}: name/alias {a!r} contains a cell delimiter")

        notes = (
            f"CLASS C, and the boundary is stipulated by the citation in "
            f"boundary_evidence, not derived from a record. "
            f"reference_nt is codons {offset * 3 + 1}-{(offset + len(p.aa)) * 3} of "
            f"{pid} ({len(nt)} bp), the codons encoding residues {offset + 1}-"
            f"{offset + len(p.aa)} of UniProt {p.parent_uniprot}; the peptide was located "
            f"by exact search in that canonical (found once) and the slice re-translates "
            f"to it exactly. The whole CDS was first compared to the canonical residue by "
            f"residue: {how}. {len(full_nt)} == 3*({len(canonical)}+1) confirms codon 1 is "
            f"residue 1. Verified for this table against: {p.witness} "
            f"READ THIS BEFORE USING THE ROW: reference_nt is ONE natural encoding of the "
            f"peptide. Vector versions of this element are routinely re-coded, so "
            f"nucleotide matching will MISS most real occurrences. That limitation is "
            f"unchanged, but its cause is no longer the schema: since 2026-07-28 class "
            f"'synthetic_part' may carry reference_aa, and fourteen sibling rows in this "
            f"block do. This row deliberately does not. A row carrying BOTH would be "
            f"matched by the tier-1 nucleotide index and by the tier-2 translated scan, "
            f"and the annotator's exact-match and ORF-fusion rules apply only to "
            f"peptide-ONLY rows - so adding a peptide here would make a short epitope "
            f"matchable with no ORF requirement at all. Giving the parented parts a "
            f"peptide reference as well is a real improvement and a separate decision "
            f"with its own tests; see features/README.md, 'Known gaps'. "
            f"Citation: {p.citation}"
        )
        if p.caveat:
            notes += " " + p.caveat

        for field, text in (("name", p.name), ("description", p.description),
                            ("notes", notes)):
            bad = [c for c in text if ord(c) > 126]
            if bad:
                raise SystemExit(
                    f"{tag} {field}: non-ASCII {sorted(set(bad))!r}. This table is "
                    f"hand-written, so a smart quote pasted from a PDF gets in silently "
                    f"and comes out of the TSV as mojibake."
                )

        rows.append(Row(
            id=rid,
            ordinal=ordinal,
            name=p.name,
            aliases=list(p.aliases),
            cls=p.cls,
            genbank_key=p.genbank_key,
            reference_nt=nt,
            # Empty, and no longer because the schema forbids it -- it does not,
            # since 2026-07-28. Empty because a row carrying both references is
            # matched by the ungated tier-2 scan as well as by tier 1, and the
            # fusion rule only guards peptide-ONLY rows. See the notes text.
            reference_aa="",
            boundary_rule=p.boundary_rule,
            boundary_evidence=p.boundary_evidence,
            description=p.description,
            notes=notes,
            patent_flag=p.patent_flag,
            provenance=[
                # The bytes the nucleotides were read out of.
                (rid, "reference_nt", "ena", pid, "INSDC-free", ENA_FASTA.format(pid),
                 nt_info["cache"].get("retrieved", TODAY),
                 nt_info["cache"].get("sha256", "")),
                # Where the residue offset came from. The peptide's *position* is
                # UniProt's sequence, so UniProt is credited for it even though no
                # UniProt string is stored in the row.
                # The residue offset is a fact about UniProt's canonical
                # sequence, so UniProt is credited for the slice it selects even
                # though no UniProt string is stored in the row. This used to be
                # keyed on a field called `peptide_anchor`, which is not a column
                # of features.tsv and therefore attributed nothing at all -- a
                # misspelling would have been equally silent, and nothing checked.
                (rid, "reference_nt", "uniprot", p.parent_uniprot, "CC-BY-4.0",
                 UNIPROT_JSON.format(p.parent_uniprot),
                 up_meta.get("retrieved", TODAY), up_meta.get("sha256", "")),
                # For Class C the citation IS the provenance of the boundary.
                (rid, "boundary_evidence", "polylinker", p.boundary_evidence,
                 "own-work", "-", TODAY, ""),
                (rid, "name", "polylinker", "-", "own-work", "-", TODAY, ""),
                (rid, "aliases", "polylinker", "-", "own-work", "-", TODAY, ""),
                (rid, "boundary_rule", "polylinker", "-", "own-work", "-", TODAY, ""),
                (rid, "description", "polylinker", p.boundary_evidence,
                 "own-work", "-", TODAY, ""),
                (rid, "notes", "polylinker", "-", "own-work", "-", TODAY, ""),
            ],
        ))
        built_nt += 1
        report.append(
            f"  OK   {rid} {p.name:34s} {len(nt):4d} bp  {pid} codons "
            f"{offset * 3 + 1}-{(offset + len(p.aa)) * 3}  (residues {offset + 1}-"
            f"{offset + len(p.aa)} of {p.parent_uniprot})"
        )

    report.append(
        f"  -- {built_nt} nucleotide row(s) + {built_aa} peptide row(s) = "
        f"{built_nt + built_aa} built, {blocked} declared and held. The held rows keep "
        f"their ordinals, so their PLF ids stay reserved and unissued."
    )
    held = [(p.name, occurrence_verdict(p)) for p in on_peptide_route(PARTS)
            if occurrence_verdict(p)]
    if held:
        report.append(
            f"  -- CURATOR: {len(held)} row(s) held, and every one is held by the "
            f"occurrence record rather than by sourcing or by length. A held row needs "
            f"somebody to READ its occurrences, not an argument about its length:"
        )
        for name, why in held:
            report.append(f"       {name}: {why}")
        report.append(
            "       Corpus for all of these: " + CORPUS
        )
    report.append(
        f"  -- {len(dropped_from_the_allow_list())} named candidate(s) are deliberately "
        f"not in PARTS at all; see dropped_from_the_allow_list()."
    )
    return rows, report


# --------------------------------------------------------------------------
# Proving the gates can fail


def self_test() -> list[str]:
    """Run each gate against input that must trip it.

    Every gate in `build()` is silent in a green run, so without this "no error"
    is indistinguishable from "the check does nothing". Deliberately offline and
    on literals: a gate that only fires on live data cannot be demonstrated on a
    day the network is down, which is exactly when someone will be tempted to
    conclude it passed.
    """
    out = []

    # locate_unique must reject an absent peptide — the failure a hand-written
    # residue table actually has.
    try:
        locate_unique("MKVLAAGIVGWA", "DYKDDDDK")
    except ValueError as e:
        out.append(f"  SELFTEST absent peptide rejected: {e}")
    else:
        raise SystemExit("SELF-TEST FAILED: locate_unique accepted a peptide that is absent")

    # ...and an ambiguous one.
    try:
        locate_unique("AAGSGSKKKGSGSAA", "GSGS")
    except ValueError as e:
        out.append(f"  SELFTEST ambiguous peptide rejected: {e}")
    else:
        raise SystemExit("SELF-TEST FAILED: locate_unique accepted a peptide occurring twice")

    # ...and must accept a unique one at the right offset. The offset is 0-based
    # and the peptide starts at residue 4, so this asserts 3 — the off-by-one
    # that would frame-shift every slice by one codon.
    if locate_unique("MKWENLYFQGS", "ENLYFQG") != 3:
        raise SystemExit("SELF-TEST FAILED: locate_unique returned the wrong offset")
    out.append("  SELFTEST unique peptide located at the expected offset")

    # codon_slice + translate must round-trip, and must NOT round-trip when the
    # offset is off by one — which is what a /codon_start != 1 record would do.
    cds = "ATGAAATGGGAAAACCTGTATTTTCAGGGCAGCTAA"   # M K W E N L Y F Q G S *
    if translate(cds).rstrip("*") != "MKWENLYFQGS":
        raise SystemExit("SELF-TEST FAILED: the fixture CDS does not translate as claimed")
    if translate(codon_slice(cds, 3, 7)) != "ENLYFQG":
        raise SystemExit("SELF-TEST FAILED: codon_slice did not recover the peptide")
    if translate(codon_slice(cds, 4, 7)) == "ENLYFQG":
        raise SystemExit("SELF-TEST FAILED: a shifted slice still matched, so the "
                         "round-trip check cannot detect a frame error")
    out.append("  SELFTEST codon_slice round-trips, and a one-residue shift does not")

    # The MIN_NT floor must actually exclude the two short protease sites, and
    # must not exclude anything this stage claims to build.
    short = [p.name for p in PARTS if p.parent_uniprot and 3 * len(p.aa) < MIN_NT]
    if sorted(short) != ["Enterokinase cleavage site", "Factor Xa cleavage site"]:
        raise SystemExit(f"SELF-TEST FAILED: the MIN_NT floor now excludes {short}")
    out.append(f"  SELFTEST MIN_NT={MIN_NT} bp excludes exactly {short}")

    # The occurrence gate must hold exactly one part, and hold it for the reason
    # recorded. Pinned by name rather than by count, because a count would stay
    # green if the gate released one part and caught another.
    peptide_route = on_peptide_route(PARTS)
    held = sorted(p.name for p in peptide_route if occurrence_verdict(p))
    expected = ["Factor Xa cleavage site"]
    if held != expected:
        raise SystemExit(
            f"SELF-TEST FAILED: the occurrence gate now holds {held}, not {expected}. "
            f"Releasing or adding a hold is a curator decision -- update the list here, "
            f"the part's own occurrence fields and features/README.md together."
        )
    out.append(f"  SELFTEST the occurrence gate holds exactly {held}")

    # ...and each of its three clauses must be able to fire on its own. A gate
    # that only ever fails as a whole cannot be shown to be three claims.
    probe = PARTS[0]
    clauses = [
        ("no measurement", {"occurrences": -1, "adjudicated": -1, "spurious": -1}),
        ("unread occurrences", {"occurrences": 3, "adjudicated": 0, "spurious": 0}),
        ("a spurious hit", {"occurrences": 3, "adjudicated": 3, "spurious": 1}),
    ]
    for label, kw in clauses:
        if not occurrence_verdict(replace(probe, **kw)):
            raise SystemExit(
                f"SELF-TEST FAILED: the occurrence gate accepted a part with {label}, "
                f"so that clause of it does nothing"
            )
    if occurrence_verdict(replace(probe, occurrences=0, adjudicated=0, spurious=0)):
        raise SystemExit(
            "SELF-TEST FAILED: the occurrence gate rejected a part that occurred zero "
            "times; nothing to read and nothing spurious must pass vacuously"
        )
    out.append(f"  SELFTEST each clause of the occurrence gate fires on its own "
               f"({', '.join(c[0] for c in clauses)}), and zero passes vacuously")

    # Every part on the peptide route must carry the measurement, whether or not
    # it ships. A part with no occurrence record is not a part with a clean one.
    unmeasured = sorted(p.name for p in peptide_route if p.occurrences < 0)
    if unmeasured:
        raise SystemExit(
            f"SELF-TEST FAILED: {unmeasured} take the peptide route with no occurrence "
            f"measurement. Count them over the corpus and record the result; there is "
            f"no length below which the question stops needing an answer."
        )
    out.append(f"  SELFTEST all {len(peptide_route)} peptide-route parts carry a "
               f"measured occurrence record")

    # The structural floor, which is NOT the specificity gate and must not be
    # confused with it. MIN_PART_AA has to agree with the Rust constant of the
    # same name, because ORF_MIN_AA = MIN_PART_AA + PARTNER_MIN is what the ORF
    # search is given and a shorter part is silently invisible in ORFs one
    # residue below it.
    rust = (Path(__file__).resolve().parents[2] / "crates" / "pl-features" / "src"
            / "annotate.rs")
    m = re.search(r"const MIN_PART_AA: usize = (\d+);", rust.read_text(encoding="utf-8"))
    if not m:
        raise SystemExit(f"SELF-TEST FAILED: MIN_PART_AA not found in {rust}")
    if int(m.group(1)) != MIN_PART_AA:
        raise SystemExit(
            f"SELF-TEST FAILED: MIN_PART_AA is {MIN_PART_AA} here and {m.group(1)} in "
            f"{rust}. The annotator refuses anything below its own value, so a table "
            f"with a lower floor ships rows that make Annotator::new panic."
        )
    tooshort = sorted(p.name for p in peptide_route if len(p.aa) < MIN_PART_AA)
    if tooshort != ["Factor Xa cleavage site"]:
        raise SystemExit(
            f"SELF-TEST FAILED: MIN_PART_AA={MIN_PART_AA} now excludes {tooshort}. "
            f"Admitting a shorter part means moving ORF_MIN_AA in {rust.name} with it."
        )
    out.append(f"  SELFTEST MIN_PART_AA={MIN_PART_AA} agrees with {rust.name} and "
               f"excludes exactly {tooshort}")

    # Containment must be DECLARED, not discovered. Three of these pairs already
    # ship and are signed, so containment cannot be a ban; what it can be is a
    # relationship nobody is allowed to introduce silently.
    found = {(a.name, b.name) for a in PARTS for b in PARTS
             if a is not b and a.aa in b.aa}
    declared = {(inner, outer) for inner, outer, _ in DECLARED_CONTAINMENT}
    if found != declared:
        raise SystemExit(
            f"SELF-TEST FAILED: containment among PARTS is {sorted(found)}, declared is "
            f"{sorted(declared)}. A new part whose residues sit inside another's -- or "
            f"contain another's -- changes what the annotator reports on one locus, and "
            f"must be declared in DECLARED_CONTAINMENT with its resolution."
        )
    for _, _, why in DECLARED_CONTAINMENT:
        if not why:
            raise SystemExit("SELF-TEST FAILED: a declared containment has no resolution")
    out.append(f"  SELFTEST all {len(declared)} containment pair(s) are declared with a "
               f"stated resolution")

    # Every part must be internally coherent, checked here rather than trusted.
    for i, p in enumerate(PARTS):
        if p.cls != "synthetic_part":
            raise SystemExit(f"SELF-TEST FAILED: {p.name} is class {p.cls!r}")
        if not p.aa or set(p.aa) - set("ACDEFGHIKLMNPQRSTVWY"):
            raise SystemExit(f"SELF-TEST FAILED: {p.name} has a non-amino-acid residue")
        if not p.citation or not p.boundary_evidence or not p.witness:
            raise SystemExit(f"SELF-TEST FAILED: {p.name} lacks a citation, boundary "
                             f"evidence or witness; Class C requires all three")
        # A peptide row's residues are the whole record, so they may not be
        # taken on trust. Anything the peptide route will actually build must
        # name a fetchable witness -- a wwPDB entity, or the UniProt parent for
        # a part too short for MIN_NT -- or `build_peptide` drops it, and a part
        # that would be dropped for a reason this file can see at import time is
        # a declaration error, not a build outcome.
        will_build = (p in peptide_route and not occurrence_verdict(p)
                      and len(p.aa) >= MIN_PART_AA)
        if will_build and not (p.pdb_entity or p.parent_uniprot):
            raise SystemExit(
                f"SELF-TEST FAILED: {p.name} takes the peptide route but names neither "
                f"a pdb_entity nor a parent_uniprot, so its residue string could only "
                f"ship unverified"
            )
        if p.pdb_entity and not re.fullmatch(r"[0-9A-Za-z]{4}_\d+", p.pdb_entity):
            raise SystemExit(
                f"SELF-TEST FAILED: {p.name} pdb_entity {p.pdb_entity!r} is not "
                f"ENTRY_N; it would be fetched as a malformed URL"
            )
        # A peptide-only row may not claim a boundary derived from a reading
        # frame it does not carry -- the loader refuses it (lib.rs), and a row
        # refused by the loader is a build defect rather than a data question.
        if p in peptide_route and p.boundary_rule in ("orf_atg_to_stop",
                                                      "orf_mature_peptide"):
            raise SystemExit(
                f"SELF-TEST FAILED: {p.name} would be a peptide-only row claiming "
                f"boundary_rule {p.boundary_rule!r}, which is a claim about bases it "
                f"does not carry"
            )
    names = [p.name for p in PARTS]
    if len(set(names)) != len(names):
        raise SystemExit("SELF-TEST FAILED: duplicate part name in PARTS")
    out.append(f"  SELFTEST {len(PARTS)} parts, each with citation, boundary evidence "
               f"and a named witness")

    # The ProteinId filter must reject the placeholder that actually broke this
    # stage, and must not reject a real accession.
    good, bad = usable_protein_ids({"uniProtKBCrossReferences": [
        {"protein_id": "AAC73448.1"}, {"protein_id": "-"}, {"protein_id": "CAA24390.1"},
    ]})
    if good != ["AAC73448.1", "CAA24390.1"] or bad != ["-"]:
        raise SystemExit(f"SELF-TEST FAILED: ProteinId filter split {good!r} / {bad!r}")
    out.append("  SELFTEST the ProteinId filter drops '-' and keeps real accessions")
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="Stage 5 -- curated designed parts, standalone")
    ap.add_argument("--refresh", action="store_true", help="re-fetch every source")
    ap.add_argument("--self-test", action="store_true", help="run the gates and stop")
    args = ap.parse_args()

    print("\n".join(self_test()))
    if args.self_test:
        return 0

    rows, report = build(args.refresh)
    print("\n".join(report))
    print(f"\n{len(rows)} row(s), all 'proposed' with no curator.")
    print("\nDeliberately NOT in the allow-list:")
    for d in dropped_from_the_allow_list():
        print(f"  - {d}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
