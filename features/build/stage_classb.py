#!/usr/bin/env python3
"""Stage 5 -- Class B elements, whose boundaries are CONVENTIONS.

`features/SOURCING.md` section 3 splits features by where their boundary comes
from, and this stage is the whole of Class B: promoters, enhancers, terminators
and poly(A) signals. There is no database that says where "the CMV promoter"
ends. Section 6 prescribes the method -- *human curation plus at least two
independent GenBank exemplars each* -- and the point of this file is that the
second half of that sentence is executed rather than asserted.

What every row does and does not claim
--------------------------------------

CLAIMED: these exact bases are ENA `accession:lo-hi` on the stated strand, and
that is re-fetched and re-sliced on every build. The bases are a fact.

CLAIMED: at least two INSDC records **from different submitting addresses**
contain those exact bases. Where each of those depositors put the edges of the
feature they annotated over them, relative to ours, is measured on this build
and written into `notes` -- INCLUDING when the answer is that they annotated
nothing there, which is what the primary records say for several of these. The
rrnB operon record holds T1 and T2 and annotates no terminator at all, so the
name comes from vector records and the locus from the primary one, and the row
says so rather than implying the primary agreed.

CLAIMED, and this is new on 2026-08-10: at least `MIN_PLACEMENTS` of those
independent submissions annotate a feature at **exactly** this extent, edge for
edge. That is a different measurement from the one above -- holding the bases is
a fact about the sequence, drawing the same edges is the only thing that makes
the word *consensus* true -- and until this build only the first was tested.
`verify()` had measured each depositor's edges since the stage was written, put
them in `notes`, and then tested nothing, so a row could ship
`boundary_rule = consensus_of_insdc` on a consensus of one. Five of the twelve
did, and they now drop:

    PLF:4002 lac promoter        1 of 4 submissions place it exactly
    PLF:4003 tac promoter        1 of 3   (and that one is two records from one lab)
    PLF:4004 trc promoter        1 of 2   (and that one is the anchor itself)
    PLF:4005 CMV promoter        1 of 3
    PLF:4011 SV40 early poly(A)  1 of 3

Those numbers are this build's, printed in full with each rival's offsets every
time the stage runs; nothing above is a stored figure. The five stay in `ITEMS`
rather than moving to `HELD`, so they keep their ids, are re-measured on every
build, and come back by themselves the day a curator cites evidence that
corroborates the extent -- or the day they are re-cut to an extent the evidence
already corroborates. That is a curator's decision and not this program's,
which is why the rows are refused rather than adjusted.

NOT CLAIMED: that the extent is correct, canonical, or agreed. It is a
convention this project chose, the rival conventions are named with their
measured offsets, and `boundary_rule = consensus_of_insdc` says exactly that.

NOT CLAIMED, AND NOT CLAIMABLE: that no extent here agrees with SnapGene's, or
that agreement would mean anything if it did. The sequences are biology and
nobody owns them; an exact match to a vendor's element proves nothing about
copying. What the corroboration rule answers is the narrower question that IS
answerable -- did our own evidence force this extent, or is it one lab's
opinion? See `MIN_PLACEMENTS` below and `features/build/insdc_posture.py` for
why the wider question cannot be answered from inside this repository.

Three findings from the corpus survey that shaped the code, not just the prose
-----------------------------------------------------------------------------

1. **INSDC is contaminated with SnapGene, and the CI taint gate cannot see it.**
   Ordinary submitters deposit records annotated in SnapGene, and ENA folds
   SnapGene's `/label` into the `/note`, so the record reads
   `/note="promoter for the E. coli lac operon; label: lac promoter"`. That
   prose is SnapGene's own editorial Description column arriving through a
   source this project cleared -- and `taint_gate.py` compares *our*
   descriptions against theirs, so it cannot possibly notice a *coordinate*
   arriving this way. Three consequences, all mechanical here: `parse_embl()`
   never reads a `/note` at all; the `snapgene` flag it does set makes
   `verify()` refuse to count such a record as an independent witness; and
   `MIN_PLACEMENTS` requires the extent to be corroborated by two independent
   submissions before the row may claim a consensus, which is the only part of
   this that a depositor who retyped the note by hand cannot walk straight
   past. Counting two SnapGene-annotated deposits as "two exemplars" would
   manufacture exactly the convergence the project exists to disclaim.

   The flag is exported as `record_is_snapgene_annotated()` and DRIVEN by
   `features/build/insdc_posture.py`, which refuses this stage if the screen
   stops seeing the tell or starts seeing it everywhere. The declaration at the
   top of this file is what that gate reads.

2. **"Two exemplars" has to mean two SUBMISSIONS, not two records.** A quarter
   of the surveyed corpus is one bulk deposit from one culture collection. By
   record count several of these elements have dozens of witnesses; by
   submission they have a handful, and one 84 nt "lac promoter" convention has
   66 records and one submitter. `parse_embl()` reads the address off the
   record's own submission reference and `same_submitter()` merges the ones that
   are one lab writing its address two different ways.

3. **Depositor strand annotation is not trustworthy.** Several records annotate
   a T7/T3/SP6 promoter without `complement()`, so the enclosed bases are the
   reverse complement of the promoter. The span is right and the strand is not.
   Every sequence here is therefore located in the exemplar on BOTH strands and
   the strand actually found is recorded, rather than inherited from the
   depositor's location expression.

What is deliberately NOT here
-----------------------------

Rho-independent terminators cannot come from Rfam: SOURCING.md line 193 records
that as a confirmed negative, so rrnB T1/T2 and T7 Tphi are anchored on their
own primary records instead and no one went looking again.

Nine further elements were worked up and are NOT rows -- eight held and one
dropped outright -- each for a stated reason; see `HELD` at the bottom of this
file. A row whose boundary would be a coin toss between two equally-supported,
non-nested conventions is not a row this database can carry, because
`consensus_of_insdc` would then be false on its face.

Usage
-----
    python features/build/stage_classb.py            # from cache
    python features/build/stage_classb.py --refresh  # re-fetch everything
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

# Reused, never reimplemented: the host allow-list, the verifying cache and the
# FASTA reader all belong to build.py, and a second copy of any of them is a
# second thing that can quietly disagree with the first.
from build import (  # noqa: E402
    CACHE,
    TODAY,
    Row,
    cached_meta,
    fetch,
    parse_fasta,
)

ENA_BASE = "https://www.ebi.ac.uk/ena/browser/api"

# Stage 5's reserved block. build.STAGES is the authority and `load_stage()`
# refuses to run a stage whose declared base disagrees with it; see the comment
# on STAGES about why a base may be widened and never moved.
PLF_BLOCK_BASE = 4000
PLF_BLOCK_SIZE = 1000
ID_BASE = PLF_BLOCK_BASE

# Two, from SOURCING.md section 6: "human curation + >=2 GenBank exemplars each".
# Counted in DISTINCT SUBMITTING ADDRESSES over records that are not
# SnapGene-annotated, which is the only reading of that line that means anything
# -- see finding 2 in the module docstring.
MIN_SUBMISSIONS = 2

# Two again, and NOT the same two. SOURCING.md section 4 asks for ">=2
# independent GenBank exemplars SHOWING WHERE DEPOSITORS ACTUALLY PLACE IT", and
# until 2026-08-10 only the first half of that sentence was executed:
# MIN_SUBMISSIONS counts submissions that CONTAIN the bases, which is a claim
# about the sequence and says nothing about the boundary. `verify()` measured
# each depositor's edges, wrote them into `notes`, and then tested nothing --
# so a row could ship `boundary_rule = consensus_of_insdc` on a consensus of
# one. Five of the twelve did; they are named in the module docstring above.
#
# This is the number that makes that sentence executable: how many INDEPENDENT
# SUBMISSIONS must annotate a feature at EXACTLY the shipped extent, edge for
# edge, before the word "consensus" is allowed on the row.
#
# WHAT IT IS NOT. It is not a taint check and must never be described as one.
# It cannot show that an extent came from SnapGene; nothing here can, and
# `features/build/insdc_posture.py` says why at length. It answers a narrower
# question that is answerable: did this project's own evidence force this
# extent, or is it one lab's opinion? Agreement that the evidence explains is
# not suspicious. An extent nothing corroborates is not a consensus, whoever it
# came from -- and the rule reads no `/note`, names no vendor, and cannot be
# evaded by retyping a description.
#
# EXACT, with no tolerance. A tolerance would be a knob to widen until the row
# passed, which is the shape of every check this project has caught being
# useless. A depositor whose feature runs 380 nt further than ours is annotating
# a different thing, not agreeing with us approximately.
MIN_PLACEMENTS = 2

# features/SOURCING.md §0.6, checked by features/build/insdc_posture.py. The
# only stage in the tree holding this posture, and the reason the vocabulary has
# a fourth entry at all.
INSDC_POSTURE = {
    "posture": "feature_table_convention",
    "reason": (
        "This stage ships extents that nothing forces -- a Class B boundary is a "
        "convention somebody chose, which is precisely what a SnapGene-annotated "
        "deposit carries into INSDC. Two mechanisms answer for that and both are "
        "executed here: parse_embl() reads no /note, /label, /gene, /product or "
        "/standard_name at all and flags the records that carry SnapGene's tell so "
        "verify() refuses to count them as witnesses, and MIN_PLACEMENTS requires two "
        "independent submissions to annotate a feature at exactly the shipped extent "
        "before the row may claim a consensus."
    ),
    "screen": "record_is_snapgene_annotated",
    "corroboration": "MIN_PLACEMENTS",
}


# --------------------------------------------------------------------------
# The allow-list


@dataclass(frozen=True)
class Convention:
    """One Class B element: the bases, where they come from, and who else has them."""

    name: str
    aliases: tuple[str, ...]
    genbank_key: str
    """INSDC feature key. Not a Sequence Ontology term -- SOURCING.md Risk 4."""
    cls: str
    anchor: str
    """INSDC accession the reference sequence is SLICED FROM, unversioned.

    Unversioned on purpose, so the fetch always asks for whatever ENA serves
    today rather than for a version pinned here -- and `anchor_version` below is
    then compared against what came back."""
    anchor_version: str
    """The `ACCESSION.SV` this row was built against.

    Cross-checked, never trusted, exactly as `stage_uniprot.Natural.parent` is:
    if ENA re-versions a record under us, the build says so and drops the row
    rather than quietly citing coordinates out of a record nobody read. A
    re-version is the one upstream change that can move a coordinate without
    changing an accession, and these rows are nothing but coordinates.

    PROVEN TO FIRE by declaring `J01695.1` for the rrnB rows and rebuilding:

        DROP ValueError: J01695 is now J01695.2, and this row was built against
        J01695.1 ...

    J01695 really is at SV 2, and the harvest that produced these coordinates
    reported it unversioned -- so the check earned its keep on the first record
    it was pointed at."""
    lo: int
    hi: int
    strand: str
    sequence: str
    """What the row will ship, 5'->3' on the element's own strand.

    Written out here and re-derived from `anchor:lo-hi` on every build. The pin
    chooses the interval; it does not excuse the check. A sequence that no
    longer matches its own coordinates drops the row rather than being
    corrected, exactly as in Stage 2."""
    exemplars: tuple[str, ...]
    """Further INSDC records expected to contain `sequence` verbatim.

    Not "records that mention this element" -- records whose bases contain
    ours. Which of them count as independent witnesses is decided at build time
    from their submission addresses and their SnapGene status, not here."""
    description: str
    """Ours, always. Never from a /note, a vendor map, or any tool's feature
    list; see SOURCING.md Risk 1 on why prose is the layer that matters."""
    caveat: str
    """What a curator must decide or must not assume. Goes into `notes`
    verbatim."""
    patent_flag: bool = False


# `genbank_key` uses the SPECIFIC keys -- `promoter`, `enhancer`, `terminator`,
# `polyA_signal` -- and not the current INSDC spelling. That is a decision, it
# went the other way first, and the reason it came back is worth recording.
#
# INSDC retired all four in favour of `regulatory` plus a `/regulatory_class`
# qualifier, and the anchor records have moved with it: V01146 writes
# `regulatory` + /regulatory_class="promoter", J02400 writes `regulatory` +
# /regulatory_class="polyA_signal_sequence". So `regulatory` on all twelve rows
# is the spec-correct answer, and that is what this stage emitted at first.
#
# It broke a shipped honesty disclosure. `Db::absent_common_kinds` probes the
# table for the literal keys `promoter`, `terminator` and `rep_origin` and is
# what makes the desktop app and `pl methods annotate` tell a user "no promoter
# is in this database yet" -- the sentence that exists so that somebody watching
# AmpR light up does not conclude their plasmid has no promoter. Under
# `regulatory` those twelve rows are invisible to that probe, so the app would
# have gone on saying "no promoter" after promoters were signed off: a
# user-facing claim made false by a schema choice nobody would connect to it.
#
# This schema has no column for `/regulatory_class`, so the choice is between a
# key that is current and says nothing, and a key that is retired and says what
# the feature is. The retired key wins because something real depends on it,
# every reader still accepts it, and a GenBank export from these rows is more
# informative for it. features/README.md records the trade rather than hiding
# it, and `absent_common_kinds`'s own probe list is the thing to keep in step if
# these keys are ever changed again.
# AN ITEM MAY NOT BE REMOVED FROM THIS TUPLE ONCE ITS ID IS PUBLISHED.
#
# `build()` allocates `PLF:{ID_BASE + i}` from the item's INDEX here, so deleting
# one renumbers every item after it. Measured 2026-08-11: dropping the CMV
# enhancer at index 6 moves the T7 terminator to PLF:4006, rrnB T1 to PLF:4007,
# rrnB T2 to PLF:4008, bGH poly(A) to PLF:4009 and SV40 early poly(A) to
# PLF:4010 -- five published ids, each now meaning a different element.
# `build.py` catches it (it re-reads the previous table and refuses to write when
# a published id changes meaning), so the failure is loud rather than silent, but
# it is still a failure and the rebuild does not complete.
#
# features/PROPOSED.md said the opposite until 2026-08-11 -- "IDs are allocated
# from where a row is declared, never from how many survived, so dropping one
# does not renumber anything after it". That is true of a row DROPPED BY A CHECK,
# which is what the five refused Class B elements are: they stay in this tuple,
# keep their index, and are re-measured every build. It is not true of an item
# deleted from the source. Withdrawing a published row therefore means keeping
# its declaration here and stopping it at the gate, not deleting it.
ITEMS: tuple[Convention, ...] = (
    Convention(
        name="T7 promoter",
        aliases=("T7 RNA polymerase promoter", "T7 class III promoter",
                 "phi10 promoter", "PT7"),
        genbank_key="promoter",
        cls="regulatory",
        anchor="V01146", anchor_version="V01146.1", lo=22887, hi=22903, strand="+",
        sequence="TAATACGACTCACTATA",
        exemplars=("AF053733", "KJ641600", "PV764404", "GQ421427"),
        description=(
            # "the 17 bp class III promoter" said, of an element whose primary
            # description this project has not read, that the class III promoter
            # IS 17 bp -- while this row's own caveat says the row is -17..-1
            # with +1 excluded, i.e. a part of something. The description is the
            # user-facing string and it now says which.
            "The 17 bp recognition element of the class III promoter of "
            "bacteriophage T7: the seventeen bases immediately upstream of a T7 "
            "transcription start, with the +1 base excluded. T7 RNA polymerase "
            "reads it as a single subunit with no sigma factor and no accessory "
            "protein, and the host polymerase cannot read it at all. That mutual "
            "blindness is what a T7 expression system is built on."
        ),
        caveat=(
            "WHY 17 AND NOT 18 OR 20. This row is -17 to -1 relative to the "
            "transcription start, and the +1 base is deliberately excluded. The "
            "anchor record settles where +1 is without anyone having to assert it: "
            "these seventeen bases occur seven times in the T7 genome, every one "
            "of the seven ends exactly one base before a coordinate the record "
            "annotates as a promoter, and in all seven the next base is G -- so the "
            "annotated point is +1 and the interval here is the promoter proper. "
            "Re-derived from the record by a second reader on 2026-08-11: 7 of 7. "
            "The copy cited is the one immediately upstream of the gene 10 capsid "
            "CDS, which is the phi10 promoter that expression vectors carry. "
            "A SECOND ARGUMENT FOR THE SAME 5' EDGE, measured on 2026-08-11 and "
            "not previously in this row: the anchor annotates SEVENTEEN T7 "
            "promoters, not seven, and aligned on their annotated points every "
            "column from -17 to -3 agrees in 14 of 17 records or better (nine of "
            "those columns in 17 of 17), while no column from -24 to -18 exceeds "
            "12 of 17. The conserved block and this row have the same 5' edge. "
            "RIVAL EXTENTS, NAMED, BECAUSE THE OFFSETS ABOVE DO NOT SHOW THEM. "
            "Until 2026-08-11 this note said a 20 nt convention 'is measured "
            "against this row in the witness offsets above'; all four offsets above "
            "are 5'+0/3'+0, so that sentence pointed a reader at evidence that is "
            "not there, and no 20 nt form was found anywhere. What was found, in "
            "records this stage names as exemplars of OTHER rows and which "
            "therefore cannot appear in this row's offsets: 19 nt at -17..+2 "
            "(MH325107.1, which the SnapGene screen flags) and 21 nt at -17..+4 "
            "(DQ250998.1, FJ457001.1) and at -18..+3 (AY640625.1). Every rival "
            "runs into the transcript; that is the decision this row takes and "
            "they do not. Note also that PLF:1004, the T7 RNA polymerase row, "
            "already describes this promoter as 17 bp, so a longer row here would "
            "have made this database disagree with itself. STILL OPEN, AND IT IS "
            "PROSE AND NOT SEQUENCE: the primary description of the class III "
            "promoter is Dunn & Studier 1983, J Mol Biol 166:477-535, PMID "
            "6864790, whose PubMed record carries no abstract. The length that "
            "paper gives for the element was NOT read from the paper here, so the "
            "17 in this row is this project's boundary and not a quotation."
        ),
    ),
    Convention(
        name="SP6 promoter",
        aliases=("SP6 RNA polymerase promoter", "PSP6"),
        genbank_key="promoter",
        cls="regulatory",
        anchor="AY288927", anchor_version="AY288927.2", lo=12542, hi=12558, strand="+",
        sequence="ATTTAGGTGACACTATA",
        exemplars=("DQ250998", "FJ457001", "KC800697"),
        description=(
            "The 17 bp promoter of Salmonella phage SP6, read by SP6 RNA polymerase "
            "and by no host enzyme. Bounded on the same rule as the T7 row, and by "
            "SP6's own published promoter consensus: the seventeen bases upstream "
            "of the transcription start. Paired with a "
            "T7 or T3 promoter at the other end of a polylinker it is what lets a "
            "vector transcribe either strand of an insert in vitro."
        ),
        caveat=(
            "THE ANCHOR'S FEATURE TABLE IS SILENT; THE ANCHOR'S OWN PAPER IS NOT. "
            "The SP6 genome record annotates genes and coding sequences and NO "
            "promoters at all, so unlike T7 there is no depositor-annotated "
            "transcription start to bound this interval against. These seventeen "
            "bases occur three times in that genome, identically, in three "
            "intergenic positions; the coordinate cited is the first of the three "
            "and the choice does not affect a single base of the row. Until "
            "2026-08-11 this row's basis stopped there and was read as bounded by "
            "analogy to T7. It is not. The record's OWN publication -- Dobbins, "
            "George, Basham, Ford, Houtz, Pedulla, Lawrence, Hatfull & Hendrix "
            "2004, J Bacteriol 186:1933-1944, PMID 15028677 -- identifies ten "
            "promoters for the SP6-encoded RNA polymerase in this genome and "
            "publishes the degenerate consensus KAWTTARGKGACACTATAG, which is 19 "
            "nt and runs -18..+1. Resolved (K=T, W=T, R=G) its -17..-1 window is "
            "this row base for base, checked on 2026-08-11. The one base further "
            "5' that the consensus carries is the degenerate K, and the three "
            "exact copies of this row in the anchor carry G, T and G there -- so "
            "-18 is the first column that varies, and 17 is where the invariant "
            "window ends. All three are followed by G at +1. Brown, Klement & "
            "McAllister 1986, Nucleic Acids Res 14:3521-3526, PMID 3010240, fixes "
            "the register independently: the core these phage promoters share, "
            "CACTA, runs -7 to -3, and in this row read as -17..-1 it does. So the "
            "boundary rests on the same articulated rule the T7 row uses -- the "
            "conserved window lying entirely upstream of +1 -- applied to primary "
            "evidence about SP6, and not on an analogy standing in for evidence. "
            "The residue of honest doubt is that both primary consensuses extend "
            "through +1, so 17 remains a choice; it is the same choice, made the "
            "same way, as the T7 row."
        ),
    ),
    Convention(
        name="lac promoter",
        aliases=("Plac", "lac operon promoter", "lacZ promoter"),
        genbank_key="promoter",
        cls="regulatory",
        anchor="J01636", anchor_version="J01636.1", lo=1210, hi=1239, strand="+",
        sequence="TTTACACTTTATGCTTCCGGCTCGTATGTT",
        exemplars=("HM126493", "KJ641600", "LT009443", "MH325107"),
        description=(
            "The promoter of the Escherichia coli lactose operon, from the -35 "
            "hexamer through the -10 hexamer. Read by sigma-70, blocked by LacI "
            "bound at the operator immediately downstream, and released by "
            "allolactose or IPTG. Weak on its own, which is why the strong "
            "expression promoters derived from it -- tac and trc -- replace its "
            "-35 hexamer with the trp one."
        ),
        caveat=(
            "THE RIVAL CONVENTION IS ARTICULABLE AND NEARLY THREE TIMES LONGER. "
            "A widely deposited 84 nt convention runs from the CAP site to the base "
            "before operator O1 -- that is, everything between the two things the "
            "anchor record itself does annotate, since it annotates the CAP site "
            "and O1 and no promoter. This row takes the narrower reading, the "
            "promoter proper, because the CAP site and the operator are separate "
            "elements with separate functions and belong in separate rows. A file "
            "labelled 'lac promoter' over the longer extent will match this row "
            "over part of its length, which is the correct outcome and not a miss. "
            "This is also NOT lacUV5: that allele differs inside the -10 hexamer."
        ),
    ),
    Convention(
        name="tac promoter",
        aliases=("Ptac", "trp-lac hybrid promoter"),
        genbank_key="promoter",
        cls="regulatory",
        anchor="MH488909", anchor_version="MH488909.1", lo=175, hi=203, strand="+",
        sequence="TTGACAATTAATCATCGGCTCGTATAATG",
        exemplars=("KM261834", "U78874", "MH488911"),
        description=(
            "A hybrid promoter carrying the -35 hexamer of the Escherichia coli trp "
            "promoter and the -10 hexamer of lacUV5, separated by a 16 bp spacer. "
            "Far stronger than either parent and still repressed by LacI at the "
            "downstream operator, so it stays IPTG-inducible."
        ),
        caveat=(
            "THE ANCHOR IS A CONSTRUCT, BECAUSE THERE IS NO NATURAL LOCUS. tac is "
            "a designed hybrid: it does not occur in the lac operon record, it does "
            "not occur in the trp operon, and the coordinates cited here are "
            "therefore a cloning vector's and not a primary locus's. "
            "ONE BASE FROM trc, AND THAT IS THE ARGUMENT FOR THE WHOLE INTERVAL. "
            "The trc row differs from this one by a single inserted C in the spacer "
            "and by nothing else. One depositor annotates 'tac promoter' not as an "
            "interval at all but as two separate features of six and seven bases, "
            "the two hexamers; a row built that way could not distinguish tac from "
            "trc, which is why this one spans -35 through -10 inclusive."
        ),
    ),
    Convention(
        name="trc promoter",
        aliases=("Ptrc",),
        genbank_key="promoter",
        cls="regulatory",
        anchor="U13872", anchor_version="U13872.1", lo=193, hi=222, strand="+",
        sequence="TTGACAATTAATCATCCGGCTCGTATAATG",
        exemplars=("LT727425", "U13859"),
        description=(
            "A hybrid promoter of the same design as tac -- the trp -35 hexamer "
            "with the lacUV5 -10 hexamer -- but with a 17 bp spacer rather than 16. "
            "The extra base restores the spacing sigma-70 prefers, and the two "
            "promoters are used interchangeably in practice."
        ),
        caveat=(
            "ONE BASE FROM tac. The single inserted C in the spacer is the entire "
            "difference between the two sequences and the entire difference between "
            "the two rows. Anything that matches this row at 29 of 30 bases is "
            "probably the other one. The anchor is the pTrc99A record, i.e. a "
            "construct record, for the same reason as the tac row: a designed "
            "promoter has no natural locus to be sliced from."
        ),
    ),
    Convention(
        name="CMV promoter",
        aliases=("hCMV-IE promoter", "CMV IE1 promoter", "PCMV",
                 "human cytomegalovirus immediate early promoter"),
        genbank_key="promoter",
        cls="regulatory",
        anchor="X17403", anchor_version="X17403.1", lo=173745, hi=173948, strand="-",
        sequence=(
            "GTGATGCGGTTTTGGCAGTACATCAATGGGCGTGGATAGCGGTTTGACTCACGGGGATTTCCAAGTCTCCACCCCAT"
            "TGACGTCAATGGGAGTTTGTTTTGGCACCAAAATCAACGGGACTTTCCAAAATGTCGTAACAACTCCGCCCCATTGA"
            "CGCAAATGGGCGGTAGGCGTGTACGGTGGGAGGTCTATATAAGCAGAGCT"),
        exemplars=("LC897329", "LT726933", "MH325107"),
        description=(
            "The proximal immediate-early promoter of human cytomegalovirus: the "
            "TATA box and the sequence between it and the IE transcription start. "
            "On its own this is an unremarkable promoter. What makes 'the CMV "
            "promoter' the default strong promoter of mammalian expression vectors "
            "is the enhancer immediately upstream of it, which is the next row."
        ),
        caveat=(
            "MOST MAPS' 'CMV PROMOTER' IS THIS ROW PLUS THE NEXT ONE. The element "
            "in pcDNA3-type vectors is a single 584 nt block, and that block is an "
            "exact slice of the cytomegalovirus genome: this row and the enhancer "
            "row are precisely its two halves, contiguous, with no base between "
            "them and none left over. Shipping the two as separate rows is the "
            "decision this database made, so that a map which labels only the "
            "enhancer and a map which labels the whole block both resolve to "
            "something true. It also means a file carrying the 584 nt block will "
            "match TWO rows, adjacently, and that is correct behaviour. Ship this "
            "row and the enhancer together or not at all; shipping one alone would "
            "silently pick a convention."
        ),
    ),
    Convention(
        name="CMV enhancer",
        aliases=("hCMV-IE enhancer", "human cytomegalovirus immediate early enhancer"),
        genbank_key="enhancer",
        cls="regulatory",
        anchor="X17403", anchor_version="X17403.1", lo=173949, hi=174328, strand="-",
        sequence=(
            "GACATTGATTATTGACTAGTTATTAATAGTAATCAATTACGGGGTCATTAGTTCATAGCCCATATATGGAGTTCCGC"
            "GTTACATAACTTACGGTAAATGGCCCGCCTGGCTGACCGCCCAACGACCCCCGCCCATTGACGTCAATAATGACGTA"
            "TGTTCCCATAGTAACGCCAATAGGGACTTTCCATTGACGTCAATGGGTGGAGTATTTACGGTAAACTGCCCACTTGG"
            "CAGTACATCAAGTGTATCATATGCCAAGTACGCCCCCTATTGACGTCAATGACGGTAAATGGCCCGCCTGGCATTAT"
            "GCCCAGTACATGACCTTATGGGACTTTCCTACTTGGCAGTACATCTACGTATTAGTCATCGCTATTACCATG"),
        exemplars=("LC897329", "OP697991", "MH325107"),
        description=(
            "The immediate-early enhancer of human cytomegalovirus: a tandem array "
            "of repeated binding sites for host transcription factors, directly "
            "upstream of and contiguous with the immediate-early promoter. It is "
            "the part that supplies the strength, and it works in most mammalian "
            "cell types, which is why it travels with the promoter into vectors."
        ),
        caveat=(
            "THE UPSTREAM HALF OF A 584 nt BLOCK WHOSE DOWNSTREAM HALF IS NOT IN "
            "THIS TABLE, AND THAT IS THIS ROW'S PROBLEM. In a pcDNA3-type vector "
            "the cytomegalovirus region is one contiguous 584 nt block. This row "
            "is its upstream 380 nt. The 204 nt immediately downstream -- the part "
            "carrying the TATA box, and the part most maps actually call the CMV "
            "promoter -- was PLF:4005, which the extent-corroboration rule refused "
            "on 2026-08-10 for want of a second independent submission drawing its "
            "edges. The condition this row shipped under, in that row's own words, "
            "was 'ship this row and the enhancer together or not at all; shipping "
            "one alone would silently pick a convention', and the table violates "
            "it today: a user annotating that block sees the upstream 380 nt light "
            "up and the promoter half stay dark. Db::absent_common_kinds does not "
            "rescue that, because it probes the literal key `promoter` and "
            "PLF:4000 and PLF:4001 supply it, so the app will not say 'no promoter "
            "is in this database yet'. WHAT THE EVIDENCE SAYS ABOUT THE SPLIT "
            "ITSELF, measured 2026-08-11. Of the six records this stage has "
            "fetched that contain these 380 bases, the three that draw the 380/204 "
            "split are the three the project has already identified as "
            "SnapGene-shaped: MH325107.1 carries the `label:` tell; LC897329.1 is "
            "SnapGene Common Feature naming from top to bottom with no tell; and "
            "OP697991.1's /note over this very interval is byte-identical to "
            "MH325107.1's but for the token 'label: ', which SOURCING.md section "
            "0.6 already records as a demonstrated false negative of the screen. "
            "The other three draw no enhancer at all: OR659033.1 annotates ONE 584 "
            "nt feature and calls it a CMV promoter, MN224159.1 one of 655 nt, "
            "MW987522.1 one of 623 nt. THE PRIMARY LITERATURE DOES NOT DRAW THIS "
            "EDGE EITHER. Boshart, Weber, Jahn, Dorsch-Haesler, Fleckenstein & "
            "Schaffner 1985, Cell 41:521-530, PMID 2985280, place the enhancer "
            "'between nucleotides -118 and -524' of the immediate-early "
            "transcription start. On the anchor's own numbering -- X17403.1 "
            "annotates exon complement(173610..173730), so +1 is 173730 -- this "
            "row is -219..-598 and the refused promoter row was -15..-218, so "
            "Boshart's -118 falls inside the promoter half and -524 inside the "
            "enhancer half. Neither of this row's edges is a Boshart edge; the "
            "split is a fragment boundary. RECOMMENDED TO THE CURATOR: WITHDRAW, "
            "or restore the promoter row before signing this one. That decision is "
            "a curator's and this note does not take it; features/PROPOSED.md sets "
            "out both options and what each costs. Finally, a 378 nt rival "
            "convention was asserted here until 2026-08-11 on the strength of a "
            "record that was neither named nor retained. It is not re-derivable "
            "from anything in this repository, so it has been removed rather than "
            "left standing."
        ),
    ),
    Convention(
        name="T7 terminator",
        aliases=("Tphi", "T7 transcription terminator", "T7Te"),
        genbank_key="terminator",
        cls="regulatory",
        anchor="V01146", anchor_version="V01146.1", lo=24164, hi=24210, strand="+",
        sequence="TAGCATAACCCCTTGGGGCCTCTAAACGGGTCTTGAGGGGTTTTTTG",
        exemplars=("AF525444", "PV764404", "KJ641600"),
        description=(
            "The Tphi transcription terminator of bacteriophage T7: a GC-rich "
            "hairpin followed by a run of thymines, which together stall T7 RNA "
            "polymerase. Placed downstream of the insert in T7 expression vectors "
            "so that transcription stops instead of running on round the plasmid."
        ),
        caveat=(
            "THE 3' EDGE IS PRIMARY; ONLY THE 5' EDGE IS A CONVENTION. Macdonald, "
            "Durbin, Dunn & McAllister 1994, J Mol Biol 238:145-158, PMID 8158645, "
            "define this signal as a stem-loop followed by a run of six uridylates "
            "where 'termination occurs at a 3' G residue just downstream of the U "
            "run'. In the anchor, 24201-24215 reads GGGTTTTTTGCTGAA, so that G is "
            "24210 -- this row's last base, and the single coordinate the anchor "
            "annotates as 'T7 transcription terminator Tphi'. The primary "
            "definition and the depositor's annotation land on the same base. The "
            "same paper says the 5' side is NOT delimited -- 'sequences upstream "
            "from the terminator have marked effects on the position and "
            "efficiency of termination' -- so nothing primary fixes where this "
            "element begins, and that is the one arbitrary edge here. THE RIVALS, "
            "RE-MEASURED 2026-08-11, AND THEY ARE NOT SYMMETRIC. Until then this "
            "note said two 48 nt forms are offset from each other by one base and "
            "'neither is wrong'. The offset is real; the symmetry is not. "
            "GQ421427.1's 48 nt shares this row's 3' edge and starts at the first "
            "base after the gene 10 stop codon, which the anchor puts at "
            "24160-24162 -- an articulable rule. AY303670.1's 48 nt starts at the "
            "LAST base of that stop codon and ends one base SHORT of the "
            "termination site, so it shares neither edge with the primary "
            "definition. Also measured: KJ641600.1 at 62 nt, same 3' edge; and "
            "KM261834.1's 253 nt 'terminator', which is not a rival extent but a "
            "synthetic composite -- it contains this row verbatim at its offset 2 "
            "and a one-base variant of the rrnB T1 row at its offset 60, and it "
            "stops nine bases into the rrnB T2 row rather than enclosing it. If a "
            "curator prefers 48 nt on the "
            "'start after the CDS' rule, that is defensible and would today have "
            "ONE exact placement, which is below this stage's floor of two. "
            "A claim that some deposit gives this name to a different part of the "
            "T7 genome stood here until 2026-08-11; no record checked shows it and "
            "none was ever named, so it has been removed."
        ),
    ),
    Convention(
        name="rrnB T1 terminator",
        aliases=("rrnB T1", "T1 terminator", "rrnBT1"),
        genbank_key="terminator",
        cls="regulatory",
        anchor="J01695", anchor_version="J01695.2", lo=6369, hi=6412, strand="+",
        sequence="ATAAAACGAAAGGCTCAGTCGAAAGACTGGGCCTTTCGTTTTAT",
        exemplars=("DQ115377", "EF216319", "U13872"),
        description=(
            "The first of the two tandem rho-independent terminators at the end of "
            "the Escherichia coli rrnB ribosomal RNA operon: a GC-rich stem-loop "
            "followed by a thymine run. Used downstream of a cloned gene to stop "
            "transcription, and upstream of a promoter to insulate it from "
            "read-through from the vector."
        ),
        caveat=(
            "THE NAME COMES FROM VECTOR RECORDS AND THE LOCUS FROM THE PRIMARY "
            "ONE, AND THE TRUE CLAIM IS NARROWER THAN THE ONE THAT STOOD HERE. "
            "What is true: J01695's FEATURE TABLE annotates no terminator -- the "
            "only feature between 5900 and 7200 is rRNA 6246..6365 -- so the "
            "coordinates come from the primary record and the name does not. What "
            "is NOT true, and was asserted here until 2026-08-11, is that nothing "
            "primary says 'T1'. Brosius 1984, Gene 27:161-172, PMID 6202587 -- the "
            "author of this operon's sequence and of the pKK vectors these extents "
            "come from -- reports that 'the putative rrnB terminators, T1 and T2, "
            "each function separately in vivo'. Orosz, Boros & Venetianer 1991, "
            "Eur J Biochem 201:653-659, PMID 1718749, which is in J01695's OWN "
            "reference list, subcloned them individually and concluded that 'T1 "
            "and T2 are both efficient terminators in isolated forms'. That is "
            "primary support for shipping T1 and T2 as two rows, which is what "
            "this row and the next do. Rfam still cannot help: SOURCING.md records "
            "as a confirmed negative that Rfam does not model standalone "
            "rho-independent terminators. THE RIVAL EXTENTS, RE-MEASURED "
            "2026-08-11. This note used to put them at 43 to 98 nt with 3' ends "
            "varying by about forty-five bases. Across every record this stage "
            "fetches that contains these bases there is no rival longer than 44 nt "
            "at all, and the 98 is not re-derivable from anything here. Five "
            "records place it at EXACTLY 44 -- DQ115377.1, EF216319.1, LT727425.1, "
            "U13859.1 and U13872.1 -- and the only 43 nt feature is one record "
            "disagreeing with itself: U13859.1 carries these bases three times and "
            "annotates all three 'rrnB T1', twice at 44 nt and once at 43, the 43 "
            "being this row without its leading A. That is a slip inside a single "
            "submission, not a competing convention. LT727425.1 is a further exact "
            "placement from an address this row does not yet cite; adding it to "
            "the exemplars would raise the corroboration count honestly."
        ),
    ),
    Convention(
        name="rrnB T2 terminator",
        aliases=("rrnB T2", "T2 terminator", "rrnBT2"),
        genbank_key="terminator",
        cls="regulatory",
        anchor="J01695", anchor_version="J01695.2", lo=6544, hi=6571, strand="+",
        sequence="AGAAGGCCATCCTGACGGATGGCCTTTT",
        exemplars=("LT739213", "U13859", "U13872"),
        description=(
            "The second of the two tandem terminators of the Escherichia coli rrnB "
            "operon, downstream of T1. Vectors that carry 'rrnB T1T2' carry both "
            "with the natural spacer between them; this row is T2 alone."
        ),
        caveat=(
            "THE ONLY ELEMENT IN THIS STAGE WITH NO COMPETING EXTENT. Every deposit "
            "found that annotates T2 separately encloses exactly these 28 bases. "
            "That is a fact about T2 and not a general reassurance about this "
            "stage: it is the shortest and most sharply bounded of the twelve. Note "
            "that 'rrnB T1T2' as one annotation is a THIRD element -- T1, the "
            "natural spacer, and T2 -- and is not this row and not the T1 row; a "
            "file carrying it will match both rows separately, with a gap. "
            "THE NAME HAS A PRIMARY SOURCE AND THIS ROW NOW CITES IT, which it did "
            "not before 2026-08-11: Orosz, Boros & Venetianer 1991, Eur J Biochem "
            "201:653-659, PMID 1718749 -- in J01695's own reference list -- "
            "subcloned T2 on its own and found it an efficient terminator, and "
            "Brosius 1984, Gene 27:161-172, PMID 6202587, reports that T1 and T2 "
            "'each function separately in vivo'. So the name rests on the primary "
            "literature and only the extent rests on the vector records."
        ),
    ),
    Convention(
        name="bGH poly(A) signal",
        aliases=("bGH polyA", "BGH poly(A) signal",
                 "bovine growth hormone polyadenylation signal"),
        genbank_key="polyA_signal",
        cls="regulatory",
        anchor="M57764", anchor_version="M57764.1", lo=2326, hi=2550, strand="+",
        sequence=(
            "CTGTGCCTTCTAGTTGCCAGCCATCTGTTGTTTGCCCCTCCCCCGTGCCTTCCTTGACCCTGGAAGGTGCCACTCCCA"
            "CTGTCCTTTCCTAATAAAATGAGGAAATTGCATCGCATTGTCTGAGTAGGTGTCATTCTATTCTGGGGGGTGGGGTGG"
            "GGCAGGACAGCAAGGGGGAGGATTGGGAAGACAATAGCAGGCATGCTGGGGATGCGGTGGGCTCTATGG"),
        exemplars=("LC897329", "MN224159", "OR659033", "MN811118"),
        description=(
            "The polyadenylation signal of the bovine growth hormone gene: the "
            "AATAAA hexamer together with enough flanking sequence to include the "
            "downstream GT-rich element, which cleavage and polyadenylation need as "
            "much as the hexamer itself. The standard 3' element of mammalian "
            "expression vectors."
        ),
        caveat=(
            "THE BEST-CONVERGED ELEMENT IN THIS STAGE, and the witness count above "
            "is the evidence for that sentence rather than this sentence being the "
            "evidence. The rival extents that do exist add or remove a few bases at "
            "the 5' end only. A poly(A) signal is one of the few Class B elements "
            "where a short row would be actively wrong: the hexamer alone is six "
            "bases and occurs by chance in any plasmid. THE ANCHOR LOCATES THE "
            "CLEAVAGE SITE, WHICH TURNS 'ENOUGH FLANKING SEQUENCE' INTO A "
            "MEASUREMENT, and this row did not say so before 2026-08-11. AATAAA "
            "occurs exactly once in these 225 bases, at row position 91 = M57764.1 "
            "2416-2421; the anchor's own exon 2138..2439 puts the cleavage and "
            "polyadenylation site at 2439, eighteen bases after the hexamer ends, "
            "and this row runs 111 further bases past it. Goodwin & Rottman 1992, "
            "J Biol Chem 267:16330-16334, PMID 1644817, found that 'a region from "
            "18 to 27 nucleotides downstream of the cleavage site contains "
            "sequences required for correctly positioning the cleavage site' -- "
            "that is 2457-2466, inside this row with 84 bases to spare -- and that "
            "the efficiency element here is 'diffuse ... rather than a discrete "
            "element', which is exactly why there is no sharp 3' edge to find. The "
            "anchor's own publication is Gordon, Quick, Erwin, Donelson & Maurer "
            "1983, Mol Cell Endocrinol 33:81-95, PMID 6357899."
        ),
    ),
    Convention(
        name="SV40 early poly(A) signal",
        aliases=("SV40 polyA", "SV40 poly(A) signal", "SV40 early polyadenylation signal"),
        genbank_key="polyA_signal",
        cls="regulatory",
        anchor="J02400", anchor_version="J02400.1", lo=2594, hi=2668, strand="-",
        sequence=(
            "AACTTGTTTATTGCAGCTTATAATGGTTACAAATAAAGCAATAGCATCACAAATTTCACAAATAAAGCATTTTTT"),
        exemplars=("LT009443", "AY640625", "LT726828", "MH325107"),
        description=(
            "The polyadenylation signal of the simian virus 40 EARLY transcription "
            "unit, on the early strand. Small, well characterised and old, which is "
            "why it is the poly(A) signal of choice where space matters -- typically "
            "on the selection-marker cassette of a mammalian vector rather than on "
            "the gene of interest."
        ),
        caveat=(
            "EARLY AND LATE ARE THE SAME BASES ON OPPOSITE STRANDS, AND A ROW CALLED "
            "'SV40 poly(A)' WOULD BE WRONG WHICHEVER STRAND IT TOOK. The anchor "
            "record annotates two polyadenylation hexamers inside this interval on "
            "the strand this row is on, and a third on the other strand; deposits "
            "in the wild annotate this identical span twice, once as 'SV40 polyA "
            "early' on one strand and once as 'SV40 polyA late' on the other. This "
            "row is the EARLY signal and says so in its name. Which strand is early "
            "was not assumed: the anchor record places the early coding sequences on "
            "this strand, ending just upstream of this interval. The late signal is "
            "not in this database and would be a separate row."
        ),
    ),
)


# Worked up, checked, and NOT rows. Each line is a refusal with its reason, and
# the list is here rather than in a commit message because the next person to
# want one of these should not have to rediscover why it is missing.
#
# SOURCING.md section 6 budgets about forty Class B rows. Twelve is what
# survived the two-independent-submissions rule and the nested-rivals rule
# applied honestly, and saying so is worth more than twelve more rows.
HELD: tuple[tuple[str, str], ...] = (
    ("T3 promoter",
     "No consensus to record. The two leading conventions are 17 nt and 19 nt, "
     "they are OFFSET rather than nested -- they share a 16 nt core and neither "
     "contains the other -- and each has exactly one independent submission "
     "behind it. Picking one would be a coin toss dressed up as "
     "consensus_of_insdc. A third deposit annotates the reverse complement, i.e. "
     "it got the strand wrong. The bases are unambiguous; the boundary is not."),
    ("SV40 early promoter",
     "The 330 nt convention is a contiguous circular interval that WRAPS the "
     "numbering origin of the SV40 record, which this schema's "
     "accession:lo-hi:strand boundary_evidence cannot express, and a rival 283 nt "
     "form does not place as a single interval at all. The region also carries a "
     "tandem repeat, so the two forms may differ in repeat copy number -- that was "
     "NOT counted and is not offered as a finding."),
    ("U6 promoter (human)",
     "The cleanest anchor of everything examined -- 249 nt, exact in the primary "
     "record for human U6 -- and only ONE independent submission witnesses it. "
     "Fails on witnesses, not on evidence. The cheapest of these to rescue."),
    ("H1 promoter (human)",
     "Three independent submissions agree on 216 nt, which is a genuine consensus, "
     "but the sequence does not occur in the human H1 RNA record and no genomic "
     "record carrying the upstream promoter could be located. Boundary witnessed, "
     "provenance absent; a row would have nothing to put in boundary_evidence."),
    ("EF-1alpha promoter (human)",
     "The 1144 nt vector element is the primary record's 1148 nt MINUS a four-base "
     "internal deletion, with both flanks exact. It is therefore not a verbatim "
     "slice of anything: a reference taken from the gene will not match real "
     "vectors, and one taken from a vector cannot be cited to the gene by "
     "coordinates. Needs an explicit decision about which sequence ships."),
    ("PGK promoter (mouse)",
     "Same shape as EF-1alpha and worse: exact for 67 nt, then a single-base shift, "
     "and roughly 48 substitutions across the rest. One submission. Two independent "
     "reasons to hold."),
    ("CAG promoter",
     "NOT ONE ELEMENT. The widely deposited 1342 nt 'AG promoter' begins in the "
     "chicken beta-actin gene and ends in rabbit beta-globin and contains no "
     "cytomegalovirus sequence at all; a 935 nt 'CAG promoter' begins in the "
     "cytomegalovirus enhancer. Merging two different elements under one "
     "near-identical name would be the worst error available in this set."),
    ("araBAD / pBAD promoter",
     "Three extents from three submissions, two of them SnapGene-annotated, and no "
     "Escherichia coli ara locus record fetched to anchor any of them. Insufficient "
     "on both legs. Note PLF:1002 already carries araC, the regulator; the promoter "
     "it works on is the hole."),
    ("tetO / TRE / Ptet",
     "DROPPED, not held. The name covers at least four unrelated elements -- a "
     "bacterial PLtetO-1, a bacterial pTet, a mammalian bidirectional TRE, and a "
     "CMV-tetO2 hybrid -- with nothing in common but the word. It must be split "
     "into separately named rows before any part of it can be sourced at all."),
)


# --------------------------------------------------------------------------
# ENA, narrowly


def _guard(url: str) -> None:
    """This stage may ask ENA for a FASTA region or an EMBL record, and nothing
    else. `build.fetch` already refuses any host SOURCING.md section 1 has not
    cleared; this is the narrower rule, so that no future edit here can reach a
    browser endpoint that serves something other than a record."""
    if not (url.startswith(ENA_BASE + "/fasta/") or url.startswith(ENA_BASE + "/embl/")):
        raise SystemExit(f"stage_classb may only fetch ENA records: {url}")


COMPLEMENT = str.maketrans("ACGTRYSWKMBDHVN", "TGCAYRSWMKVHDBN")


def revcomp(s: str) -> str:
    return s.translate(COMPLEMENT)[::-1]


def ena_region(acc: str, lo: int, hi: int, refresh: bool) -> tuple[str, str, dict]:
    """(header, bases, cache metadata) for exactly `acc:lo-hi`."""
    url = f"{ENA_BASE}/fasta/{acc}?range={lo}-{hi}"
    _guard(url)
    name = f"classb_region_{acc}_{lo}_{hi}.fa"
    text = fetch(url, name, refresh).decode("utf8", "replace")
    records = list(parse_fasta(text))
    if not records:
        raise ValueError(f"ENA returned no FASTA record for {acc}:{lo}-{hi}")
    # Find the record actually asked for rather than taking the first: asking
    # for a WGS contig returns the whole WGS set. stage_rfam learned this the
    # expensive way and the note is repeated because the failure is silent.
    stem = acc.split(".")[0]
    match = [(h, s) for h, s in records if stem in h] or records[:1]
    header, body = match[0]
    got = body.upper().replace("U", "T")
    # ENA silently ignores `range=` on some records and serves the whole thing.
    # Its own header says so when it honoured the request, so check rather than
    # hope, and slice locally when it did not.
    if f"Location:{lo}..{hi}" not in header.replace(" ", "") and len(got) >= hi:
        got = got[lo - 1:hi]
        header = header.strip() + f" [range= ignored by ENA; {lo}..{hi} taken locally]"
    return header.strip(), got, cached_meta(name)


FT_LINE = re.compile(r"^FT {3}(\S+) {2,}(\S.*)$")
FT_CONT = re.compile(r"^FT {19}(\S.*)$")
# A location this stage can turn into an interval: `1..10`, `complement(1..10)`,
# or a BARE COORDINATE. The bare form is not an edge case here, it is the most
# informative thing several anchors have to say: V01146 annotates all 26 of its
# phage promoters as single points, and a pattern requiring `..` silently
# reported "the record annotates nothing near this interval" for the T7 promoter
# row -- whose entire boundary argument rests on where that point is.
SIMPLE_LOC = re.compile(r"^(complement\()?(\d+)(?:\.\.(\d+))?\)?$")


@dataclass
class Record:
    accession: str
    """Versioned, off the record's own ID line."""
    length: int
    sequence: str
    submitter: str
    """The address on the record's own submission reference, normalised. Empty
    if the record carries no `Submitted (...) to the INSDC` reference."""
    snapgene: bool
    """Does the feature table carry SnapGene's `label:` tell? See finding 1."""
    features: list = field(default_factory=list)
    """(key, lo, hi, strand) for the regulatory-ish keys only."""


REGULATORY_KEYS = {
    "promoter", "terminator", "regulatory", "enhancer", "polyA_signal",
    "misc_signal", "misc_feature", "protein_bind", "TATA_signal", "minus_35_signal",
    "minus_10_signal", "RBS", "polyA_site", "prim_transcript",
}


def parse_embl(text: str) -> Record:
    """Everything this stage reads out of a record, and nothing else.

    Reads: the ID line, the submission address, the SQ block, and the
    coordinates of regulatory-ish features. Does NOT read `/note`, `/label`,
    `/standard_name`, `/gene` or `/product` -- see finding 1. That is not
    fastidiousness: a `/note` on one of these records can be SnapGene's own
    Description column, and this stage's whole defensibility rests on never
    having read it.
    """
    accession, length, snap = "", 0, False
    submitter_lines: list[str] = []
    in_sub, seq_lines, in_seq = False, [], False
    features: list = []
    cur_key, cur_loc, loc_done = None, "", True

    def flush() -> None:
        nonlocal cur_key, cur_loc
        if cur_key and cur_key in REGULATORY_KEYS:
            m = SIMPLE_LOC.match(cur_loc.replace(" ", ""))
            if m:
                lo = int(m.group(2))
                features.append(
                    (cur_key, lo, int(m.group(3)) if m.group(3) else lo,
                     "-" if m.group(1) else "+")
                )
            # A join()/order() location is skipped on purpose rather than
            # approximated: an element spanning a circular origin has no single
            # (lo, hi), and inventing one would be a fabricated boundary in the
            # very file that exists to avoid them.
        cur_key, cur_loc = None, ""

    for ln in text.splitlines():
        if ln.startswith("ID   "):
            parts = ln[5:].split(";")
            accession = parts[0].strip()
            # VERSIONED, from the same line's `SV` field, because an
            # accession without one is not a citation: J01695 is at SV 2 and
            # J01636 at SV 1, and a boundary_evidence string reading `J01695`
            # would point at whatever ENA serves under that name next year. The
            # ID line spells the version separately from the accession, which is
            # why this has to be reassembled rather than read.
            sv = re.search(r";\s*SV\s+(\d+)", ln)
            if sv:
                accession = f"{accession}.{sv.group(1)}"
            m = re.search(r"(\d+)\s*BP\.", ln)
            length = int(m.group(1)) if m else 0
        elif ln.startswith("RL   "):
            body = ln[5:].strip()
            if body.startswith("Submitted"):
                in_sub = True
                submitter_lines.append(re.sub(r"^Submitted \([^)]*\) to [^.]*\.\s*", "", body))
            elif in_sub:
                submitter_lines.append(body)
        elif ln.startswith("RN   ") or ln.startswith("XX"):
            in_sub = False
        elif ln.startswith("FT"):
            m = FT_LINE.match(ln)
            if m:
                flush()
                cur_key, cur_loc, loc_done = m.group(1), m.group(2).strip(), False
                continue
            c = FT_CONT.match(ln)
            if c and cur_key is not None:
                body = c.group(1)
                if body.startswith("/"):
                    loc_done = True
                    if re.search(r"\blabel:", body):
                        snap = True
                elif not loc_done:
                    cur_loc += body.strip()
        elif ln.startswith("SQ   "):
            flush()
            in_seq = True
        elif ln.startswith("//"):
            in_seq = False
        elif in_seq:
            seq_lines.append(re.sub(r"[^A-Za-z]", "", ln))
    flush()

    return Record(
        accession=accession,
        length=length,
        sequence="".join(seq_lines).upper().replace("U", "T"),
        submitter=re.sub(r"\s+", " ", " ".join(submitter_lines)).strip().casefold(),
        snapgene=snap,
        features=features,
    )


def record_is_snapgene_annotated(embl_text: str) -> bool:
    """Does this record's feature table carry SnapGene's `label:` tell?

    Named and exported because `INSDC_POSTURE` above points at it, and
    `features/build/insdc_posture.py` DRIVES it -- against a record carrying the
    tell and one without -- rather than taking the declaration's word for it.
    The screen is therefore not a comment claiming a thing is checked; it is a
    function some other program can prove still works.

    It is the whole record that goes in, deliberately, so what the gate exercises
    is the parser this stage actually runs and not a regex lifted out of it. A
    change to the continuation-line handling that stopped the tell being seen
    would be invisible to a regex-level test and is caught by this one.
    """
    return parse_embl(embl_text).snapgene


def ena_record(acc: str, refresh: bool) -> tuple[Record, dict]:
    url = f"{ENA_BASE}/embl/{acc}"
    _guard(url)
    name = f"classb_embl_{acc}.embl"
    rec = parse_embl(fetch(url, name, refresh).decode("utf8", "replace"))
    if rec.length and len(rec.sequence) != rec.length:
        raise ValueError(
            f"{acc}: ID line declares {rec.length} BP, the SQ block holds "
            f"{len(rec.sequence)} -- two halves of one record disagree"
        )
    return rec, cached_meta(name)


def occurrences(haystack: str, needle: str) -> list:
    """Every 1-based (lo, hi, strand) of `needle` in `haystack`, both strands.

    Both strands because of finding 3: a depositor's own location expression is
    not reliable about strand, so the strand this row is found on is measured
    rather than taken from anybody's annotation.
    """
    out = []
    for strand, pat in (("+", needle), ("-", revcomp(needle))):
        i = haystack.find(pat)
        while i != -1:
            out.append((i + 1, i + len(pat), strand))
            i = haystack.find(pat, i + 1)
    return sorted(out)


def overlaps(a_lo: int, a_hi: int, b_lo: int, b_hi: int) -> bool:
    return a_lo <= b_hi and b_lo <= a_hi


SUBMITTER_MERGE = 0.6
"""Token-containment above which two submission addresses are treated as ONE.

Deliberately fuzzy, and deliberately biased towards merging. The same lab
writes its own address differently on different deposits -- measured, on two
records used here as SP6 witnesses: "institute of molecular biology, university
of oregon, 355 streisinger hall, eugene, or 97403-1229, usa" and "institute of
molecular biology, university of oregon, eugene, or 97403, usa". Exact string
equality would call those two independent submissions, which is the ONE error
this stage must not make, because "two independent exemplars" is the whole of
SOURCING.md's prescribed method for Class B. Merging too eagerly can only lower
the counted independence and so can only make the gate harder to pass; merging
too little inflates it. When a threshold has to be wrong, it should be wrong in
the direction that refuses rows.
"""


def _addr_tokens(addr: str) -> set:
    return {t for t in re.split(r"[^a-z0-9]+", addr.casefold())
            if len(t) > 2 and not t.isdigit()}


def same_submitter(a: str, b: str) -> bool:
    ta, tb = _addr_tokens(a), _addr_tokens(b)
    if not ta or not tb:
        return a == b
    return len(ta & tb) / min(len(ta), len(tb)) >= SUBMITTER_MERGE


ANCHOR_WINDOW = 80
"""How far either side of the shipped interval to report the anchor record's own
annotation. Wide enough to catch a transcription-start POINT sitting one base
outside the interval -- which is the single most informative thing several of
these records have to say -- and narrow enough not to drag in the neighbouring
gene."""


def corroborating_submissions(submitters: dict) -> dict:
    """Of the independent submissions, the ones that drew the edges where we did.

    Split out of `verify()` so `self_test()` can drive it: inside the loop it was
    reachable only with the network or the cache, which is the condition under
    which a rule quietly stops being tested. One submission may hold several
    records and only some of them place the feature exactly; the SUBMISSION
    counts once, because two records from one lab are one opinion -- the same
    reading of "independent" that `MIN_SUBMISSIONS` already uses.
    """
    out = {key: [e for e in group if e.get("exact")] for key, group in submitters.items()}
    return {key: group for key, group in out.items() if group}


def relate(ours: tuple, theirs: tuple, key: str, our_strand: str,
           their_strand: str = "") -> str:
    """Describe one anchor-record feature relative to the shipped interval.

    A point is described as a point. Three of these anchors annotate the
    element's transcription start or terminator site as a single coordinate and
    not as an interval, and flattening that into `N..N` would hide the best
    evidence in the file that the interval is a convention somebody chose.
    """
    o_lo, o_hi = ours
    t_lo, t_hi = theirs
    # Their strand matters and ours does not describe it. The SV40 poly(A) row
    # turns on this: the anchor annotates two hexamers on the row's own strand
    # and a third on the opposite one, and the third is the LATE signal over the
    # same bases. Printing three anonymous intervals would lose the finding.
    on = f" on the {their_strand} strand" if their_strand else ""
    if t_lo == t_hi:
        if t_lo < o_lo:
            d, side = o_lo - t_lo, "5'" if our_strand == "+" else "3'"
        elif t_lo > o_hi:
            d, side = t_lo - o_hi, "3'" if our_strand == "+" else "5'"
        else:
            return f"{key} as a POINT at {t_lo}{on}, inside this interval"
        return f"{key} as a POINT at {t_lo}{on}, {d} base(s) {side} of this interval"
    return f"{key} {t_lo}-{t_hi}{on} ({offsets(ours, theirs, our_strand)})"


def offsets(ours: tuple, theirs: tuple, our_strand: str) -> str:
    """How far the depositor's edges sit from ours, in the ELEMENT's orientation.

    Reported in the element's own 5'->3' sense, not in the record's coordinate
    sense, because "this depositor starts three bases earlier" is the statement
    a curator needs and it inverts on the minus strand. A positive number means
    the depositor's feature extends FURTHER than this row's in that direction.
    """
    o_lo, o_hi = ours
    t_lo, t_hi = theirs
    left, right = o_lo - t_lo, t_hi - o_hi
    if our_strand == "-":
        left, right = right, left
    return f"5'{left:+d}/3'{right:+d}"


# --------------------------------------------------------------------------
# The stage


def verify(item: Convention, refresh: bool) -> tuple[dict, list]:
    """Everything this row asserts, checked against ENA. Raises on any failure.

    Returns (evidence, report lines). Nothing is smoothed over: a row whose
    coordinates no longer hold its sequence, or which cannot find two
    independent untainted witnesses, raises and is dropped by `build()` with the
    reason printed.
    """
    lines = []
    header, got, region_meta = ena_region(item.anchor, item.lo, item.hi, refresh)
    want = item.sequence if item.strand == "+" else revcomp(item.sequence)
    if got != want:
        raise ValueError(
            f"{item.anchor}:{item.lo}-{item.hi} does not hold the bases this row would "
            f"ship ({len(got)} nt read, {len(want)} nt expected on the {item.strand} "
            f"strand). Refusing to publish a boundary the depositor's own record "
            f"contradicts"
        )
    if len(item.sequence) != item.hi - item.lo + 1:
        raise ValueError(
            f"declared sequence is {len(item.sequence)} nt but {item.lo}-{item.hi} spans "
            f"{item.hi - item.lo + 1}"
        )
    lines.append(
        f"    anchor  {item.anchor}:{item.lo}-{item.hi}:{item.strand} "
        f"{len(item.sequence)} nt re-sliced and identical"
    )

    # The whole anchor record, for three things the region fetch cannot give:
    # a second view of the same bases, how many times the element occurs in its
    # own source, and what the record's own feature table says about the locus.
    arec, arec_meta = ena_record(item.anchor, refresh)
    if arec.accession != item.anchor_version:
        raise ValueError(
            f"{item.anchor} is now {arec.accession or '(no ID line)'}, and this row was "
            f"built against {item.anchor_version}. A re-version moves coordinates "
            f"without changing an accession, so the interval below can no longer be "
            f"trusted to mean what it meant"
        )
    if arec.sequence[item.lo - 1:item.hi] != got:
        raise ValueError(
            f"{item.anchor}: the FASTA region and the EMBL flat file of the same "
            f"record disagree over {item.lo}-{item.hi}. One of the two views is "
            f"being served wrong and there is no way to tell which from here"
        )
    anchor_hits = occurrences(arec.sequence, item.sequence)
    anchor_near = [
        relate((item.lo, item.hi), (flo, fhi), k, item.strand, s)
        for (k, flo, fhi, s) in arec.features
        if flo <= item.hi + ANCHOR_WINDOW and fhi >= item.lo - ANCHOR_WINDOW
    ]
    lines.append(
        f"    anchor  occurs {len(anchor_hits)}x in {arec.accession}; record annotates "
        + ("; ".join(anchor_near) if anchor_near else "nothing within "
           f"{ANCHOR_WINDOW} nt of this interval")
    )

    # THE ANCHOR IS A WITNESS TOO, and getting that wrong nearly cost a row.
    # SOURCING.md asks for "two independent GenBank exemplars", and the record
    # the bases are sliced from is a GenBank record like any other -- for the
    # designed promoters (tac, trc) it is a cloning vector whose depositor
    # annotated the element, i.e. exactly the thing being counted. Excluding it
    # made the trc row fail with one witness while two independent depositors
    # plainly held the sequence. What the anchor does NOT automatically supply
    # is a PLACEMENT: J01695 holds rrnB T1 and T2 and annotates no terminator at
    # all, so its contribution is measured by the same code as everyone else's
    # and comes out as "no regulatory feature of that record overlaps it".
    witnesses, submitters, tainted, absent = [], {}, [], []
    for acc in (item.anchor, *item.exemplars):
        rec, meta = ena_record(acc, refresh)
        if acc == item.anchor and rec.snapgene:
            # Does not fire on any current row -- every anchor here is a primary
            # genome or an old vector record. It is a gate against a future edit
            # pinning the reference sequence itself to a SnapGene-annotated
            # deposit, which would put their convention at the root of the row
            # rather than merely among its witnesses.
            raise ValueError(
                f"anchor {acc} is a SnapGene-annotated record; the bases a Class B "
                f"row ships must not be sliced out of one"
            )
        hits = occurrences(rec.sequence, item.sequence)
        if not hits:
            absent.append(acc)
            lines.append(f"    exemplar {acc:12s} DOES NOT CONTAIN the sequence -- not counted")
            continue
        lo, hi, strand = hits[0]
        # Deliberately NOT named `near`: an earlier version of this function used
        # that name for the anchor's features as well and the exemplar loop
        # silently overwrote them.
        near_feats = [
            (k, flo, fhi) for (k, flo, fhi, _s) in rec.features
            if overlaps(lo, hi, flo, fhi) and (fhi - flo + 1) <= 12 * len(item.sequence)
        ]
        placed = ", ".join(
            f"{k} {offsets((lo, hi), (flo, fhi), strand)}" for k, flo, fhi in near_feats[:4]
        ) or "nothing over them at all"
        # The corroboration test, computed from the intervals rather than from
        # the string above -- `placed` is truncated to four features for the
        # report, and a fifth feature landing exactly on our edges would be
        # invisible to anything that parsed it. Equality of the raw interval IS
        # `5'+0/3'+0`; offsets() is only how it is rendered.
        exact = [(k, flo, fhi) for k, flo, fhi in near_feats if (flo, fhi) == (lo, hi)]
        entry = {
            "accession": rec.accession or acc,
            "is_anchor": acc == item.anchor,
            "lo": lo, "hi": hi, "strand": strand,
            "snapgene": rec.snapgene,
            "submitter": rec.submitter,
            "placed": placed,
            "exact": [k for k, _lo, _hi in exact],
            "occurrences": len(hits),
            "meta": meta,
        }
        witnesses.append(entry)
        if rec.snapgene:
            tainted.append(rec.accession or acc)
            lines.append(
                f"    exemplar {acc:12s} contains it, but the record is "
                f"SnapGene-annotated -- NOT counted as independent"
            )
            continue
        # A record with no `Submitted (...) to the INSDC` reference -- which is
        # every one of the old primary anchors here -- is keyed on its own
        # accession rather than merged with every other addressless record. Two
        # addressless records would then count as two submissions, which is the
        # inflation this whole mechanism exists to prevent, so `notes` says how
        # many witnesses were counted that way and a curator can weigh it.
        addr = rec.submitter or f"no submission address, counted as {acc}"
        key = next((k for k in submitters if same_submitter(k, addr)), addr)
        merged = key != addr
        entry["anonymous"] = not rec.submitter
        submitters.setdefault(key, []).append(entry)
        lines.append(
            f"    {'anchor  ' if acc == item.anchor else 'exemplar'} {acc:12s} "
            f"{rec.accession:12s} at {lo}-{hi}:{strand} "
            f"x{len(hits)}; depositor places {placed}"
            + ("  [same submitter as an earlier record]" if merged else "")
            + ("  [no submission address]" if not rec.submitter else "")
        )

    if len(submitters) < MIN_SUBMISSIONS:
        raise ValueError(
            f"{len(submitters)} independent submission(s) witness this sequence, "
            f"SOURCING.md section 6 requires {MIN_SUBMISSIONS}. "
            f"{len(tainted)} further record(s) contain it but are SnapGene-annotated "
            f"and a SnapGene-annotated record is not an independent witness of a "
            f"SnapGene convention. Absent from: {absent or 'none'}"
        )

    # THE SECOND HALF OF SOURCING.md SECTION 4'S SENTENCE. The count above is of
    # submissions that hold the BASES; this one is of submissions that draw the
    # EDGES where this row draws them. They are different numbers and only the
    # second one is about a boundary. See MIN_PLACEMENTS at the top of this file
    # for why it is not a taint check and must not be called one.
    corroborating = corroborating_submissions(submitters)
    for key, group in corroborating.items():
        for e in group:
            lines.append(
                f"    PLACES IT EXACTLY  {e['accession']:12s} "
                f"{'/'.join(e['exact'])} at {e['lo']}-{e['hi']}"
            )
    if len(corroborating) < MIN_PLACEMENTS:
        near = "; ".join(
            f"{e['accession']} places {e['placed']}"
            for g in submitters.values() for e in g
        )
        raise ValueError(
            f"{len(corroborating)} independent submission(s) annotate a feature at "
            f"EXACTLY this extent, and {MIN_PLACEMENTS} are required. "
            f"{len(submitters)} submission(s) hold the bases, which is a fact about the "
            f"sequence and not about the boundary -- SOURCING.md section 4 asks for two "
            f"exemplars 'showing where depositors actually place it'. On this evidence "
            f"`boundary_rule = consensus_of_insdc` would be false on its face: the "
            f"extent is one lab's opinion. Where they actually put the edges: {near}. "
            f"The remedy is a curator's, not a program's -- cite more evidence, or ship "
            f"an extent the evidence corroborates."
        )

    return (
        {
            "header": header,
            "region_meta": region_meta,
            "anchor_record": arec,
            "anchor_meta": arec_meta,
            "anchor_hits": anchor_hits,
            "anchor_near": anchor_near,
            "witnesses": witnesses,
            "submitters": submitters,
            "corroborating": corroborating,
            "tainted": tainted,
            "absent": absent,
        },
        lines,
    )


def notes_for(item: Convention, ev: dict) -> str:
    """The measured part of `notes`. Every number here is computed this build."""
    subs = ev["submitters"]
    placed = "; ".join(
        f"{e['accession']} places {e['placed']}"
        for group in subs.values() for e in group
    )
    anon = sum(1 for g in subs.values() for e in g if e["anonymous"])
    arec = ev["anchor_record"]
    out = (
        f"CLASS B: the boundary is a CONVENTION, not a measurement, and this row "
        f"records which convention it chose. Bases re-fetched and re-sliced from ENA "
        f"{arec.accession} {item.lo}-{item.hi} on the {item.strand} strand this build: "
        f"{len(item.sequence)} nt, identical to the sequence in the allow-list, and the "
        f"record's FASTA and flat-file views of that interval agree. These bases occur "
        # SAY WHICH KEYS WERE LOOKED AT, because "none" is otherwise read as
        # "the record says nothing here" and that is a stronger claim than was
        # measured. parse_embl() keeps REGULATORY_KEYS and nothing else, so a
        # CDS, gene or exon over the interval is invisible to this sentence and
        # was never counted. It is not hypothetical: X17403.1 annotates
        # `CDS complement(173505..>173909)` across PLF:4005's interval and
        # `exon complement(173610..173730)` just outside it, and this note said
        # "none". The row is still right -- the CDS is not a rival promoter
        # boundary -- but a curator reading "none" would have been told the
        # region was bare, which it is not.
        f"{len(ev['anchor_hits'])} time(s) in the anchor record. ANCHOR RECORD'S OWN "
        f"ANNOTATION within {ANCHOR_WINDOW} nt of this interval, counting only the "
        f"regulatory-type feature keys this stage reads and therefore silent about "
        f"any CDS, gene or exon there: "
        + ("; ".join(ev["anchor_near"]) or "none") + ". "
    )
    out += (
        f"WITNESSES: {len(subs)} independent submitting address(es) over "
        f"{sum(len(g) for g in subs.values())} record(s) contain these exact bases, "
        f"the anchor among them. Addresses are compared fuzzily and merged when they "
        f"look like one lab writing its address twice, so this count is a floor and "
        f"not a headline"
        + (f"; {anon} of them carry no submission reference at all and are counted "
           f"on their own accession. " if anon else ". ")
        + f"Where those depositors put the edges, measured on this build in the "
        f"element's own 5'-to-3' sense and signed so that a positive number means "
        f"the depositor's feature extends further than this row's: {placed}. "
    )
    corr = ev["corroborating"]
    # Grouped by SUBMISSION and not by record, because the number in front of it
    # counts submissions: a flat list of three accessions beside the number 2
    # reads as an arithmetic slip rather than as two labs, one of which
    # deposited twice.
    exactly = "; ".join(
        " and ".join("{} ({})".format(e["accession"], "/".join(e["exact"])) for e in group)
        + (" -- one submitting address" if len(group) > 1 else "")
        for group in corr.values()
    )
    out += (
        f"CORROBORATION OF THE EXTENT ITSELF, which is a different measurement from the "
        f"one above and is the one `boundary_rule = consensus_of_insdc` actually rests "
        f"on: {len(corr)} of those {len(subs)} independent submission(s) annotate a "
        f"feature at EXACTLY this extent, edge for edge, against a floor of "
        f"{MIN_PLACEMENTS}. They are: {exactly}. Holding the bases is a fact about the "
        f"sequence; drawing the same edges is the only thing that makes the word "
        f"consensus true, and until 2026-08-10 this stage tested the first and not the "
        f"second. This is NOT a taint check and cannot show where anybody's convention "
        f"came from -- it says only whether this project's own evidence forced this "
        f"extent. "
    )
    if ev["tainted"]:
        out += (
            f"NOT COUNTED: {len(ev['tainted'])} further record(s) contain the bases but "
            f"carry SnapGene's 'label:' tell in their feature table "
            f"({', '.join(ev['tainted'])}); a SnapGene-annotated deposit is not an "
            f"independent witness of a SnapGene convention, and the CI taint gate "
            f"cannot see a coordinate arriving this way. "
        )
    if ev["absent"]:
        out += (
            f"CHECKED AND DOES NOT CONTAIN THE BASES: {', '.join(ev['absent'])}. "
        )
    out += "CURATOR: " + item.caveat
    return out


def build(refresh: bool) -> tuple[list, list]:
    """Return (rows, report), the shape every other stage returns."""
    report, rows = [], []
    for i, it in enumerate(ITEMS):
        ordinal = i + 1
        rid = f"PLF:{ID_BASE + i:04d}"
        report.append(f"  {rid} {it.name}")
        try:
            ev, lines = verify(it, refresh)
        except Exception as e:  # noqa: BLE001 -- one bad item must not kill the stage
            report.append(f"    DROP {e.__class__.__name__}: {e}")
            continue
        report += lines

        # `source_accession` is the VERSION that came back, not the unversioned
        # string the URL asked for. The URL is recorded as fetched, so the two
        # columns say different true things: this is where we looked, and this
        # is what was there. `verify()` has already refused the row if the
        # version was not the one the allow-list expects.
        anchor_src = (
            "ena", ev["anchor_record"].accession, "INSDC-free",
            f"{ENA_BASE}/fasta/{it.anchor}?range={it.lo}-{it.hi}",
            ev["region_meta"].get("retrieved", TODAY),
            ev["region_meta"].get("sha256", ""),
        )
        anchor_rec_src = (
            "ena", ev["anchor_record"].accession, "INSDC-free",
            f"{ENA_BASE}/embl/{it.anchor}",
            ev["anchor_meta"].get("retrieved", TODAY),
            ev["anchor_meta"].get("sha256", ""),
        )
        prov = [
            (rid, "reference_nt", *anchor_src),
            (rid, "boundary_evidence", *anchor_src),
            (rid, "boundary_evidence", *anchor_rec_src),
            (rid, "name", "polylinker", "-", "own-work", "-", TODAY, ""),
            (rid, "aliases", "polylinker", "-", "own-work", "-", TODAY, ""),
            (rid, "boundary_rule", "polylinker", "-", "own-work", "-", TODAY, ""),
            (rid, "description", "polylinker", "-", "own-work", "-", TODAY, ""),
        ]
        # One provenance row per witness, on `boundary_evidence`, because the
        # witnesses ARE the evidence for a Class B boundary. features/NOTICE
        # promises per-field sourcing; "we checked two other records" is a
        # sourcing claim and belongs in the table where it can be re-fetched.
        exemplar_ids = []
        for group in ev["submitters"].values():
            for e in group:
                if e["is_anchor"]:
                    continue  # already cited twice above, as region and as record
                exemplar_ids.append(e["accession"])
                prov.append((
                    rid, "boundary_evidence", "ena", e["accession"], "INSDC-free",
                    f"{ENA_BASE}/embl/{e['accession']}",
                    e["meta"].get("retrieved", TODAY), e["meta"].get("sha256", ""),
                ))
        rows.append(
            Row(
                id=rid,
                ordinal=ordinal,
                name=it.name,
                aliases=list(it.aliases),
                cls=it.cls,
                genbank_key=it.genbank_key,
                reference_nt=it.sequence,
                reference_aa="",
                boundary_rule="consensus_of_insdc",
                boundary_evidence=(
                    f"{ev['anchor_record'].accession or it.anchor}:{it.lo}-{it.hi}"
                    f":{it.strand}; the same bases also in "
                    + ", ".join(exemplar_ids)
                ),
                description=it.description,
                notes=notes_for(it, ev),
                patent_flag="1" if it.patent_flag else "0",
                provenance=prov,
            )
        )
    return rows, report


# --------------------------------------------------------------------------
# Gates, run against input that must trip them


def self_test() -> list[str]:
    """A check that cannot fail proves nothing. These are the four this stage
    turns on, each driven with input it must refuse.

    None of them touches the network, so they run on every build.
    """
    out, fails = [], []

    def must(label: str, cond: bool) -> None:
        out.append(f"  {'PASS' if cond else 'FAIL'}  {label}")
        if not cond:
            fails.append(label)

    # 1. revcomp round-trips, and is not the identity on a real element.
    s = "TAATACGACTCACTATA"
    must("revcomp round-trips", revcomp(revcomp(s)) == s)
    must("revcomp is not the identity here", revcomp(s) != s)

    # 2. occurrences finds a minus-strand copy. Finding 3 is the reason this
    #    exists at all; a version that searched one strand would silently miss
    #    every exemplar that annotated the element the wrong way round.
    hay = "GGGG" + revcomp(s) + "TTTT"
    hits = occurrences(hay, s)
    must("minus-strand copy is found", hits == [(5, 5 + len(s) - 1, "-")])
    must("absent sequence yields no hit", occurrences("ACGTACGT", s) == [])

    # 3. The SnapGene tell is detected in a feature table and not elsewhere. The
    #    fixture is written in the shape ENA actually serves -- the `label:` is
    #    inside a folded /note -- because that shape is the finding.
    tainted = (
        "ID   XX01; SV 1; circular DNA; STD; SYN; 10 BP.\n"
        "FT   promoter        1..10\n"
        'FT                   /note="promoter for the E. coli lac operon; label: lac'
        ' promoter"\n'
        "SQ   Sequence 10 BP;\n"
        "     acgtacgtac                                                       10\n"
        "//\n"
    )
    clean = tainted.replace("; label: lac promoter", "")
    must("SnapGene 'label:' tell is detected", parse_embl(tainted).snapgene)
    must("a clean record is not flagged", not parse_embl(clean).snapgene)
    # The exported screen must agree with the parser it wraps, because
    # insdc_posture.py drives the export and would otherwise be proving a
    # function nothing else calls still works.
    must("the exported screen sees what parse_embl sees",
         record_is_snapgene_annotated(tainted) and not record_is_snapgene_annotated(clean))
    must("SQ block is parsed", parse_embl(clean).sequence == "ACGTACGTAC")
    # The accession is reassembled with its SV. `boundary_evidence` citing a
    # bare accession would point at whatever ENA serves under that name next.
    must("the accession carries its sequence version",
         parse_embl(clean).accession == "XX01.1")
    must("regulatory feature coordinates are parsed",
         parse_embl(clean).features == [("promoter", 1, 10, "+")])

    # A BARE COORDINATE is a location, and reading it as one is what lets the
    # T7 promoter row say where the record puts the transcription start. This
    # test was written after a version that required `..` reported "the record
    # annotates nothing near this interval" about a record that annotates 26
    # promoters, every one of them as a point.
    pointy = (
        "ID   XX03; SV 1; linear DNA; STD; PHG; 10 BP.\n"
        "FT   regulatory      7\n"
        'FT                   /regulatory_class="promoter"\n'
        "FT   terminator      complement(3)\n"
        'FT                   /note="x"\n'
        "SQ   Sequence 10 BP;\n"
        "     acgtacgtac                                                       10\n"
        "//\n"
    )
    must("a bare coordinate is read as a point feature",
         parse_embl(pointy).features
         == [("regulatory", 7, 7, "+"), ("terminator", 3, 3, "-")])
    must("a point is described as a point, with its strand",
         relate((1, 5), (7, 7), "regulatory", "+", "+")
         == "regulatory as a POINT at 7 on the + strand, 2 base(s) 3' of this interval")

    # 4. offsets() reports in the ELEMENT's sense, so the two strands are
    #    mirror images of each other. Getting this backwards would tell a
    #    curator a depositor trimmed the 5' end when they extended it.
    must("offsets on the plus strand", offsets((10, 20), (7, 25), "+") == "5'+3/3'+5")
    must("offsets invert on the minus strand", offsets((10, 20), (7, 25), "-") == "5'+5/3'+3")

    # 4b. THE IDENTITY THE CORROBORATION RULE RESTS ON. `verify()` decides
    #     "places it exactly" by comparing raw intervals, because the rendered
    #     string is truncated to four features and would silently lose a fifth.
    #     That is only sound if interval equality and `5'+0/3'+0` are the same
    #     statement, on both strands -- so assert it rather than assume it.
    for st in ("+", "-"):
        must(f"equal intervals render as 5'+0/3'+0 on the {st} strand",
             offsets((10, 20), (10, 20), st) == "5'+0/3'+0")
        must(f"an off-by-one interval does not, on the {st} strand",
             offsets((10, 20), (10, 21), st) != "5'+0/3'+0")

    # 4c. The rule itself, driven with evidence that must fail it. Without this
    #     the check is reachable only through the network or the cache, which is
    #     precisely the condition under which a rule stops being tested.
    def sub(*flags):
        return [{"exact": ["regulatory"] if f else [], "accession": f"X{i}"}
                for i, f in enumerate(flags)]

    one_lab_twice = {"kaist": sub(True, True)}
    two_labs = {"kaist": sub(True), "gent": sub(True)}
    holds_but_disagrees = {"kaist": sub(True), "gent": sub(False), "oregon": sub(False)}
    must("two records from ONE lab are one corroborating submission",
         len(corroborating_submissions(one_lab_twice)) == 1)
    must("two labs placing it exactly are two",
         len(corroborating_submissions(two_labs)) == MIN_PLACEMENTS)
    must("submissions that hold the bases but draw other edges do not count",
         len(corroborating_submissions(holds_but_disagrees)) == 1)
    must("a submission whose only record places it partly still counts once",
         len(corroborating_submissions({"a": sub(False, True)})) == 1)
    must("the floor is above one, or the rule permits a consensus of one",
         MIN_PLACEMENTS >= 2)

    # 5. Two spellings of ONE lab's address merge; two institutions do not.
    #    The first two strings are the submission addresses ENA serves for
    #    DQ250998 and FJ457001, both of which witness the SP6 row. Exact string
    #    equality counts them as two independent submissions. They are one lab.
    ore_a = ("institute of molecular biology, university of oregon, 355 streisinger "
             "hall, eugene, or 97403-1229, usa")
    ore_b = "institute of molecular biology, university of oregon, eugene, or 97403, usa"
    other = "usda-ars, nadc, 1920 dayton ave, ames, ia 50010, usa"
    must("one lab's two address spellings merge", same_submitter(ore_a, ore_b))
    must("two different institutions do not merge", not same_submitter(ore_a, other))
    must("an empty address does not merge with everything",
         not same_submitter("", other))

    # 6. A join() location contributes no interval rather than a wrong one.
    joined = (
        "ID   XX02; SV 1; circular DNA; STD; SYN; 10 BP.\n"
        "FT   promoter        join(8..10,1..2)\n"
        'FT                   /gene="x"\n'
        "SQ   Sequence 10 BP;\n"
        "     acgtacgtac                                                       10\n"
        "//\n"
    )
    must("a join() location is skipped, not approximated",
         parse_embl(joined).features == [])

    # 7. The fetch guard refuses an ENA URL that is not a record.
    try:
        _guard(f"{ENA_BASE}/search?query=lac")
        must("the fetch guard refuses a non-record ENA endpoint", False)
    except SystemExit:
        must("the fetch guard refuses a non-record ENA endpoint", True)

    if fails:
        raise SystemExit(
            "stage_classb self-test failed: " + "; ".join(fails)
            + ". Refusing to emit rows from gates that do not hold."
        )
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="Stage 5 -- Class B conventions, standalone")
    ap.add_argument("--refresh", action="store_true", help="re-fetch every source")
    args = ap.parse_args()

    print("Stage 5 -- Class B conventions (standalone run)")
    print(f"  date {TODAY}   cache {CACHE}")
    print(f"  {len(ITEMS)} allow-listed items, {len(HELD)} worked up and held\n")
    print("\n".join(self_test()))
    print()

    rows, report = build(args.refresh)
    print("\n".join(report))
    print(f"\n{len(rows)}/{len(ITEMS)} row(s) verified")
    return 0 if len(rows) == len(ITEMS) else 1


if __name__ == "__main__":
    raise SystemExit(main())
