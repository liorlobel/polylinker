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
items declared on that date did, and they now drop:

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

A ROW MAY ALSO COME BACK BECAUSE THIS FILE CHANGED, AND THAT IS A DIFFERENT
THING WEARING THE SAME CLOTHES. The paragraph above is about NEW EVIDENCE. On
2026-08-11 the rule's IMPLEMENTATION changed instead: `verify()` scored
`occurrences()[0]` and no other copy, so a depositor who carries the element
twice and draws our edges over the SECOND copy was measured as having drawn
nothing. `place_in_record()` now scores every copy. Because that widens what
counts as corroboration for every Class B row at once, it must be said plainly
what it moved, and the answer is MEASURED and not assumed:

    NOTHING. All five refused rows are refused on the same evidence and by the
    same numbers; all nine rows IN THE TABLE ON THAT DATE keep their extents,
    their offsets and their `notes` byte for byte; features.tsv and
    provenance.tsv rebuild identical. Four witness records in `ITEMS` carry
    their element more than once -- V01146 (T7, 7x), AY288927 (SP6, 3x),
    U13859 (rrnB T2, 2x) and AJ318471 (T3, 3x) -- and in every one of them the
    copies are annotated alike, so scoring the first was already scoring all
    of them.

That was measured over the HELD list as it stood on 2026-08-11, and not only
over `ITEMS`, because a held element is the population a returning row would
come from. Eleven of the extents those entries named could be re-derived from
the accessions they cite and were driven through this same code both ways;
EXACTLY ONE of them moves, the mouse PGK promoter, from one exact placement to
two -- the entry that is now `PLF:4015`. Nothing else does -- not the U6 forms,
not H1, not either AG form, not chicken beta-actin, not PLtetO-1, and not the
tetO7 heptamer, which was the one to watch because a tandem repeat is where
multiple copies are expected. The SV40 early promoter
cannot be reached by this fix at all: its 419 nt form occurs contiguously in no
record, so there is no first copy to have been scoring. TWO ENTRIES ARE NOT
COVERED BY THAT SENTENCE AND SAY SO RATHER THAN BEING COUNTED AS CLEAN -- the
CMV-enhancer-containing CAG forms and the CMV-tetO2/PTight group state no
accession:lo-hi this file could re-derive, so their extents were not measured
either way, then or now.

PGK WAS HELD ON THAT DATE AND WAS NOT A ROW, so no row returned on this change,
which is what this paragraph was written to establish. If a row is ever seen to
return on a change to this file, this paragraph is the first thing to re-run: a
return that no new evidence explains is the implementation moving the bar, and
it must be reported as that.

PGK IS A ROW NOW, `PLF:4015`, AND IT DID NOT RETURN -- IT WAS ISSUED. That is
the distinction this whole section exists to keep, so it is drawn here rather
than left to the reader. On 2026-08-12 Lior Lobel instructed that the element be
issued, and the item was APPENDED to `ITEMS` on that instruction. The stage
still refuses to promote an element on its own, the sentence below about a
curator's decision still stands unamended, and nothing in the corroboration rule
was touched: the row measures two independent submissions and two exact
placements against floors of two and two, the same numbers this paragraph
recorded when the element was held. What was missing on 2026-08-11 was never a
number; it was a signature on the judgement, and a human supplied it.

ONE CLAIM THIS FIX ITSELF MADE HAD TO BE WITHDRAWN, which is the reason to
distrust a widening even when it measures clean. `place_in_record()` at first
appended the words "this record's copies DISAGREE" whenever a record's copies
were not annotated identically. Measured on KX264176.1 -- a PLtetO-1 witness
below -- that is false: it carries the element twice, the depositor drew
`regulatory` edge for edge over BOTH copies, and only a neighbouring
`misc_feature` differs. The copies agree about the extent perfectly. The
disclosure now says what was actually tested, that the record does not annotate
its copies alike, and `self_test()` drives that record's shape.

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

Nine further elements were worked up in the first pass and were NOT rows. Three
of them are rows now -- the T3 promoter, the araBAD promoter and the human
EF-1alpha promoter, PLF:4012 to PLF:4014, APPENDED on 2026-08-11 so that no
published id moved. Each had been refused for a reason that measurement did not
support: T3 for a want of witnesses it does not have, EF-1alpha because a vector
form cannot be cited to the gene by coordinates (true, and it does not need to
be), araBAD for want of an anchor that exists. The corrected reasons are in
`HELD` below beside the six elements that are still refused, which are now ten
entries because two of them were one name over several unrelated elements and
have been split. A row whose boundary would be a coin toss between two
equally-supported, non-nested conventions is not a row this database can carry,
because `consensus_of_insdc` would then be false on its face.

NOT DONE, AND IT IS A DEFECT IN THIS FILE RATHER THAN IN ANY ROW: `FT_LINE`
cannot match a feature key fifteen characters wide, because EMBL pads the key
field to sixteen columns and the pattern demands two spaces. `prim_transcript`,
`minus_35_signal` and `minus_10_signal` are therefore declared in
`REGULATORY_KEYS` and unreachable -- a key that cannot match is the same shape
of nothing as a check that cannot fail. It is left alone here because widening
it adds text to the `notes` of rows already under sign-off, and this pass is not
entitled to move those. PLF:4014's caveat records where it bites.

Usage
-----
    python features/build/stage_classb.py            # from cache
    python features/build/stage_classb.py --refresh  # re-fetch everything
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field, replace
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
    withdrawn: str = ""
    """WHY the curator withdrew this row, or "" if it ships. Set it and the row
    stops at the gate in `build()`; the declaration stays here and keeps its id.

    A STRING AND NOT A BOOL, and that is the whole design of this field. The id
    is permanent, so a withdrawal is permanent too, and `withdrawn = True` would
    record that somebody decided without recording what they decided -- which is
    the one thing this database refuses everywhere else. The reason is written
    into the build log next to the id it retires, so a reader who finds a gap in
    the numbering has the answer in the same place as the gap.

    WHY THE DECLARATION MUST STAY. `allocation()` numbers from the item's INDEX
    in `ITEMS`, so deleting a withdrawn item renumbers every item after it -- see
    the comment above `ITEMS`, which measures exactly what that would cost here.
    Keeping the declaration is what makes the retired id unreachable forever
    rather than merely unused today.

    NOT a review status and not a substitute for one. `review_status` moves only
    through features/SIGNOFF.tsv, which this file cannot write; this field
    removes a row from the table altogether, and a row that is not in the table
    cannot be signed."""


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
#
# THE GATE NOW EXISTS: `Convention.withdrawn`, added 2026-08-11 and carrying the
# curator's reason. Set it on the item you are withdrawing and leave the item
# where it is. `self_test()` check 8 pins PLF:4006..PLF:4010 to the elements they
# are published as and asserts that marking one moves none of the others -- and
# asserts, against the same fixture with the item DELETED instead, that the pin
# really does catch the renumbering.
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
        # WITHDRAWN 2026-08-11 by Lior Lobel, the curator, taking the decision the
        # caveat above declined to take for him. The declaration stays here and
        # keeps index 6, so PLF:4006 is retired rather than freed and can never be
        # issued to another element. features/PROPOSED.md sets out the alternative
        # -- restoring the promoter row first -- and what it would have cost.
        withdrawn=(
            "withdrawn 2026-08-11 by Lior Lobel. Its notes referenced PLF:4005, "
            "which is not in the table, and asserted a shipping condition -- 'ship "
            "this row and the enhancer together or not at all' -- that the table "
            "violates. Of this project's own evidence, only the SnapGene-shaped "
            "submissions draw the 380/204 split; every other deposit annotates the "
            "region as a single element and calls it a promoter. And Boshart et al. "
            "1985, Cell 41:521-530, PMID 2985280, place the enhancer at -118..-524, "
            "which straddles the split on either numbering, so neither of this "
            "row's edges is a literature edge."
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
        # The sentence below used to end "it is the shortest and most sharply
        # bounded of the twelve", and that was false on the day it was written,
        # not merely overtaken: the twelve were PLF:4000..PLF:4011, and two of
        # them -- T7 and SP6, both declared above this row -- are 17 nt against
        # this row's 28. A superlative no reader can re-derive from the table is
        # the kind of claim this file exists to refuse, so the replacement states
        # the measurement it was pretending to restate (every witness at
        # 5'+0/3'+0), records that the old claim was wrong rather than dropping
        # it, and leaves the ranking to anyone who measures `reference_nt`.
        # "Most sharply bounded" is not kept as a superlative either, because on
        # the offsets alone PLF:4000 and PLF:4001 tie this row exactly.
        caveat=(
            "THE ONLY ELEMENT IN THIS STAGE WITH NO COMPETING EXTENT. Every deposit "
            "found that annotates T2 separately encloses exactly these 28 bases. "
            "That is a fact about T2 and not a general reassurance about this "
            "stage. IT IS NOT A FACT ABOUT ITS LENGTH, AND THE SENTENCE THAT STOOD "
            "HERE UNTIL 2026-08-12 SAID IT WAS: 'the shortest and most sharply "
            "bounded of the twelve' was WRONG ON THE DAY IT WAS WRITTEN and not "
            "merely overtaken since. The twelve were PLF:4000..PLF:4011; PLF:4000 "
            "and PLF:4001 are 17 nt each and both are declared above this row, and "
            "PLF:4012, appended later, is 19 nt. Measure `reference_nt` in "
            "features.tsv: 28 nt is the fourth shortest of the nine Class B rows "
            "in the table, and is not the shortest of the twelve, of the nine, or "
            "of the fifteen elements this stage declares. WHAT THE "
            "MEASUREMENT ABOVE SUPPORTS IS THE EDGES AND NOT THE COUNT, and it is "
            "not a superlative either -- every witness of this row that annotates "
            "anything over these bases draws 5'+0/3'+0, and PLF:4000 and PLF:4001 "
            "read the same way. What separates this row from those two is in the "
            "notes and not in the offsets: theirs NAME rival extents found "
            "elsewhere (19 nt and 21 nt forms for T7; a 19 nt published consensus "
            "running through +1 for SP6) and this row's names none. Note "
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
    # ------------------------------------------------------------------
    # APPENDED 2026-08-11, and APPENDED is the whole of why they are here and
    # not where they belong. The T3 promoter is the sibling of PLF:4000 and
    # PLF:4001 and reads far better next to them; putting it there would have
    # moved twelve published ids, so it is PLF:4012 and the reader is told why
    # rather than shown a tidy tuple that lies about its own history.
    #
    # All three came out of `HELD`. Each was refused there for a reason that
    # measurement did not support, and the reasons that replaced them are at the
    # bottom of this file. Nothing was re-cut to make evidence fit: every extent
    # below is the extent the corpus already drew, and where a nested rival is
    # also corroborated it is named in the caveat with its own count.
    Convention(
        name="T3 promoter",
        aliases=("T3 RNA polymerase promoter", "PT3", "T3 phi10 promoter",
                 "T3 class III promoter"),
        genbank_key="promoter",
        cls="regulatory",
        anchor="AJ318471", anchor_version="AJ318471.1", lo=20733, hi=20751, strand="+",
        sequence="AATTAACCCTCACTAAAGG",
        exemplars=("PV959484", "PQ463640", "OK413188", "PP475160", "LC795782"),
        description=(
            "The 19 bp promoter of bacteriophage T3, read by T3 RNA polymerase and by "
            "no host enzyme, in the extent depositors overwhelmingly draw: the "
            "seventeen bases upstream of a T3 transcription start together with the "
            "first two bases of the transcript. T3 and T7 RNA polymerases are "
            "cross-blind -- the difference is a single C where T7 has G-A, at -10 -- "
            "so a vector carrying a T7 promoter at one end of its polylinker and a T3 "
            "promoter at the other can transcribe either strand of an insert "
            "selectively in vitro."
        ),
        caveat=(
            "THIS ROW USES A DIFFERENT CONVENTION FROM PLF:4000 AND PLF:4001, "
            "DELIBERATELY, AND THE REASON IS EVIDENCE AND NOT PREFERENCE. The T7 and "
            "SP6 rows stop at -1 and exclude the +1 base. The extent that would match "
            "them here is 17 nt, AATTAACCCTCACTAAA = -17..-1, and it is deposited: "
            "checked on 2026-08-11, LT009443.1, LT726830.1 and LT727417.1 all annotate "
            "exactly those seventeen bases -- and all three carry the SAME submitting "
            "address, BCCM/LMBP Universiteit Gent, i.e. one bulk culture-collection "
            "deposit and therefore ONE submission. It fails MIN_PLACEMENTS at one. "
            "This 19 nt extent does not: five independent addresses in this row's own "
            "witness list draw it edge for edge, and a 2026-08-11 survey of T3-family "
            "deposits put it at eighteen. Refusing the corroborated extent to keep "
            "three sibling rows visually consistent would be substituting a "
            "preference for the rule, so the rows disagree and this note says so. "
            "WHERE +1 IS, FROM THE PRIMARY LITERATURE, WHICH MAKES THAT DISAGREEMENT "
            "MEASURABLE RATHER THAN VAGUE. Bailey, Klement & McAllister 1983, Proc "
            "Natl Acad Sci USA 80:2814-2818, PMID 6574450, sequenced six T3 promoters "
            "and report that five share an identical 16 bp block, ACCCTCACTAAAGGGA, "
            "running -12..+4 with initiation at GTP at +1. Aligned on that block this "
            "row is -17..+2, so its last two bases are transcribed. Adhya, Basu, "
            "Sarkar & Maitra 1981, Proc Natl Acad Sci USA 78:147-151, PMID 6264429, "
            "fix the same register independently, marking +1 in a T3 promoter "
            "sequence and reporting T3 and T7 to be virtually identical between -9 "
            "and +4 and quite different between -17 and -10. "
            "AND THE 5' EDGE IS NOT A CONSERVATION BOUNDARY HERE, WHICH IS THE ONE "
            "PLACE THE ANALOGY TO PLF:4000 BREAKS. The T7 row's -17 is where a "
            "conserved column block ends. Bailey's conserved T3 block is only 16 bp "
            "and is preceded by a six-base A+T region that VARIES between T3 "
            "promoters: Adhya's reads TATTTA where all three copies in this row's "
            "anchor read AATTAA. So -17 here is the edge the anchor and the deposits "
            "draw, not the edge conservation forces. "
            "THE ANCHOR, AND WHY THIS COPY. AJ318471.1 is the T3 genome (Pajunen et "
            "al., University of Turku). These nineteen bases occur three times in it "
            "and the record annotates a promoter at each of the three, every one at "
            "23 nt = -17..+6: phi6.5 at 17160..17182, phi10 at 20733..20755, phi13 at "
            "25457..25479. The coordinates cited are the phi10 copy, which sits "
            "immediately upstream of the gene 10A major-capsid CDS at 20891..21934 -- "
            "the same choice, for the same reason, as the T7 row's phi10. Which of "
            "the three is cited does not change a base. Note that the witness line "
            "above scores the anchor at 17160, because occurrences() returns the "
            "first copy; the annotation there is the phi6.5 promoter and its offsets "
            "against this row are identical. "
            "RIVALS, MEASURED 2026-08-11, ALL NESTED WITH THIS ONE EXCEPT THE LAST. "
            "20 nt (-17..+3) is genuinely corroborated and is this row plus one G: "
            "MG490656.1 (Universite de Montreal), MZ541859.1 (Wisconsin-Madison), "
            "OP405364.1 (CNB-CSIC Madrid) and AF151087.1 (Stratagene) annotate it "
            "exactly, four addresses, and the survey put it at fourteen. This row "
            "takes the narrower of two nested conventions, as PLF:4002 does; a file "
            "labelled over 20 or 23 nt matches this row across part of its length, "
            "which is the correct outcome and not a miss. The anchor's own 23 nt is a "
            "third nested form with one submission. What is NOT nested is a family "
            "shifted one base 3' -- ATTAACCCTCACTAAAGGG and ATTAACCCTCACTAAAGGGA -- "
            "and that family is why this element was held: the HELD reason said the "
            "two leading conventions were 17 nt and 19 nt, offset, sharing a 16 nt "
            "core, with one submission each. Read as a comparison of the 17 nt form "
            "with the SHIFTED 19 nt form that is exactly right, and both of those do "
            "have one submission. It was the wrong pair: on a corpus large enough to "
            "see it, the two leading conventions are this row and the 20 nt, they "
            "share this row's 5' edge, and they are nested."
        ),
    ),
    Convention(
        name="araBAD promoter",
        aliases=("pBAD promoter", "PBAD", "araBAD operon promoter",
                 "arabinose-inducible promoter"),
        genbank_key="promoter",
        cls="regulatory",
        anchor="J01641", anchor_version="J01641.1", lo=1004, hi=1288, strand="+",
        sequence=(
            "AAGAAACCAATTGTCCATATTGCATCAGACATTGCCGTCACTGCGTCTTTTACTGGCTCTTCTCGCTAACCAAACCGG"
            "TAACCCCGCTTATTAAAAGCATTCTGTAACAAAGCGGGACCAAAGCCATGACAAAAACGCGTAACAAAAGTGTCTATA"
            "ATCACGGCAGAAAAGTCCACATTGATTATTTGCACGGCGTCACACTTTGCTATGCCATAGCATTTTTATCCATAAGAT"
            "TAGCGGATCCTACCTGACGCTTTTTATCGCAACTCTCTACTGTTTCTCCAT"),
        exemplars=("OR900359", "PQ381271", "PQ015303", "PP457274", "LC143902"),
        description=(
            "The regulatory region driving the Escherichia coli L-arabinose operon, in "
            "the extent cloning vectors carry as 'the pBAD promoter': the araC-araBAD "
            "intergenic region, containing the AraC operator and inducer sites, the "
            "cyclic-AMP receptor protein site, and PBAD itself. AraC represses it by "
            "looping between operators in the absence of arabinose and activates it in "
            "the presence of arabinose and CRP-cAMP, which is why this promoter is "
            "prized for titratable expression rather than raw strength."
        ),
        caveat=(
            "WHAT THE TWO EDGES ARE, AND WHAT NEITHER OF THEM IS. On the anchor's own "
            "feature table the araC coding sequence ends at 977 and the araB "
            "initiation codon is at 1316, so this interval starts 26 bases after the "
            "one and stops 27 bases before the other, entirely inside the intergenic "
            "region and touching neither gene. IT IS NOT ESTABLISHED HERE THAT 1288 "
            "IS -1 RELATIVE TO THE araBAD TRANSCRIPTION START, and this row does not "
            "claim it: unlike PLF:4000, the anchor annotates no transcription start "
            "anywhere in this region, and the length was not read from any paper. "
            "Both edges are the convention the deposits draw and nothing more. "
            "WHAT THE ANCHOR DOES ANNOTATE INSIDE THE INTERVAL, all four of which the "
            "depositor marks '(approx)': a CRP site at 1143-1168 and an AraC operator "
            "at 1145-1183 in the Pc region, a second CRP site at 1182-1211 and the "
            "AraC inducer site at 1212-1240 in the PBAD region. The row encloses the "
            "whole cluster; it is not bounded by any one of them. "
            "THESE BASES ARE NOT IN THE K-12 REFERENCE GENOME, AND THAT IS A "
            "MEASUREMENT AND NOT A WORRY. Against U00096.3 (E. coli K-12 MG1655) "
            "these 285 bases match at 284 of 285, the genome carrying the element on "
            "its minus strand at 70076-70360. The single difference is at THIS ROW'S "
            "POSITION 72 = J01641.1:1075 = U00096.3:70289 -- A in the anchor, C in "
            "MG1655 -- and no feature of either record annotates that base; it is 68 "
            "bases 5' of the first regulatory site the anchor draws. J01641 is cited "
            "because it is the record the bases "
            "are verbatim in. Its own reference list draws on both K12 and B/r "
            "material (Smith & Schleif 1978, J Biol Chem 253:6931-6933, PMID 357433, "
            "covering 1082-1335; Greenfield, Boone & Wilcox 1978, Proc Natl Acad Sci "
            "USA 75:4724-4728, PMID 368797, covering 1124-1332; Ogden, Haggerty, "
            "Stoner, Kolodrubetz & Schleif 1980, Proc Natl Acad Sci USA 77:3346-3350, "
            "PMID 6251457; Lee, Gielow & Wallace 1981, Proc Natl Acad Sci USA "
            "78:752-756, PMID 6262769, on the overlapping Pc and PBAD promoters), and "
            "WHICH strain this vector fragment descends from was NOT determined here. "
            "A NESTED 28 nt CONVENTION EXISTS AND HAS THE BEST PROVENANCE OF ANY OF "
            "THEM: J01641.1:1253-1280, annotated in X81837.1 (pBAD24) and X81838.1 "
            "(pBAD18) -- L. M. Guzman, Harvard Medical School, i.e. the pBAD vectors' "
            "own author, Guzman, Belin, Carson & Beckwith 1995, J Bacteriol "
            "177:4121-4130, PMID 7608087. Those two records are ONE submitting "
            "address, so on this stage's arithmetic that extent stands or falls on "
            "witnesses this row has not enumerated; it is nested inside this row "
            "either way, so a file annotated over it matches here across part of its "
            "length. A 131 nt convention also circulates and is NOT this row and was "
            "NOT measured here: the 2026-08-11 survey reported it as an iGEM registry "
            "part arriving through INSDC deposits with an engineered NheI site at its "
            "3' end, and iGEM is a NO-GO source under SOURCING.md section 1, so no "
            "extent was taken from it -- recorded as a warning, not as a finding. "
            "Note finally that PLF:1002 already carries AraC, the protein that works "
            "on this promoter; the two rows are halves of one system and neither "
            "implies the other."
        ),
    ),
    Convention(
        name="EF-1alpha promoter (human)",
        aliases=("EF-1a promoter", "EEF1A1 promoter", "PEF-1alpha",
                 "human elongation factor 1 alpha promoter"),
        genbank_key="promoter",
        cls="regulatory",
        anchor="J04617", anchor_version="J04617.1", lo=373, hi=1560, strand="+",
        sequence=(
            "CGTGAGGCTCCGGTGCCCGTCAGTGGGCAGAGCGCACATCGCCCACAGTCCCCGAGAAGTTGGGGGGAGGGGTCGGCA"
            "ATTGAACCGGTGCCTAGAGAAGGTGGCGCGGGGTAAACTGGGAAAGTGATGTCGTGTACTGGCTCCGCCTTTTTCCCG"
            "AGGGTGGGGGAGAACCGTATATAAGTGCAGTAGTCGCCGTGAACGTTCTTTTTCGCAACGGGTTTGCCGCCAGAACAC"
            "AGGTAAGTGCCGTGTGTGGTTCCCGCGGGCCTGGCCTCTTTACGGGTTATGGCCCTTGCGTGCCTTGAATTACTTCCA"
            "CGCCCCTGGCTGCAGTACGTGATTCTTGATCCCGAGCTTCGGGTTGGAAGTGGGTGGGAGAGTTCGAGGCCTTGCGCT"
            "TAAGGAGCCCCTTCGCCTCGTGCTTGAGTTGAGGCCTGGCCTGGGCGCTGGGGCCGCCGCGTGCGAATCTGGTGGCAC"
            "CTTCGCGCCTGTCTCGCTGCTTTCGATAAGTCTCTAGCCATTTAAAATTTTTGATGACCTGCTGCGACGCTTTTTTTC"
            "TGGCAAGATAGTCTTGTAAATGCGGGCCAAGATCTGCACACTGGTATTTCGGTTTTTGGGGCCGCGGGCGGCGACGGG"
            "GCCCGTGCGTCCCAGCGCACATGTTCGGCGAGGCGGGGCCTGCGAGCGCGGCCACCGAGAATCGGACGGGGGTAGTCT"
            "CAAGCTGGCCGGCCTGCTCTGGTGCCTGGCCTCGCGCCGCCGTGTATCGCCCCGCCCTGGGCGGCAAGGCTGGCCCGG"
            "TCGGCACCAGTTGCGTGAGCGGAAAGATGGCCGCTTCCCGGCCCTGCTGCAGGGAGCTCAAAATGGAGGACGCGGCGC"
            "TCGGGAGAGCGGGCGGGTGAGTCACCCACACAAAGGAAAAGGGCCTTTCCGTCCTCAGCCGTCGCTTCATGTGACTCC"
            "ACGGAGTACCGGGCGCCGTCCAGGCACCTCGATTAGTTCTCGAGCTTTTGGAGTACGTCGTCTTTAGGTTGGGGGGAG"
            "GGGTTTTATGCGATGGAGTTTCCCCACACTGAGTGGGTGGAGACTGAAGTTAGGCCAGCTTGGCACTTGATGTAATTC"
            "TCCTTGGAATTTGCCCTTTTTGAGTTTGGATCTTGGTTCATTCTCAAGCCTCAGACAGTGGTTCAAAGTTTTTTTCTT"
            "CCATTTCAGGTGTCGTGA"),
        exemplars=("LC884827", "LC904051"),
        description=(
            "The promoter of the human translation elongation factor 1-alpha gene "
            "EEF1A1, taken as vectors take it: the region upstream of the "
            "transcription start together with the whole of the first exon and the "
            "whole of the first intron, ending nine bases into exon 2. The intron is "
            "not padding -- it is the reason this element is used, and the reason it "
            "is a kilobase long. A housekeeping promoter, active in most mammalian "
            "cell types and much less prone to silencing than the viral alternatives, "
            "which is what recommends it for stable lines where the CMV promoter fades."
        ),
        caveat=(
            "THIS IS NOT A -N..-1 INTERVAL AND A READER WHO ASSUMES IT IS WILL BE "
            "WRONG BY A KILOBASE. The anchor annotates a TATA box at 546..552, a "
            "primary transcript beginning at 576 and an intron at 609..1551, so this "
            "row runs from 203 bases upstream of the transcription start, through "
            "exon 1, through all of intron 1, and nine bases into exon 2. "
            "AND THE MEASURED 'ANCHOR RECORD'S OWN ANNOTATION' LINE ABOVE WILL NOT "
            "SHOW THE TRANSCRIPT, WHICH IS A DEFECT IN THIS STAGE AND IS RECORDED "
            "RATHER THAN FIXED. `FT_LINE` requires two or more spaces between the "
            "feature key and its location; EMBL pads the key field to sixteen "
            "columns; `prim_transcript` is fifteen characters, so it is followed by "
            "exactly one space and the pattern never matches it. The same is true of "
            "`minus_35_signal` and `minus_10_signal` -- three of the keys "
            "REGULATORY_KEYS declares are unreachable, and a key that cannot match is "
            "the same shape of nothing as a check that cannot fail. Repairing it "
            "would add text to the notes of rows already under sign-off, so it is "
            "left for a pass that can re-sign them. The transcript coordinates above "
            "were read out of J04617.1 by hand on 2026-08-11 and are re-checkable in "
            "four lines of its feature table. "
            "THE VECTOR FORM IS A DIFFERENT SEQUENCE, ALSO CLEARS THIS STAGE'S RULE, "
            "AND WAS NOT TAKEN. It is 1179 nt = J04617.1:378-685 followed directly by "
            "J04617.1:690-1560 -- this row minus its first five bases and minus the "
            "four bases GCCC at J04617.1:686-689 -- and three independent submissions "
            "annotate exactly it: MG547974.1 (Bioengineering, UCSD), OQ300330.1 "
            "(Siriraj, Mahidol) and PP944532.1 (Pharmaceutical Sciences, Tsinghua). "
            "Three is more than this row's two. It was still not taken, for two "
            "reasons that are about evidence and not taste. First, all three of those "
            "placements begin at base 1 of their own record, so they corroborate the "
            "3' edge and not the 5', which is where their fragment happens to start; "
            "this row's two placements are interior annotations where the depositor "
            "chose both edges -- LC884827.1 regulatory 2914..4101 and LC904051.1 "
            "misc_feature 2117..3304. Second, the vector form is a verbatim slice of "
            "no primary record and would have to be anchored on a synthetic-construct "
            "deposit. THE COST OF THAT CHOICE, STATED PLAINLY: this row is not what "
            "most pEF vectors carry. THE DELETION IS THE VECTORS' AND NOT THE "
            "RECORD'S ERROR: AL603910.6, the Sanger genomic clone RP11-505P4, holds "
            "J04617.1:656-715 verbatim, GCCC included. "
            "IT WILL STILL ANNOTATE THOSE VECTORS, and that was checked rather than "
            "hoped: pl-features aligns with an edit-tolerant matcher whose default "
            "`min_identity` is 0.96, and a four-base deletion over 1188 nt is 99.7% "
            "identity, so a real pEF vector resolves against this row with the indel "
            "reported -- it does not fall through. "
            "AT THE FLOOR, WITH NOTHING TO SPARE. Two corroborating submissions "
            "against a floor of two, and one of the two annotates the interval as "
            "`misc_feature` rather than as a regulatory key. Both are Tokyo deposits, "
            "which is worth an eye: they are different institutions (Tokyo "
            "Metropolitan Institute of Medical Science; The University of Tokyo) and "
            "same_submitter() scores their address overlap at 0.25 against a merge "
            "threshold of 0.6. AL603910.6 holds this row's 1188 bases at "
            "113562-114749 on the minus strand with ONE mismatch, at this row's "
            "position 431 = J04617.1:803, C in the anchor and T in the clone -- so "
            "the anchor is not an outlier, but neither record is 'the' sequence. "
            "Finally, the figures in the old HELD reason -- a 1144 nt vector element "
            "against a 1148 nt gene -- were not reproducible on any record fetched "
            "here; the two real conventions are 1179 and 1188. The primary source for "
            "both is Uetsuki, Naito, Nagata & Kaziro 1989, J Biol Chem 264:5791-5798, "
            "PMID 2564392, which is J04617's own publication."
        ),
    ),
    # ISSUED ON THE CURATOR'S INSTRUCTION, 2026-08-12, by Lior Lobel. APPENDED,
    # so it takes PLF:4015 and none of PLF:4000..PLF:4014 moves.
    #
    # This is the entry the module docstring's every-copy paragraph named as the
    # ONE element that moved when `place_in_record()` started scoring every copy
    # -- from one exact placement to two. That paragraph also said, correctly,
    # that PGK was HELD and that no row returns on that change, because "a stage
    # that promoted an element the moment its own code stopped under-counting
    # would be adjusting its own membership". Nothing about that reasoning has
    # been revised. What changed is that the decision the stage was refusing to
    # take was taken by the human it belongs to, and this comment is the record
    # of WHO took it, so that a reader who finds the docstring's refusal and
    # this row in the same file is not left to guess which won.
    Convention(
        name="PGK promoter (mouse)",
        aliases=("PGK1 promoter", "mPGK promoter", "Pgk-1 promoter",
                 "mouse phosphoglycerate kinase 1 promoter"),
        genbank_key="promoter",
        cls="regulatory",
        anchor="BX469914", anchor_version="BX469914.4", lo=13192, hi=13699, strand="+",
        sequence=(
            "TACCGGGTAGGGGAGGCGCTTTTCCCAAGGCAGTCTGGAGCATGCGCTTTAGCAGCCCCGCTGGGCACTTGGCGCTAC"
            "ACAAGTGGCCTCTGGCCTCGCACACATTCCACATCCACCGGTAGGCGCCAACCGGCTCCGTTCTTTGGTGGCCCCTTC"
            "GCGCCACCTTCTACTCCTCCCCTAGTCAGGAAGTTCCCCCCCGCCCCGCAGCTCGCGTCGTGCAGGACGTGACAAATG"
            "GAAGTAGCACGTCTCACTAGTCTCGTGCAGATGGACAGCACCGCTGAGCAATGGAAGCGGGTAGGCCTTTGGGGCAGC"
            "GGCCAATAGCAGCTTTGCTCCTTCGCTTTCTGGGCTCAGAGGCTGGGAAGGGGTGGGTCCGGGGGCGGGCTCAGGGGC"
            "GGGCTCAGGGGCGGGGCGGGCGCCCGAAGGTCCTCCGGAGGCCCGGCATTCTGCACGCTTCAAAAGCGCACGTCTGCC"
            "GCGCTGTTCTCCTCTTCCTCATCTCCGGGCCTTTCGACCT"),
        exemplars=("CR293496", "AB242435"),
        description=(
            "The promoter of the mouse phosphoglycerate kinase 1 gene Pgk-1, which "
            "encodes the somatic isoform of the glycolytic enzyme phosphoglycerate "
            "kinase and lies on the X chromosome. Adra, Boer & McBurney sequenced this "
            "region in 1987 and reported the architecture these bases have: G+C rich, "
            "carrying the Sp1-binding hexamer GGGCGG and a CCAAT sequence, and carrying "
            "no TATA box -- a housekeeping gene's promoter and not a regulated one -- "
            "and the same paper showed it drives expression after DNA-mediated "
            "transfection into mammalian cells. Counted over the 508 bases this row "
            "ships rather than taken from the paper: 65.0% G+C, five copies of GGGCGG "
            "(four on this strand and one on the other), one CCAAT, and no TATA "
            "anywhere. In constructs it is used as a short constitutive driver; both "
            "of the records that corroborate this extent are gene-targeting vectors."
        ),
        caveat=(
            "WHAT THESE 508 BASES ARE RELATIVE TO THE GENE, AND WHY IT TOOK AN "
            "ALIGNMENT TO SAY SO. The anchor annotates NOTHING -- not merely nothing "
            "over this interval: BX469914.4 is a finished genomic clone whose feature "
            "table holds one feature, `source 1..30886`, so it cannot place a "
            "transcription start or an initiation codon and the 'ANCHOR RECORD'S OWN "
            "ANNOTATION' line above is empty for that reason and not because the region "
            "was checked and found bare. The landmarks are therefore carried across "
            "from the primary record, M18735.1 (Adra, Boer & McBurney 1987, Gene "
            "60:65-74, PMID 3440520, DOI 10.1016/0378-1119(87)90214-9), which is the "
            "primary source for this promoter and annotates its own exon 1 at 853..1006 "
            "and the initiation codon at 946. Aligned to it, this row runs from 431 "
            "bases upstream of the 5' end of exon 1 to 77 bases inside it: -431..+77, "
            "which is 431 + 77 = 508. It therefore includes the whole of the promoter "
            "as that paper describes it -- the five GGGCGG hexamers sit at -235, -57, "
            "-45, -33 and -28 and the CCAAT at -117, and those are the only copies of "
            "either in the whole 1110 bp primary record -- and then runs on into the "
            "5' untranslated region and stops 16 bases short of the ATG, which is at "
            "BX469914.4:13715. "
            "NEITHER EDGE IS DELIMITED BY ANYTHING PRIMARY, AND THAT IS THE WHOLE "
            "CONTENT OF `boundary_rule = consensus_of_insdc` HERE. -431 is not a "
            "landmark: it is 196 bases further out than the furthest-upstream Sp1 site "
            "and nothing in M18735.1, in the anchor, or in the 1987 paper draws a line "
            "there. +77 is not a landmark either -- it is a point in the middle of exon "
            "1, chosen by neither the transcription start nor the initiation codon, "
            "both of which it misses. This is a convention two depositors converged on, "
            "it is not the gene telling anyone where its promoter ends, and this row "
            "must not be read as claiming otherwise. "
            "IT CLEARS AT EXACTLY THE FLOOR, ON BOTH COUNTS, WITH NOTHING TO SPARE: "
            "two independent submissions against MIN_SUBMISSIONS = 2, and two exact "
            "placements against MIN_PLACEMENTS = 2. Lose either witness and the row "
            "fails. That is the difference between corroborated and corroborated twice "
            "over, and a curator deciding whether to sign is entitled to have it said "
            "rather than inferred from the numbers above. The anchor is NOT one of the "
            "two: BX469914 (Wellcome Trust Sanger Institute) and CR293496 (Sanger "
            "Centre) are both Hinxton addresses, same_submitter() merges them, and the "
            "anchor annotates nothing here in any case -- so it adds neither a "
            "submission nor an opinion about the edges. "
            "THE SECOND PLACEMENT IS ONLY VISIBLE BECAUSE OF THE 2026-08-11 SECOND-COPY "
            "FIX, and that is this row's honest provenance rather than a footnote. "
            "AB242435.1 (Central Institute for Experimental Animals, Kawasaki) carries "
            "the element TWICE: at 374-881, where the depositor drew `regulatory "
            "366..881` -- 516 nt, 5'+8/3'+0 -- and again at 2089-2596, where the "
            "depositor drew `regulatory 2089..2596`, exactly these 508 bases. verify() "
            "scored occurrences()[0] and nothing else, so it saw the 516 and never the "
            "508. Measured under that implementation this element had TWO submissions "
            "and ONE placement and was refused; measured under place_in_record(), which "
            "scores every copy, it has two and two. Nothing about the evidence changed "
            "between those two measurements. "
            "AND THE FIGURE THE OLD HELD REASON GAVE FOR THIS ELEMENT IS WITHDRAWN "
            "RATHER THAN REPEATED. It claimed the element differs from the gene by "
            "roughly forty-eight substitutions; that came of measuring against M18735 "
            "instead of against the anchor, and it is not what either comparison says. "
            "Against BX469914.4 the element is a VERBATIM slice, re-sliced and "
            "re-checked on every build. Against M18735.1 it is three exact blocks -- "
            "element 1-64, 65-442 and 443-508 -- covering all 508 bases with ZERO "
            "substitutions, separated by two single-base indels: one G the element has "
            "and M18735.1 does not, in a G run around M18735.1:486, and one C M18735.1 "
            "has and the element does not, at M18735.1:864, twelve bases into exon 1. "
            "Those two indels are why the longest exact PREFIX shared with M18735.1 is "
            "64 nt, and reading that 64 as the point where the two sequences part "
            "company is the error the old reason made. WHICH RECORD IS RIGHT WAS NOT "
            "DETERMINED: M18735.1 is a 1987 C3H/He record and BX469914.4 is a finished "
            "RPCI-23 BAC, so the two differences could be strain or could be sequencing, "
            "and nothing here distinguishes them. "
            "ISSUED ON THE CURATOR'S INSTRUCTION, 2026-08-12, BY LIOR LOBEL. The stage "
            "refused to promote this element itself and the reason it gave still "
            "stands; the decision was taken by the human it belongs to and not by this "
            "program changing its mind. The row is `proposed` like every other Class B "
            "row: issuing it puts it in the table, and only features/SIGNOFF.tsv can "
            "put it in what `Db::reviewed()` serves."
        ),
    ),
)


# Worked up, checked, and NOT rows. Each line is a refusal with its reason, and
# the list is here rather than in a commit message because the next person to
# want one of these should not have to rediscover why it is missing.
#
# SOURCING.md section 6 budgets about forty Class B rows. Sixteen are declared
# above and ten of them are rows in the table -- one withdrawn by the curator,
# five refused by MIN_PLACEMENTS -- which is what survived the
# two-independent-submissions rule and the nested-rivals rule applied honestly,
# and saying so is worth more than thirty more rows. NONE OF THE TEN SHIPS:
# every one is `proposed`, and `Db::reviewed()` serves none of them. "Reached
# the table" and "ships" are two different events with a curator's signature
# between them, and this file does not use the second word for the first.
#
# Rewritten 2026-08-11. Three entries left this list for `ITEMS`; two were one
# name over several unrelated elements and are now separate entries with
# separate statuses; the rest carry the reason the measurement supports rather
# than the one the first pass guessed. Where an entry now says an extent CLEARS
# the corroboration floor and is still not a row, the missing thing is named.
#
# A FOURTH LEFT ON 2026-08-12, AND NOT FOR THE SAME KIND OF REASON. The three
# above left because re-measurement contradicted the reason they had been held
# for. The mouse PGK promoter's reason was not contradicted: its own entry ended
# by saying the measurement was done and the judgement was not, and it named
# issuing the row as the one decision this file must never take. Lior Lobel took
# that decision on 2026-08-12 and the item was appended as PLF:4015. The entry is
# deleted rather than kept with a note, because a list of refusals is the wrong
# place to record an element that is no longer refused; the history of the
# decision travels with the row, in the comment above it and in its caveat.
HELD: tuple[tuple[str, str], ...] = (
    ("SV40 early promoter",
     "The 330 nt convention is a contiguous circular interval that WRAPS the "
     "numbering origin of the SV40 record, which this schema's "
     "accession:lo-hi:strand boundary_evidence cannot express, and a rival 283 nt "
     "form does not place as a single interval at all. The region also carries a "
     "tandem repeat, so the two forms may differ in repeat copy number -- that was "
     "NOT counted and is not offered as a finding. "
     "RE-CHECKED 2026-08-11 ON A 419 nt FORM OF THE SAME CONVENTION, and the wrap "
     "is not one record's numbering quirk but every record's: the interval is "
     "5171..5589 of a 5243 bp genome, i.e. it runs 346 bases past the origin, and "
     "the assembled 419 bases occur CONTIGUOUSLY in none of J02400.1, AF316139.1 or "
     "EF579804.1 -- all three 5243 bp, all three sharing that origin. The obstacle "
     "is the schema and not the evidence, which is a different thing from the other "
     "holds in this list and would be cleared by a boundary_evidence form that can "
     "express a circular join, not by more fetching."),
    ("U6 promoter (human)",
     "STILL HELD, and the reason it stood under until 2026-08-11 was backwards. It "
     "said only ONE independent submission witnesses the 249 nt extent and that it "
     "'fails on witnesses, not on evidence'. Both halves are the wrong way round. "
     "The bases are witnessed abundantly: four independent addresses in a single "
     "afternoon's fetching hold them (Allen Institute for Brain Science PZ036121.1, "
     "George Washington University JN255690.1, AIST LC414435.1, UCSD MK318530.1) and "
     "a 2026-08-11 survey of 479 records put it at eleven. What exactly ONE "
     "submission does is DRAW those edges: OP099837.1, OP099840.1 and OP099843.1, "
     "all Drug Discovery Sciences, Boehringer Ingelheim Pharma, one address. So it "
     "clears MIN_SUBMISSIONS several times over and fails MIN_PLACEMENTS at one, "
     "which is the opposite failure and has the opposite remedy. "
     "THE EXTENT IS THE ARTICULABLE ONE, WHICH IS WHY IT IS WORTH SAYING SO. "
     "M14486.1 (human U6 gene, clone pGEM/U6, Kunkel, Texas A&M; Kunkel, Maser, "
     "Calvet & Pederson 1986, Proc Natl Acad Sci USA 83:8575-8579, PMID 3464970) "
     "annotates the PSE at 263..282, the TATA box at 298..306 and prim_transcript "
     "329..435, so +1 is 329 and the 249 nt convention is exactly M14486.1:80-328 = "
     "-249..-1, the same rule PLF:4000 and PLF:4001 use. (That prim_transcript is "
     "invisible to this stage's own parser -- see the EF-1alpha row's caveat on "
     "fifteen-character feature keys -- and was read by hand.) "
     "TWO NESTED ALTERNATIVES DO CLEAR THE FLOOR AND ARE DELIBERATELY NOT OFFERED "
     "AS ROWS. 241 nt = M14486.1:80-320 is annotated exactly by the Allen Institute "
     "(PZ036121.1, PZ036141.1 -- one address), PX139666.1 and MK318530.1, three "
     "independent submissions; it stops nine bases short of +1 for no reason anyone "
     "has articulated. 264 nt = M14486.1:65-328 is annotated exactly by George "
     "Washington University (JN255690.1, JN255691.1 -- one address) and AIST "
     "(LC414435.1), two submissions, at the floor, and it ends at -1 like the 249. "
     "Adopting either BECAUSE the 249 failed is re-cutting an extent until two "
     "records agree with it, which is the move this stage exists to refuse; the "
     "module docstring already says such a re-cut 'is a curator's decision and not "
     "this program's'. They are recorded here so that decision is cheap, not taken. "
     "AND THE TRAP, WHICH IS WHY MG550105.1 IS NAMED HERE: it does NOT contain the "
     "anchor's 249 bases at all -- checked. The same survey reports it as one of "
     "three submissions drawing a 249 nt extent that differs from the primary record "
     "by a single substitution at M14486.1:146, which was not re-measured here. "
     "Counting a variant of that shape would ship a sequence that is in no primary "
     "record, which is the failure PLF:4014's caveat describes for the EF-1alpha "
     "VECTOR form. (This sentence named the PGK entry as a second example until "
     "2026-08-12 and was wrong to by then: 'not a verbatim slice of anything' was "
     "retired from that entry on 2026-08-11, and PLF:4015 ships an exact slice of a "
     "primary genomic record. It is the counter-example, not a second case.)"),
    ("H1 promoter (human)",
     "STILL HELD, and both halves of the old reason were wrong; the corrected reason "
     "is stronger and it fails on two legs rather than one. "
     "THE PROVENANCE GAP IS CLOSED, so 'no genomic record carrying the upstream "
     "promoter could be located' is retired. The record is X16612.1, 'Human gene for "
     "H1 RNA', 1057 bp, clone pMBH1, deposited by S. Altman at Yale (Baer, Nilsen, "
     "Costigan & Altman). It annotates the TATA box at 345..348 and precursor_RNA "
     "375..715, and X16612.1:152-366 -- 215 nt -- is held verbatim by FOUR "
     "independent addresses: AL355075.6:176331-176545 (Genoscope, chromosome 14 "
     "clone), AF479321.1:1615-1829 (Genome Sciences, University of Washington), "
     "Neurology at the University of Goettingen (AY640625.1:2317-2531 and "
     "AY640626.1:2373-2587, one address between them) and the anchor itself. "
     "THAT COUNT SAID THREE UNTIL 2026-08-11 AND THE FOURTH IS THE INFORMATIVE ONE: "
     "re-measured while the every-copy fix below was having its blast radius taken, "
     "Goettingen holds the 215 verbatim and draws it 5'+0/3'+1 -- their 216 nt starts "
     "on this extent's own 5' edge and runs one base past its 3' edge, which is the "
     "whole of the 215-versus-216 question in one number. The old reason was checking "
     "X15624, 'Human H1 RNA', which is a 340 bp transcript-level record and contains "
     "none of the promoter. "
     "THE CONSENSUS WAS NEVER THERE, which is the leg that now kills it. 'Three "
     "independent submissions agree on 216 nt' counted address STRINGS, which is "
     "precisely the error same_submitter() exists to prevent (finding 2 above). "
     "Every record found holding those 216 bases is one department: AY640625.1 and "
     "AY640626.1, Neurology, University of Goettingen, Waldweg 33 -- one address, "
     "one submission. FJ687158.1, DQ465352.1, MH749464.1, LT727092.1, AL355075.6 "
     "and AF479321.1 were each checked and hold no copy of the 216 nt at all. "
     "Corroborating submissions: ONE. "
     "AND THE 216 nt IS STILL NOT A SLICE OF ANYTHING. It is X16612.1:152-366 plus "
     "one further base, and that base is A where X16612.1:367 is T -- so it is 215 "
     "verbatim bases with a 216th that disagrees with the gene, and boundary_evidence "
     "would have nothing to point at. The genomic 215 nt form, which does slice "
     "cleanly, is annotated by NOBODY: four submissions hold it and zero draw it, "
     "so it fails MIN_PLACEMENTS at nought -- and it fails there under the every-copy "
     "rule exactly as it did under the first-copy one, re-measured 2026-08-11: no "
     "witness of this element carries it more than once, so there was never a second "
     "copy being overlooked here. WHAT WOULD RESCUE IT: two genuinely "
     "independent submissions annotating exactly 215 nt (today none) or exactly 216 "
     "nt (today one, and it would still not be a verbatim slice). Nothing in the "
     "schema and nothing about the anchor is now the obstacle."),
    ("AG promoter of pCAGGS, 1342 nt",
     "HELD AT ONE PLACEMENT, and the old CAG entry's central claim is confirmed "
     "harder than it was stated. These 1342 bases -- LT727518.1:3457-4798, the "
     "pCAGGS record deposited by BCCM/LMBP Gent -- share ZERO 20-mers with X17403.1, "
     "the human cytomegalovirus genome, on either strand. There is no CMV sequence "
     "in the element that most maps call a CAG promoter's front half; it is chicken "
     "beta-actin running into rabbit beta-globin. "
     "WHAT KILLS IT IS NOT THAT: it is that 'widely deposited' was false. The extent "
     "is drawn edge for edge in record after record of one series -- LT727518.1, "
     "LT726810.1 and LT726815.1 were each checked -- and every one of them carries "
     "the same submitting address, the BCCM/LMBP Gent bulk plasmid-collection "
     "deposit. That is finding 2 of the module docstring in its purest form: dozens "
     "of records, ONE submission. One placement, floor of two. "
     "A SECOND, INDEPENDENT AG-TYPE ELEMENT EXISTS AND WAS CHECKED: EF186083.1 "
     "annotates regulatory 449..1795, 1347 nt, which likewise shares ZERO 20-mers "
     "with X17403.1, and EF186088.1 carries the same address -- MCB, LUMC Leiden. "
     "So the no-CMV family is not one depositor's quirk, and it is still not a "
     "consensus: one address again."),
    ("CAG promoter, the CMV-enhancer-containing forms",
     "HELD, AND THE NUMBER IS THE FINDING: fifteen distinct extents were measured "
     "and not one of them reaches two independent submissions. The 935 nt form "
     "(JN898959.1/JN898962.1, KIST, one submission) really does begin inside the "
     "cytomegalovirus enhancer -- 210 20-mers shared with X17403.1, against zero for "
     "the AG element above -- and so do the 1699, 1722, 1740, 1733, 1728, 1721, "
     "1720, 1710, 1677, 1673, 1667 and 1647 nt forms, each with exactly one "
     "submission behind it. Fifteen single opinions is not a convention, and the "
     "reason to keep this entry separate from the AG one is that a single row named "
     "'CAG promoter' spanning both would merge an element that contains CMV with an "
     "element that provably does not."),
    ("chicken beta-actin promoter",
     "HELD, AND THIS IS THE ONE TO READ TWICE, because the two legs of the rule are "
     "satisfied by two INCOMPATIBLE extents and the near-miss is exactly the shape "
     "PLF:4006 was withdrawn over. "
     "276 nt = X00182.1:268-543 has the better boundary argument in this whole file: "
     "the record annotates a CAAT signal at 455..459 and a TATA box at 517..524, and "
     "the extent is -276..-1 against the transcription start, +1 excluded -- the "
     "PLF:4000 rule arrived at independently. It has TWO submissions annotating it "
     "exactly, and the second one must not be counted: OK413188.1 (OHSU/ONPRC) is an "
     "honest witness, and OP697986.1 is the same submitting address as OP697991.1 -- "
     "same_submitter() returns True -- which SOURCING.md section 0.6 names as a "
     "DEMONSTRATED false negative of record_is_snapgene_annotated, and which section "
     "deliberately declined to widen the screen for. The mechanical screen passes it; "
     "a curator may not. ONE honest placement. "
     "278 nt has two honest submissions (Oxford Protein Production Facility, "
     "EF372394.1 and EU733644.1, deposited 2007 and 2008; Witten/Herdecke, "
     "PQ540283.1, 2024) and no anchor: it is the 276 nt with TWO extra G in a "
     "G-homopolymer -- measured, the run is 14 G in X00182.1 and 16 G in the vector "
     "form -- so it is a verbatim slice of no primary record and reference_nt taken "
     "through that tract will silently miss whichever half of the wild population "
     "has the other length. WHAT WOULD RESCUE IT: one more independent submission "
     "drawing 276 nt exactly. Nothing else is missing."),
    ("PLtetO-1 (bacterial tet-regulated promoter)",
     "STILL NOT A ROW, BUT NO LONGER FOR THE OLD REASON. The old 'tetO / TRE / Ptet' "
     "entry was DROPPED on the ground that the name covers at least four unrelated "
     "elements and 'must be split into separately named rows before any part of it "
     "can be sourced at all'. This entry and the two below are that split, and the "
     "sourcing now exists for parts of it. "
     "A 74 nt extent -- KX682238.1:4307-4380, the lambda PL promoter with two tet "
     "operators -- is annotated edge for edge by FOUR independent addresses checked "
     "on 2026-08-11: Pacific Northwest National Laboratory (KX682238.1), Biomedical "
     "Engineering, Boston University (KM521209.1), Biological Engineering, MIT "
     "(KT893256.1, KX264176.1 -- one address) and Biology, University of Texas at "
     "Tyler (MK753225.1). None carries the SnapGene tell. It clears both legs. "
     "RE-MEASURED 2026-08-11 UNDER THE EVERY-COPY RULE and unchanged at four "
     "submissions and four placements -- but this is the element that corrected the "
     "wording of that fix, so the record is kept here too. KX264176.1 carries the 74 "
     "bases TWICE, at 7377-7450 and at 8612-8685, and the depositor drew `regulatory` "
     "edge for edge over BOTH; only a `misc_feature` overlapping the first copy "
     "differs. `place_in_record()` first called such copies 'in disagreement', which "
     "would have told a curator this witness was weaker than it is. It now reports "
     "only what it tested, that the record does not annotate its copies alike. "
     "WHAT IT DOES NOT YET HAVE, and why no row is offered: a curated name and a "
     "description written from the primary source (Lutz & Bujard 1997, Nucleic Acids "
     "Res 25:1203-1210, PMID 9092630, whose text was NOT read here), and a decision "
     "between this 74 nt "
     "and the 54 nt form nested inside it -- JX155235.1:1-54, whose placements "
     "checked here (JX155235.1, JX155240.1, JX155247.1) are all ONE address, UC "
     "Berkeley EECS. This is a designed hybrid with no natural locus, so it would be "
     "anchored on a construct record exactly as PLF:4003 and PLF:4004 are. It is the "
     "strongest unclaimed candidate in this list."),
    ("tetO7 / TRE (mammalian tet-response element)",
     "HELD, WITH THE CORROBORATION MEASURED AND A HAZARD THAT IS NOT YET ANSWERED. "
     "A 271 nt heptamer array -- MG883664.1:1376-1646 -- is annotated edge for edge "
     "by three independent addresses checked on 2026-08-11: Southwest University "
     "(MG883664.1, protein_bind), University of York (PQ260749.1) and Human "
     "Genetics, University of Michigan (PQ360726.1). A 291 nt form was reported by "
     "the same survey with three more. Both clear the floor on their face. "
     "THE HAZARD IS THAT THIS ELEMENT IS A TANDEM REPEAT, so two extents differing "
     "by 20 nt may be the same convention counted over a different number of "
     "operator copies rather than two rival boundaries -- and this stage has no way "
     "to tell those apart, because occurrences() matches a whole string and says "
     "nothing about periodicity. That is the same hazard the SV40 early promoter "
     "entry flags and does not count. Until somebody counts the repeat units in each "
     "deposit and says which extents are the same element, a row here would be "
     "asserting a boundary it has not distinguished from an artefact."),
    ("CMV-tetO2, PTight and the remaining tet hybrids",
     "HELD, AND STILL PARTLY UNSPLIT. The third and fourth elements the dropped "
     "'tetO / TRE / Ptet' entry named -- a CMV-tetO2 hybrid and a bacterial pTet -- "
     "plus a PTight/TRE-Tight form of 315 nt that the 2026-08-11 survey turned up, "
     "have not been separated into individually named elements here and none of "
     "their extents was measured against this stage's own rule. A CMV-tetO2 element "
     "additionally OVERLAPS PLF:4005's refused interval, so it cannot be worked up "
     "independently of the decision the CMV promoter row is still waiting on. "
     "Recorded so the split is complete on paper even where the evidence is not."),
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


def place_in_record(rec: Record, needle: str) -> dict:
    """Where one record puts `needle`, scoring EVERY copy of it the record holds.

    Returns {} when the record does not contain the sequence at all; otherwise
    `lo`, `hi`, `strand` and `copy` for the copy that was scored, the `placed`
    string for the report and for `notes`, the `exact` feature keys that fall on
    that copy's edges, and `occurrences`.

    SPLIT OUT OF `verify()` SO `self_test()` CAN DRIVE IT, for the same reason
    `corroborating_submissions()` is: inside the exemplar loop this was reachable
    only with the network or the cache, which is exactly the condition under
    which a rule quietly stops being tested. The rule it decides is
    MIN_PLACEMENTS, and MIN_PLACEMENTS is the whole of what makes
    `boundary_rule = consensus_of_insdc` more than an assertion.

    WHY EVERY COPY AND NOT THE FIRST -- a 2026-08-11 fix to the IMPLEMENTATION of
    the rule and not to the rule. MIN_PLACEMENTS asks whether an independent
    submission "annotates a feature at EXACTLY this extent"; a depositor whose
    construct carries the element twice and who draws our edges over the SECOND
    copy has done precisely that. Until this build the loop read `hits[0]` and
    nothing else, so it scored one copy and threw the rest away -- while the dict
    it returned carried `occurrences` and therefore knew all along that the other
    copies were there.

    MEASURED, on the mouse PGK promoter -- which was HELD when this was written
    and is `PLF:4015` since the curator issued it on 2026-08-12. The 508 nt
    element is BX469914.4:13192-13699. AB242435.1 (Central Institute for
    Experimental Animals, Kawasaki) carries it TWICE: at 374-881, where the
    depositor drew `regulatory 366..881` -- 516 nt, 5'+8/3'+0 -- and again at
    2089-2596, where the depositor drew `regulatory 2089..2596`, exactly these
    508 bases. Scoring copy 1 alone saw the 516 and never the 508, so on the rule
    as IMPLEMENTED that submission placed nothing and on the rule as WRITTEN it
    placed the element edge for edge.
    """
    hits = occurrences(rec.sequence, needle)
    if not hits:
        return {}

    scored = []
    for lo, hi, _strand in hits:
        # Deliberately NOT named `near`: an earlier version of `verify()` used
        # that name for the anchor's features as well and the exemplar loop
        # silently overwrote them.
        near_feats = [
            (k, flo, fhi) for (k, flo, fhi, _s) in rec.features
            if overlaps(lo, hi, flo, fhi) and (fhi - flo + 1) <= 12 * len(needle)
        ]
        scored.append(near_feats)

    def render(i: int) -> str:
        lo, hi, strand = hits[i]
        return ", ".join(
            f"{k} {offsets((lo, hi), (flo, fhi), strand)}" for k, flo, fhi in scored[i][:4]
        ) or "nothing over them at all"

    # The corroboration test, computed from the intervals rather than from the
    # rendered string -- `render()` truncates to four features for the report,
    # and a fifth feature landing exactly on our edges would be invisible to
    # anything that parsed it. Equality of the raw interval IS `5'+0/3'+0`;
    # offsets() is only how it is rendered.
    def exact_keys(i: int) -> list:
        lo, hi, _strand = hits[i]
        return [k for k, flo, fhi in scored[i] if (flo, fhi) == (lo, hi)]

    # ONE RECORD CONTRIBUTES ONE PLACEMENT, however many of its copies are drawn
    # edge for edge, and the next reader's question is answered here so it is not
    # re-litigated: YES, a record that draws copy 1 inexactly and copy 2 exactly
    # is ONE exact placement, and a record that draws both exactly is still one.
    # The reason is that the unit of corroboration in this stage is the
    # SUBMISSION and never the copy. `corroborating_submissions()` already
    # collapses one lab's several RECORDS into a single opinion, because two
    # records from one lab are one opinion; two copies inside ONE record are less
    # independent than that and not more, so they cannot be allowed to buy what
    # two records from one lab cannot. That the depositor also drew different
    # edges elsewhere in the same construct is a fact about that record's
    # internal consistency -- it is disclosed in `placed` below rather than being
    # allowed to cancel a draw the depositor really made.
    pick = next((i for i in range(len(hits)) if exact_keys(i)), 0)
    lo, hi, strand = hits[pick]
    placed = render(pick)

    # WHICH COPY THIS DESCRIBES, said out loud when and only when the record does
    # NOT ANNOTATE ITS COPIES ALIKE. Before the fix the scored copy was always
    # copy 1, so the string was merely incomplete; now it is chosen by a rule a
    # reader cannot invert from the offsets alone, and an evidence string a
    # curator cannot resolve is worse than one that is plainly wrong.
    #
    # THE TRIGGER IS THE ANNOTATION AND NOT THE EXTENT, and the wording has to
    # say so, because those are not the same test and the stronger word would be
    # a claim nobody checked. MEASURED 2026-08-11 on KX264176.1, which the
    # PLtetO-1 entry in HELD already names: it carries that 74 nt element twice,
    # at 7377-7450 and at 8612-8685, and the depositor drew `regulatory` EDGE FOR
    # EDGE OVER BOTH -- so those copies agree about this extent as completely as
    # two copies can -- while a `misc_feature` overlaps only the first. The
    # difference is real and worth disclosing, but calling such copies "in
    # disagreement" would tell a curator the corroboration is shakier than it is,
    # in a file whose entire subject is how strong the corroboration is. What is
    # true, and all that is true, is that the record does not annotate its copies
    # alike; the enumeration that follows is what lets a curator see which kind
    # of difference it is, and check 4d drives exactly this case.
    #
    # Where every copy renders the same there is nothing to disambiguate and
    # nothing is added -- which is not a convenience but the condition under
    # which this fix leaves the rows in the table untouched. All four multi-copy
    # witnesses in ITEMS today are of that kind: V01146 (T7, 7 copies) and
    # AY288927 (SP6, 3 copies) annotate nothing over any copy, AJ318471 (T3, 3
    # copies) draws 5'+0/3'+4 over every one, and U13859 (rrnB T2, 2 copies)
    # draws 5'+0/3'+0 over both.
    renders = [render(i) for i in range(len(hits))]
    if len(set(renders)) > 1:
        placed += (
            f" [scored on copy {pick + 1} of {len(hits)}, at {lo}-{hi}:{strand};"
            f" this record does NOT annotate its copies alike -- "
            + "; ".join(
                f"copy {i + 1} at {h[0]}-{h[1]}:{h[2]} places {r}"
                for i, (h, r) in enumerate(zip(hits, renders))
            )
            + "]"
        )

    return {
        "lo": lo,
        "hi": hi,
        "strand": strand,
        "copy": pick + 1,
        "placed": placed,
        "exact": exact_keys(pick),
        "occurrences": len(hits),
    }


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
        # EVERY copy this record holds is scored, not just the first; see
        # `place_in_record()` for why, and for the PGK record that proved it.
        placement = place_in_record(rec, item.sequence)
        if not placement:
            absent.append(acc)
            lines.append(f"    exemplar {acc:12s} DOES NOT CONTAIN the sequence -- not counted")
            continue
        lo, hi, strand = placement["lo"], placement["hi"], placement["strand"]
        placed = placement["placed"]
        entry = {
            "accession": rec.accession or acc,
            "is_anchor": acc == item.anchor,
            "snapgene": rec.snapgene,
            "submitter": rec.submitter,
            "meta": meta,
            **placement,
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
            f"x{placement['occurrences']}; depositor places {placed}"
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


def allocation(items: tuple[Convention, ...] | None = None) -> list:
    """(id, item) for every DECLARED item, in declaration order.

    The id comes from the item's INDEX and from nothing else, so this list has
    one entry per declaration whatever any check later decides -- a row refused
    on its evidence, and a row the curator withdrew, both keep their place in it.
    That is the property the comment above `ITEMS` measures the cost of losing.

    `build()` iterates THIS, rather than re-deriving `ID_BASE + i` in its own
    loop, so that `self_test()` can drive the real allocator over a mutated tuple
    instead of over a copy of the expression it is meant to be testing.
    """
    src = ITEMS if items is None else items
    return [(f"PLF:{ID_BASE + i:04d}", it) for i, it in enumerate(src)]


def withdrawn_ids(items: tuple[Convention, ...] | None = None) -> dict:
    """{id: reason} for every item the curator has withdrawn.

    Read by `build.py`'s id-stability audit, which is otherwise right to call a
    published id's disappearance fatal. A withdrawal is the one absence that has
    an answer, and this is where the answer is kept -- so an id that vanishes
    WITHOUT a reason here still stops the build, which is the point.
    """
    return {rid: it.withdrawn for rid, it in allocation(items) if it.withdrawn}


def build(refresh: bool) -> tuple[list, list]:
    """Return (rows, report), the shape every other stage returns."""
    # RUN THE GATES BEFORE EMITTING ANYTHING FROM THEM. `self_test()` has said
    # since it was written that its checks "run on every build", and until
    # 2026-08-11 that was only true of `python features/build/stage_classb.py`
    # -- `build.py` called this function and never that one, so on the path that
    # actually writes features.tsv the stage's own gates never ran. They need no
    # network, so there was never a reason for that; it was an oversight, and a
    # sentence claiming otherwise is worse than no sentence.
    report, rows = list(self_test()), []
    for ordinal, (rid, it) in enumerate(allocation(), start=1):
        report.append(f"  {rid} {it.name}")
        # WITHDRAWN BY THE CURATOR. Not a check failing -- a decision taken -- so
        # it is reported in its own words rather than as a DROP, and the reason
        # travels with the id. The item stayed in `ITEMS` to get here, which is
        # what keeps `rid` meaning this element and no other, forever.
        if it.withdrawn:
            report.append(f"    WITHDRAWN -- {it.withdrawn}")
            continue
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

    # 4d. EVERY COPY IS SCORED, NOT THE FIRST ONE. This is the check for the
    #     2026-08-11 fix in `place_in_record()`, and the record it is modelled on
    #     is real: AB242435.1 carries the mouse PGK promoter TWICE and annotates
    #     `regulatory 366..881` over the first copy (which sits at 374-881, so
    #     5'+8/3'+0) and `regulatory 2089..2596` over the second (2089-2596,
    #     edge for edge). Reading `hits[0]` saw the 516 nt draw and never the 508,
    #     so PGK measured ONE placement on the rule as implemented and TWO on the
    #     rule as written.
    #
    #     THE FIXTURE IS SYNTHETIC AND THE SHAPE IS AB242435's, because a test
    #     that needed that record would need the network or the cache -- the
    #     condition under which a rule stops being tested. It is driven through
    #     `parse_embl()` rather than through a hand-built `Record`, so what runs
    #     is the parser the stage actually uses.
    twice_seq = (
        "     tttttttttg attacaggcc ttaagctcga aaaaaaaaaa aaaaaaaaag attacaggcc     60\n"
        "     ttaagctcgc ccccccccc                                                 79\n"
    )

    def two_copy_record(ft: str) -> Record:
        return parse_embl(
            "ID   XX05; SV 1; circular; other DNA; STD; SYN; 79 BP.\n"
            + ft
            + "SQ   Sequence 79 BP;\n" + twice_seq + "//\n"
        )

    elem = "GATTACAGGCCTTAAGCTCG"
    #     The literal coordinates below are the claim, not a restatement of the
    #     fixture: if the sequence above is ever edited without moving them, this
    #     is the assertion that says so instead of the rest of the block quietly
    #     testing a one-copy record.
    disagree = two_copy_record(
        "FT   regulatory      7..29\n"
        'FT                   /regulatory_class="promoter"\n'
        "FT   regulatory      50..69\n"
        'FT                   /regulatory_class="promoter"\n'
    )
    must("the fixture really does hold the element twice",
         occurrences(disagree.sequence, elem) == [(10, 29, "+"), (50, 69, "+")])
    #     The control that makes the three assertions after it mean something: the
    #     FIRST copy is drawn INEXACTLY, so anything that scores only `hits[0]`
    #     must come back with no exact placement at all.
    must("the fixture's first copy is drawn inexactly, 5'+3 like AB242435's",
         [k for k, flo, fhi, _s in disagree.features if (flo, fhi) == (10, 29)] == []
         and offsets((10, 29), (7, 29), "+") == "5'+3/3'+0")
    p = place_in_record(disagree, elem)
    must("a record that draws its SECOND copy exactly places the element exactly",
         p["exact"] == ["regulatory"])
    must("and the copy reported is the one that was drawn exactly, not the first",
         (p["lo"], p["hi"], p["copy"], p["occurrences"]) == (50, 69, 2, 2))
    #     A row whose evidence string is ambiguous is worse than one that is
    #     merely wrong: the offsets alone cannot tell a curator which of two
    #     copies they describe, so a record that does not annotate its copies
    #     alike has to say which one this describes.
    must("when the copies are not annotated alike the report names the copy it scored",
         "scored on copy 2 of 2, at 50-69:+" in p["placed"]
         and "does NOT annotate its copies alike" in p["placed"])
    must("and discloses what the other copy was drawn as, so nothing is hidden",
         "copy 1 at 10-29:+ places regulatory 5'+3/3'+0" in p["placed"])
    #     ONE RECORD, ONE PLACEMENT -- the decision `place_in_record()` documents.
    #     A depositor who drew our edges once is one opinion, so this record on
    #     its own still cannot reach the floor.
    must("a record whose second copy is exact is ONE corroborating submission",
         len(corroborating_submissions({"kawasaki": [p]})) == 1)
    must("so one such record on its own still does not meet MIN_PLACEMENTS",
         len(corroborating_submissions({"kawasaki": [p]})) < MIN_PLACEMENTS)

    #     AND THE CONTROL THAT PROTECTS EVERY ROW ALREADY SHIPPING. Nothing is
    #     appended when the copies AGREE, because then the sentence is true of all
    #     of them and there is nothing to disambiguate. All four multi-copy
    #     witnesses in ITEMS are of that kind, so this fix leaves their `notes`
    #     byte for byte where they were: the two cases below are U13859's (every
    #     copy exact, PLF:4009) and AJ318471's (every copy drawn the same wrong
    #     way, PLF:4012).
    agree_exact = place_in_record(two_copy_record(
        "FT   regulatory      10..29\n"
        'FT                   /regulatory_class="promoter"\n'
        "FT   regulatory      50..69\n"
        'FT                   /regulatory_class="promoter"\n'
    ), elem)
    must("copies that all place it exactly add no disambiguation, as U13859's do",
         agree_exact["placed"] == "regulatory 5'+0/3'+0" and agree_exact["copy"] == 1)
    must("and they still place it exactly", agree_exact["exact"] == ["regulatory"])
    #     The other half of "one record, one placement", and it needs a fixture of
    #     its own: a record BOTH of whose copies are drawn edge for edge must
    #     still be ONE submission. Two copies inside one record cannot buy what
    #     two records from one lab cannot, and `exact` is a flag on the record
    #     rather than a tally of copies precisely so that it cannot.
    must("a record with BOTH copies drawn exactly is still ONE submission",
         len(corroborating_submissions({"kawasaki": [agree_exact]})) == 1
         and len(agree_exact["exact"]) == 1)
    agree_wrong = place_in_record(two_copy_record(
        "FT   regulatory      7..29\n"
        'FT                   /regulatory_class="promoter"\n'
        "FT   regulatory      47..69\n"
        'FT                   /regulatory_class="promoter"\n'
    ), elem)
    must("copies that are all drawn the same wrong way add none either, as AJ318471's",
         agree_wrong["placed"] == "regulatory 5'+3/3'+0" and agree_wrong["copy"] == 1)
    must("and drawing every copy the same wrong way corroborates nothing",
         agree_wrong["exact"] == [])
    #     THE CASE THAT DECIDES THE WORDING, and it is a real record: KX264176.1
    #     carries the PLtetO-1 element HELD names TWICE, at 7377-7450 and
    #     8612-8685, draws `regulatory` edge for edge over BOTH, and overlaps only
    #     the first with a `misc_feature`. The copies therefore agree about this
    #     extent completely and are still not annotated alike. Measured
    #     2026-08-11 while this fix's blast radius was being taken; the fixture
    #     below is that shape, with the extra feature over copy 1 only.
    #
    #     TWO WAYS TO GET THIS WRONG, and this block refuses both. Suppressing
    #     the note because the copies agree on the extent would hide from a
    #     curator that the record annotates its copies differently at all; and
    #     the note may not say the copies DISAGREE, because here they do not --
    #     the trigger is `render()` differing, which is a statement about the
    #     annotation and not about the boundary, and a file whose whole subject
    #     is the strength of corroboration must not overstate a weakness any more
    #     than a strength.
    exact_both_extra = place_in_record(two_copy_record(
        "FT   regulatory      10..29\n"
        'FT                   /regulatory_class="promoter"\n'
        "FT   regulatory      50..69\n"
        'FT                   /regulatory_class="promoter"\n'
        "FT   misc_feature    1..40\n"
    ), elem)
    must("a record that draws both copies exactly but annotates one further says so",
         "scored on copy 1 of 2, at 10-29:+" in exact_both_extra["placed"]
         and "does NOT annotate its copies alike" in exact_both_extra["placed"])
    must("and shows both copies drawn edge for edge, rather than calling that a disagreement",
         "copy 1 at 10-29:+ places regulatory 5'+0/3'+0, misc_feature 5'+9/3'+11"
         in exact_both_extra["placed"]
         and "copy 2 at 50-69:+ places regulatory 5'+0/3'+0" in exact_both_extra["placed"]
         and "DISAGREE" not in exact_both_extra["placed"])
    must("and it is still one exact placement, scored on the first exact copy",
         exact_both_extra["exact"] == ["regulatory"] and exact_both_extra["copy"] == 1)
    must("a record that does not hold the sequence places nothing",
         place_in_record(disagree, "ACGTACGTACGTACGTACGT") == {})

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

    # 8. WITHDRAWING A ROW MUST NOT MOVE ANY OTHER ID, and this is the check
    #    that says so. It is here because the failure it guards against is
    #    invisible: deleting an item from `ITEMS` leaves a tuple that is
    #    perfectly well-formed, builds without a murmur from this stage, and
    #    silently repoints five published accessions at different elements.
    #
    #    THE PIN IS BY NAME AND NOT BY ARITHMETIC. Asserting `ITEMS[7].name ==
    #    ITEMS[7].name` would pass under any renumbering; the five ids below are
    #    written out with the elements they are PUBLISHED as, in
    #    features/features.tsv, so the literal is an independent statement about
    #    the world and not a restatement of the code.
    #    The last three were appended on 2026-08-11 and are `proposed`, not
    #    signed -- but they are in features/features.tsv under these ids from
    #    that build onward, so an id that moves under them is as wrong as one
    #    that moves under the five above, and pinning them is what stops the
    #    next person tidying the T3 promoter up next to PLF:4000.
    published = {
        "PLF:4006": "CMV enhancer",
        "PLF:4007": "T7 terminator",
        "PLF:4008": "rrnB T1 terminator",
        "PLF:4009": "rrnB T2 terminator",
        "PLF:4010": "bGH poly(A) signal",
        "PLF:4012": "T3 promoter",
        "PLF:4013": "araBAD promoter",
        "PLF:4014": "EF-1alpha promoter (human)",
    }
    live = {rid: it.name for rid, it in allocation()}
    for rid, nm in published.items():
        must(f"{rid} still names the element it was published as, {nm}",
             live.get(rid) == nm)

    # Withdraw one, the RIGHT way: replace the declaration in place, leave it in
    # the tuple. Every id, including the withdrawn one's, must be untouched.
    #
    # `at` is SEARCHED FOR and its absence is a stated failure, not a
    # StopIteration. Deleting the item is precisely the mistake the block above
    # exists to catch, so being told about it by a traceback out of the test's
    # own scaffolding would be the worst available outcome.
    at = next((i for i, it in enumerate(ITEMS) if it.name == "CMV enhancer"), None)
    must("the CMV enhancer is still DECLARED, withdrawn or not", at is not None)
    if at is None:
        # AND EVERY FAILURE THE PIN ACTUALLY FOUND, not only this one. `must`
        # accumulates its labels into `fails`, and on the ordinary path at the
        # foot of this function `fails` is what the exit message is built from --
        # but an early `raise` here discards it. Until 2026-08-11 that is what
        # happened: deleting the declaration for real printed one sentence about
        # the enhancer and no evidence whatever that four further published ids
        # had moved with it, while `PROPOSED.md` and `CHANGELOG.md` both say the
        # pin fails on all five. Those five reassignments ARE the subject of this
        # check; a message that measures them and then throws them away leaves
        # its own claim unwitnessed, which is the failure mode this file is
        # least entitled to.
        raise SystemExit(
            "stage_classb self-test failed: the CMV enhancer is not in ITEMS. If it "
            "was deleted to withdraw it, every id after it has just shifted down one "
            "and PLF:4006 now means the T7 terminator. Restore the declaration and "
            "set its `withdrawn` field instead; see the comment above ITEMS.\n"
            "  Every pin that failed, so the reassignment is visible and not merely "
            "asserted:\n    " + "\n    ".join(fails)
        )
    marked = ITEMS[:at] + (replace(ITEMS[at], withdrawn="fixture"),) + ITEMS[at + 1:]
    after = {rid: it.name for rid, it in allocation(marked)}
    must("withdrawing a row moves no other id", after == live)
    must("the withdrawn row keeps its own id too, so it can never be reissued",
         after["PLF:4006"] == "CMV enhancer")
    must("the withdrawal is reported against that id, with its reason",
         withdrawn_ids(marked) == {"PLF:4006": "fixture"})
    must("and nothing is withdrawn unless it says so",
         withdrawn_ids(ITEMS[:at] + ITEMS[at + 1:]) == {})

    # THE INVERTED CONTROL, and the reason the checks above are worth anything.
    # Withdraw the same row the WRONG way -- delete it -- and the pin must fail,
    # naming the exact five reassignments PROPOSED.md measured.
    deleted = {rid: it.name for rid, it in allocation(ITEMS[:at] + ITEMS[at + 1:])}
    must("deleting the item instead renumbers, so the pin above can fail",
         deleted != live)
    must("and it renumbers by exactly one place, five published ids deep",
         [deleted.get(f"PLF:{n}") for n in range(4006, 4011)]
         == ["T7 terminator", "rrnB T1 terminator", "rrnB T2 terminator",
             "bGH poly(A) signal", "SV40 early poly(A) signal"])

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
    withdrawn = withdrawn_ids()
    print(f"  {len(ITEMS)} allow-listed items, {len(HELD)} worked up and held, "
          f"{len(withdrawn)} withdrawn")
    # `self_test()` is NOT called here any more: `build()` runs it and returns
    # its lines at the head of the report, so this path and the `build.py` path
    # run the same gates rather than only this one.
    rows, report = build(args.refresh)
    print("\n".join(report))
    print(f"\n{len(rows)}/{len(ITEMS) - len(withdrawn)} declared row(s) verified "
          f"({len(withdrawn)} withdrawn and not offered)")
    return 0 if len(rows) == len(ITEMS) - len(withdrawn) else 1


if __name__ == "__main__":
    raise SystemExit(main())
