#!/usr/bin/env python3
"""Stage 2 — UniProt -> ENA CDS, for natural proteins absent from AMRFinderPlus.

`features/SOURCING.md` §2 Stage 2. The chain is five steps and step 4 is the
whole point:

    1. UniProt JSON               rest.uniprot.org/uniprotkb/{acc}.json
    2. EMBL cross-reference       -> properties['ProteinId']   (a *pinned* one)
    3. ENA CDS nucleotides        ebi.ac.uk/ena/browser/api/fasta/{ProteinId}
    4. translate, and REQUIRE an exact match to the UniProt canonical
    5. parent record + coordinates for `boundary_evidence`

Why the ProteinId is hard-coded rather than taken from the entry
----------------------------------------------------------------

A UniProt accession is not one sequence. `P62593` is a merged entry covering
TEM-1/2/3/4/5/6/8/16/24 whose nine EMBL cross-references point at *different
alleles with different sequences*; `xref[0]` is one of the ones that matches,
which is worse than if it were wrong, because it teaches you to trust the
index. `P03023` (lacI) is worse still and was measured, not assumed: 11 EMBL
xrefs, only 2 of which translate to the canonical. `xref[0]` is wrong, and so is
J01636 — the classic lac operon record, the most famous accession of the set.
Three of the wrong ones are the lacI carried in pGEX-6P-1/-2/-3, i.e. the trap
is sitting inside cloning vectors people use. Only the two whole-genome records
are right.

Those counts are re-measured on every build by `survey_xrefs()` and written into
each row's `notes`. They used to be prose here and in `caveat` strings, and prose
about arithmetic rots silently: the hand-written version of the lacI note labelled
the deviating records `S286L` when, relative to this database's own pinned
canonical, the deviation is `L286S`.

So `ITEMS` below pins the exact `ProteinId` that was verified, and the chain
*re-derives* the verification every build rather than trusting the pin. The pin
chooses which candidate to test; it does not excuse the test.

Nothing here is patched. If a translation disagrees, the item is dropped with a
printed reason. A silently corrected sequence is the failure mode this whole
database exists to prevent, and a "fix" would be indistinguishable from a
correct row in the output.

Why a control runs on every build
---------------------------------

A check that cannot fail proves nothing. `run_control()` re-runs the documented
TEM-1 trap on live data: `AAB59737.1` must match `P62593` and `CAA45828.1` must
NOT. If the negative half ever passes, the exact-match gate has stopped working
and this module refuses to emit anything — which is the only condition under
which "all items verified" would be a lie rather than a result.

Licences differ *within one row*, which is why provenance is per field
---------------------------------------------------------------------

  * `name`, `aliases`, `reference_aa`  — UniProt, **CC BY 4.0**, and the
    UniProt LICENSE additionally imposes a per-copy notice condition
    (SOURCING.md §1, disagreement 4). `features/NOTICE` must carry the UniProt
    Consortium copyright line before these rows ship. NOTE FOR WHOEVER UPDATES
    IT: NOTICE currently says "All names, aliases and descriptions are written
    by the Polylinker contributors". That sentence becomes false the moment
    this stage lands — the descriptions are still ours, the names are not.
  * `reference_nt`, `boundary_evidence` — ENA, **INSDC-free**, with a per-record
    credit expectation to the original submitter. UniProt's CC BY does not and
    cannot cover these; they have different provenance.
  * `boundary_rule`, `description`      — **own-work**.

Ingest is a field-level allow-list, never "everything the API returns"
---------------------------------------------------------------------

These entries also carry DrugBank, KEGG, ChEMBL and DrugCentral cross-references.
KEGG states it "is not a public database" and that non-academic use "requires a
commercial license"; DrugBank's terms returned HTTP 403 and are unverified.
UniProt cannot relicense any of them. `pick_uniprot()` therefore parses named
fields and drops the rest before anything reaches a `Row`, and asserts that no
non-EMBL database name survived — so the allow-list is enforced rather than
merely intended.

Usage
-----
    python features/build/stage_uniprot.py            # from cache
    python features/build/stage_uniprot.py --refresh  # re-fetch everything
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

# Reused, never reimplemented. An independent translate() could agree with the
# catalogue exactly where build.py's disagrees, greenlighting rows the real
# builder would drop — the verification would then be measuring the wrong code.
from build import (  # noqa: E402
    CACHE,
    TODAY,
    Row,
    cached_meta,
    cds_matches_protein,
    esc,
    fetch,
    parse_fasta,
    translate_cds,
)

UNIPROT_JSON = "https://rest.uniprot.org/uniprotkb/{}.json"
ENA_FASTA = "https://www.ebi.ac.uk/ena/browser/api/fasta/{}"
ENA_EMBL = "https://www.ebi.ac.uk/ena/browser/api/embl/{}"

# features/SOURCING.md §0.6, checked by features/build/insdc_posture.py.
#
# This stage DOES read a depositor's feature table. `ena_cds_record()` walks it,
# and `boundary_evidence` is the depositor's own location expression copied out
# verbatim. That is exactly the route SnapGene's boundary convention travels,
# and a declaration here saying "we never look" would be false.
#
# What makes it defensible is not that we do not look, it is that the extent is
# nobody's to choose. The sliced nucleotides have to translate residue for
# residue to a named UniProt canonical, and a CDS that does not is dropped
# rather than trimmed to fit. Agreement with any vendor's convention is
# therefore explained by the arithmetic — which is the only kind of agreement
# this project can defend, and the distinction the whole posture vocabulary
# exists to make.
INSDC_POSTURE = {
    "posture": "feature_table_forced",
    "reason": (
        "ena_cds_record() parses the depositor's CDS feature and boundary_evidence is "
        "that depositor's own location expression. The extent is nevertheless forced: "
        "the nucleotides must translate residue for residue to the UniProt canonical "
        "protein the row is keyed on, and a difference anywhere but the initiator drops "
        "the row instead of moving the boundary. A depositor who annotated their "
        "plasmid in an editor cannot shift that extent without breaking the equality, "
        "so no convention of anyone's rides in on it."
    ),
    "forced_by": "cds_matches_protein",
}

# Stage 2's reserved block. IDs are what a curator signs off against: if Stage 2
# numbered from "wherever Stage 1 ended", adding one resistance marker would
# renumber every natural-protein row, and a signature on PLF:0009 would silently
# come to mean a different feature. Blocks are cheap; renumbering is not.
#
# `build.STAGES` is the authority on where this block starts, and `load_stage()`
# refuses to run a stage whose declared base disagrees with it. That check is why
# these are constants and not comments: this file originally said 101, build.py
# said 1000, and the two disagreeing silently produced PLF:1100 as the first
# Stage 2 id — deterministic, stable, and a lie about which block the row is in.
# Nothing had been published from this block yet, so it was free to fix; after
# publication it would not have been.
PLF_BLOCK_BASE = 1000
PLF_BLOCK_SIZE = 1000
ID_BASE = PLF_BLOCK_BASE


# --------------------------------------------------------------------------
# The allow-list: which proteins, which cross-reference, and what we expect


@dataclass(frozen=True)
class Natural:
    """One natural protein, with the single EMBL cross-reference we trust."""

    uniprot_acc: str
    protein_id: str
    """The EMBL ProteinId verified to translate to this entry's canonical.

    Pinned deliberately. See the module docstring: for several of these entries
    most cross-references are a different allele, a fragment, or an alternative
    start, and one of them is an *engineered* variant filed under the wild-type
    accession."""
    parent: str
    """Versioned parent record expected on the ENA `PA` line. Cross-checked, not
    trusted: if ENA re-versions a record under us we want the build to say so
    rather than quietly citing coordinates from a record we never read."""
    nt_len: int
    aa_len: int
    description: str
    """Ours, written from the primary literature. Never from SnapGene, Addgene,
    pLannotate or any vendor map — prose is the layer where the claim that this
    database is not derived from theirs is most easily broken."""
    patent_flag: bool = False
    caveat: str = ""
    """Something the curator must decide or must not be allowed to assume. Goes
    into `notes` verbatim; it is not decoration."""


ITEMS: tuple[Natural, ...] = (
    Natural(
        "P03023", "AAC73448.1", "U00096.3", 1083, 360,
        "Repressor of the Escherichia coli lactose operon. A tetramer that binds "
        "the operator O1 -- with auxiliary sites O2 and O3, which it loops the "
        "intervening DNA to reach -- and blocks transcription initiation. "
        "Allolactose, or the non-hydrolysable analogue IPTG, binds the core "
        "domain and lowers operator affinity, releasing the operon. Supplied in "
        "cis or from the host to hold lac-derived promoters silent until induction.",
        caveat=(
            "Worst multi-allele trap in this stage, and the reason the pin is "
            "on a whole-genome record: even J01636, the classic lac operon "
            "entry a biologist would reach for first, carries a residue-286 "
            "polymorphism, and so does the lacI of pGEX-6P-1/-2/-3. Others are "
            "longer, read from an upstream alternative start. The measured "
            "cross-reference survey above is the evidence; it is regenerated "
            "every build rather than written down here."
        ),
    ),
    Natural(
        "P00722", "AAA24053.1", "J01636.1", 3075, 1024,
        "Escherichia coli beta-galactosidase. A homotetramer that hydrolyses the "
        "beta-1,4 linkage of lactose to glucose and galactose, and cleaves the "
        "chromogenic substrate X-gal to a product that oxidises into an insoluble "
        "blue indigo dye -- the read-out behind blue/white screening.",
        caveat=(
            "This is the FULL-LENGTH parent, not lacZ-alpha. The alpha peptide "
            "is an N-terminal sub-region whose extent is a convention set by the "
            "vector (pUC and M13 constructs differ), not a fact derivable from "
            "this record, and UniProt has no entry for it: protein_name:\"lacZ "
            "alpha\" returns 0, and gene:lacZ + organism 83333 + length 1-200 "
            "returns 0. lacZ-alpha must be hand-curated as literature_defined "
            "with a citation to the vector paper that defines it."
        ),
    ),
    Natural(
        "P0A9E0", "AAC73175.1", "U00096.3", 879, 292,
        "Dual-function regulator of the Escherichia coli L-arabinose regulon. "
        "Without arabinose it bridges the araI1 and araO2 half-sites and loops "
        "the DNA, repressing araBAD; arabinose binding reorients the dimerisation "
        "arms, breaks the loop, and the protein activates transcription from "
        "PBAD instead. The switch behind arabinose-inducible expression.",
    ),
    Natural(
        "P03034", "AAA96581.1", "J02459.1", 714, 237,
        "Repressor of bacteriophage lambda. Binds the OL and OR operator triplets "
        "to silence PL and PR while activating its own promoter PRM, which is what "
        "makes lysogeny self-sustaining. In the SOS response, RecA-stimulated "
        "autocleavage severs the DNA-binding domain from the dimerisation domain "
        "and the lytic programme starts.",
        caveat=(
            "Two things a reader will otherwise get wrong. (1) cI857, the "
            "temperature-sensitive allele actually used for heat induction, is "
            "NOT this sequence and cannot be reached from UniProt: it has no "
            "accession (UniProt does not give point mutants their own), and the "
            "defining substitution is deliberately not written here. (2) The "
            "/product qualifier of J02459.1 is unusable as a name source -- "
            "three different CDSs in that record all read "
            "/product=\"rexb (exclusion;144)\", including this one, and the ENA "
            "FASTA description line inherits the wrong name. SOURCING.md Stage 3 "
            "rule 1 says never key on /label; /product is no better here."
        ),
    ),
    Natural(
        "P00573", "CAA24390.1", "V01146.1", 2652, 883,
        "Single-subunit DNA-dependent RNA polymerase of bacteriophage T7. It "
        "recognises only its own 17 bp class III promoter, needs no sigma factor "
        "or accessory subunits, and elongates several-fold faster than the host "
        "enzyme. That orthogonality -- a promoter the host cannot read and a "
        "polymerase that ignores host promoters -- is what a T7 expression system "
        "is built on.",
        caveat=(
            "The divergent cross-reference CAA24333.1 reads as a frameshift or "
            "sequencing artefact in the old V01127 record, and it is still "
            "883 aa -- so a length check passes it and only a residue-by-residue "
            "comparison does not. Where and how far it diverges is measured in "
            "the cross-reference survey above, not asserted here."
        ),
    ),
    Natural(
        "P06956", "AAQ13978.1", "AF234172.1", 1032, 343,
        "Site-specific tyrosine recombinase of bacteriophage P1. It recombines "
        "two 34 bp loxP sites -- a 13 bp inverted repeat either side of an "
        "8 bp asymmetric spacer -- with no accessory protein, no host factor and "
        "no high-energy cofactor. Site orientation decides the outcome: head-to-head "
        "inverts the intervening DNA, head-to-tail excises it as a circle.",
        patent_flag=True,
        caveat=(
            "PATENT: Cre/loxP carries a dense historical estate. Flagged, not "
            "adjudicated -- no patent database was searched. SOURCING.md Risk 6."
        ),
    ),
    Natural(
        "P03870", "AAB59340.1", "J01347.1", 1272, 423,
        "Site-specific tyrosine recombinase encoded by the 2-micron plasmid of "
        "Saccharomyces cerevisiae. Recombines 34 bp FRT sites by the same "
        "strand-exchange chemistry as Cre. In its native role it inverts the "
        "segment between the plasmid's inverted repeats, which converts "
        "bidirectional replication into a rolling-circle-like amplification and "
        "restores plasmid copy number.",
        patent_flag=True,
        caveat=(
            "Two things. (1) BOUNDARY: this CDS wraps the origin of a circular "
            "record -- join(J01347.1:5570..6318,J01347.1:1..523). It is recorded "
            "as the depositor's own INSDC location string precisely because the "
            "acc:start-end:+ form Stage 1 uses cannot express it, and rendering "
            "it as 1..523 or 5570..6318 would be a wrong boundary that looks "
            "well-formed. (2) PATENT, and the exposure is worse than this row: "
            "FLPe and FLPo, the variants actually used, are absent from UniProt "
            "entirely -- querying \"FLPe OR FLPo\" returns 10 hits, every one an "
            "unrelated bacterial FlpE pilus-assembly protein. They must be "
            "hand-curated and they carry the live patent exposure."
        ),
    ),
    Natural(
        "Q9T221", "CAA07153.1", "AJ006589.3", 1818, 605,
        "Serine integrase of Streptomyces phage phiC31. Recombines the phage attP "
        "site with the bacterial attB site to give attL and attR. Unlike the "
        "tyrosine recombinases the reaction is unidirectional on its own: "
        "reversing it additionally requires the phage-encoded recombination "
        "directionality factor, so an integration event is stable without any "
        "selection to hold it.",
        patent_flag=True,
        caveat=(
            "QUALITY PRECONDITION NOT MET, stated rather than smoothed over: "
            "SOURCING.md section 2 scopes Stage 2 to reviewed/Swiss-Prot entries with "
            "annotation_score 3-5. Q9T221 is UNREVIEWED (TrEMBL), "
            "annotation_score 1.0, and has only a submission name. Searching "
            "taxon 10719 returns 5 reviewed entries and none is the integrase; "
            "the only reviewed phiC31 recombination protein is Q9T216, the "
            "recombination directionality factor, which is a DIFFERENT protein "
            "and must not be substituted for it. The translation check passes "
            "exactly, so the sequence is as well evidenced as any other row "
            "here; the upstream curation is not. PATENT: site-specific "
            "integration by phiC31 is patented. Flagged, not adjudicated."
        ),
    ),
    Natural(
        "P05655", "CAB15450.1", "AL009126.3", 1422, 473,
        "Levansucrase of Bacillus subtilis. It cleaves sucrose and transfers the "
        "fructosyl unit onto a growing levan chain rather than onto water. "
        "Expressed in a Gram-negative host, that reaction is lethal in the "
        "presence of sucrose, which is what makes the gene a counter-selectable "
        "marker: sucrose sensitivity is dominant, so losing the gene is what is "
        "selected for.",
    ),
    Natural(
        "P62554", "AAA24899.1", "M12987.1", 306, 101,
        "Toxin of the ccdA/ccdB post-segregational killing system of the "
        "Escherichia coli F plasmid. CcdB traps DNA gyrase in its covalent "
        "cleavage complex, converting a transient double-strand break into a "
        "permanent one. The labile CcdA antitoxin neutralises it in cells that "
        "keep the plasmid; cells that lose it degrade CcdA first and die.",
        patent_flag=True,
        caveat=(
            "PATENT, and easy to miss because CcdB is an ordinary natural "
            "F-plasmid gene with a 1986 record: its commercial significance is "
            "as a negative-selection marker in a proprietary cloning system. "
            "Flagged, not adjudicated."
        ),
    ),
    Natural(
        "P08515", "AAB59203.1", "M14654.1", 657, 218,
        "The Sj26 antigen: a 26 kDa glutathione S-transferase, from the blood "
        "fluke Schistosoma japonicum. Conjugates reduced glutathione to "
        "electrophilic substrates; "
        "because it binds glutathione with high affinity and folds well in "
        "Escherichia coli, it became the archetypal affinity fusion partner.",
        caveat=(
            "The GST moiety encoded by pGEX vectors is NOT byte-identical to "
            "this record -- the vector adds linker and protease-site residues. A "
            "Tier-1 exact match against a real pGEX backbone will therefore fall "
            "short at the C-terminus, and that is correct behaviour, not a bug."
        ),
    ),
    Natural(
        "P0AEX9", "AAC77004.1", "U00096.3", 1191, 396,
        "Periplasmic maltose- and maltodextrin-binding protein of Escherichia "
        "coli: the substrate-recognition subunit of the MalEFGK2 ABC importer and "
        "the primary receptor for maltose chemotaxis. It closes around its ligand "
        "in a large hinge-bending motion, and folds so robustly that fusing it to "
        "a partner tends to keep the partner soluble.",
        caveat=(
            "BOUNDARY DECISION FOR THE CURATOR, not a sequence problem. The "
            "canonical 396 aa is the precursor and INCLUDES the 26-residue "
            "signal peptide. Fusion vectors use the MATURE protein. This row is "
            "written orf_atg_to_stop because that is what the chain actually "
            "derives; whether the shipped record should instead be "
            "orf_mature_peptide is a judgement, and making it silently here "
            "would be choosing a boundary rather than deriving one."
        ),
    ),
    Natural(
        "P42212", "AAA27721.1", "M62653.1", 717, 238,
        "Green fluorescent protein of the jellyfish Aequorea victoria. An "
        "eleven-stranded beta-barrel with a central helix whose "
        "Ser65-Tyr66-Gly67 tripeptide cyclises, dehydrates and is oxidised "
        "autocatalytically into a 4-(p-hydroxybenzylidene)imidazolinone "
        "chromophore -- needing molecular oxygen and nothing else, which is why "
        "the protein fluoresces in organisms that have never met a jellyfish.",
        patent_flag=True,
        caveat=(
            "WILD TYPE, not any engineered variant: no F64L, no S65T, no "
            "codon optimisation, and it will not exact-match EGFP. The trap is "
            "specific and it is inside this entry's own cross-references -- "
            "AAB18957.1 is titled \"synthetic construct green fluorescent "
            "protein mutant 3\", an ENGINEERED chromophore-region variant "
            "reachable from the wild-type accession. Which residues it changes "
            "is in the measured survey above; it is not restated here, because "
            "the hand-written version of that sentence had the two "
            "substitutions inverted and so asserted that wild-type avGFP "
            "carries Gly65. PATENT: fluorescent proteins are among the most "
            "heavily patented sequences in molecular biology; wild-type avGFP is "
            "old, but the flag stays and the database grants no patent licence."
        ),
    ),
    Natural(
        "Q9U6Y8", "AAF03369.1", "AF168419.2", 678, 225,
        "Red fluorescent protein drFP583 from a Discosoma coral. Same beta-barrel "
        "fold as GFP, but an additional dehydrogenation step extends the "
        "chromophore conjugation with an acylimine, red-shifting emission to "
        "around 583 nm. The wild-type protein is an obligate tetramer and matures "
        "slowly, which is what the later monomeric variants were made to fix.",
        patent_flag=True,
        caveat=(
            "WILD TYPE. The monomeric and rapidly-maturing derivatives are "
            "different sequences and are not in UniProt. PATENT: the most "
            "exposed item in this stage -- the anthozoan FP estate is "
            "commercially held and has been asserted. Flagged, not adjudicated; "
            "refer to counsel before shipping."
        ),
    ),
    # ------------------------------------------------------------------
    # Selection markers, added 2026-08-10. SOURCING.md Gap 6 (eukaryotic
    # selection markers) and Gap 7's yeast half were the two named holes that
    # this route can close without a NO_GO source, so these fourteen close them.
    #
    # ONE BOUNDARY DECISION COVERS ALL FOURTEEN and it is stated once here
    # rather than fourteen times below. Every row is the CDS the depositor
    # annotated, initiator codon through stop codon inclusive, and every row
    # therefore EXCLUDES the promoter. That is not a preference; it is what the
    # chain derives, and it is checkable from the row itself -- `len(nt) ==
    # 3*(len(aa)+1)` is asserted in every `notes` string, and each CDS begins at
    # a coordinate well inside its parent (`M25346.1:254..853` leaves 253 bp of
    # native pac upstream sequence outside the row). SOURCING.md:221 names the
    # two traps this raises and neither bites here:
    #
    #   * The bla signal-peptide trap. The UniProt feature tables of all
    #     fourteen were read: zero Signal, zero Propeptide, zero Transit
    #     peptide. Three (URA3, DHFR, rpsL) carry `Initiator methionine 1..1`,
    #     so their MATURE protein starts at residue 2; the rows keep residue 1,
    #     because that is a fact about the protein and not about the DNA, and
    #     `orf_atg_to_stop` is a claim about the reading frame.
    #   * The cassette-vs-ORF trap, which does bite. "PuroR" in a real vector is
    #     a promoter-ORF-polyA cassette; "URA3" in pRS416 is the gene with its
    #     own promoter and terminator; "TRP1" in YRp7 means TRP1-ARS1. An exact
    #     match against those files covers only part of the labelled region.
    #     Said again in the caveat of every row it applies to, because that is
    #     where a curator will be reading.
    Natural(
        "P13249", "AAA64928.1", "M25346.1", 600, 199,
        "Puromycin N-acetyltransferase of Streptomyces alboniger. Transfers an "
        "acetyl group from acetyl-CoA onto the free amino group of the tyrosinyl "
        "moiety of puromycin. Puromycin is an aminonucleoside that mimics the 3' "
        "end of an aminoacyl-tRNA and terminates the growing peptide chain; "
        "acetylation abolishes the mimicry. The standard dominant selection "
        "marker for mammalian cell culture, where killing is fast and selection "
        "is usually complete within a few days.",
        caveat=(
            "THE PINNED RECORD IS FLAGGED BY THE ARCHIVE AND THIS ROW MUST SAY SO. "
            "M25346.1's own DE line reads 'UNVERIFIED: Streptomyces alboniger "
            "puromycin N-acetyltransferase (pac) gene, complete cds', it carries "
            "the keyword UNVERIFIED_ORGANISM, and its comment says GenBank staff "
            "were unable to verify the source organism, the sequence and/or the "
            "annotation. P13249 has exactly ONE EMBL cross-reference, so there is "
            "no second INSDC record to fall back on. Nothing here rests on that "
            "record alone, which is why the row still stands. The organism claim "
            "rests on the record's own primary publication -- Lacalle, Pulido, "
            "Vara, Zalacain & Jimenez 1989, Gene 79:375-380, PMID 2676728, "
            "'Molecular analysis of the pac gene encoding a puromycin N-acetyl "
            "transferase from Streptomyces alboniger' -- and on the record's "
            "/strain=\"ATCC 12461\" and /culture_collection=\"ATCC:12461\". The "
            "extent claim is corroborated by that same paper, which reports 'a "
            "600-nt open reading frame, starting with an ATG codon': this row is "
            "600 nt, ATG through stop. The sequence claim rests on this stage's "
            "own forced translation, which a corrupted record would not survive. "
            "A signature that does not mention the archive's flag would be a "
            "signature saying a curator read the record and did not notice its "
            "first line. CASSETTE, NOT ORF, is what a vector map means by 'PuroR': "
            "a promoter, this ORF, and a poly(A) signal. A tier-1 nucleotide match "
            "will cover the ORF and stop, and that is correct behaviour. Mammalian "
            "constructs also frequently carry a codon-optimised pac, which these "
            "nucleotides cannot match at all -- SOURCING.md section 3 puts that "
            "case squarely on the translated tier, and the protein reference on "
            "this row is what serves it."
        ),
    ),
    Natural(
        "P0C2P0", "BAA12074.1", "D83710.1", 393, 130,
        # PHRASED AROUND THE TAINT GATE, on purpose, and the shape is the
        # evidence rather than an accident: the first draft opened "Blasticidin
        # S deaminase of Aspergillus terreus", and after stopword removal that
        # is a five-token run which occurs verbatim in snapgene.csv. Nothing was
        # copied -- it is the enzyme's name followed by its organism, in the only
        # order anyone writes them -- but SOURCING.md section 0.4 makes a shared
        # five-token run a hard fail with no appeal, and the project's own
        # precedent (PLF:3012) is to rewrite rather than argue with the
        # measurement. Leading with the reaction instead of the name breaks the
        # run and says more. Do not "tidy" this back to the obvious opening.
        "An enzyme of the fungus Aspergillus terreus that inactivates the "
        "nucleoside antibiotic blasticidin S. It hydrolyses the amino group off "
        "the drug's cytosine ring, and the deaminohydroxy product no longer "
        "blocks peptide-bond formation at the ribosome. Two entirely unrelated "
        "deaminases are sold under the name 'blasticidin resistance'; this is "
        "the fungal one.",
        caveat=(
            "TWO DIFFERENT PROTEINS SHARE THIS SELECTION. This is the fungal bsd. "
            "The bacterial bsr is the next row and is a different sequence with a "
            "different length; a construct carrying one will not match the other "
            "at the nucleotide level and the two must not be merged on the "
            "strength of the shared vernacular name 'BsdR'. THE EXTENT HAS A "
            "PRIMARY SOURCE, added 2026-08-11: Kimura, Kamakura, Tao, Kaneko & "
            "Yamaguchi 1994, Mol Gen Genet 242:121-129, PMID 8159161, which "
            "isolated this cDNA, reports that it contains 'an open reading frame "
            "of 393 bp, encoding a polypeptide of 130 amino acids' -- this row's "
            "extent to the base. The same paper is what names the other enzyme "
            "'bsr, the BS deaminase gene isolated from Bacillus cereus', and "
            "reports no homology and a large difference in codon usage between "
            "the two. CASSETTE, NOT ORF: see the pac row above."
        ),
    ),
    Natural(
        "P33967", "AAC60404.1", "S81409.1", 423, 140,
        # PHRASED AROUND THE TAINT GATE for the same reason as the bsd row above:
        # the obvious opening names the enzyme and then its organism, and after
        # stopword removal that is a five-token run of exactly the shape
        # SOURCING.md section 0.4 hard-fails. The organism is written in -- that
        # was the whole point of resolving it -- but it is written in a sentence
        # of its own, separated from the enzyme name. Do not "tidy" the two back
        # together.
        "Blasticidin S deaminase of the bsr type. Inactivates blasticidin S by the "
        "same hydrolytic deamination as the fungal bsd enzyme, from a different "
        "protein family. Its origin is bacterial: the gene was cloned out of "
        "pBSR8, a plasmid carried by the soil organism Bacillus cereus K55-S1. "
        "Widely used as a blasticidin selection marker in mammalian and insect "
        "cells.",
        caveat=(
            "ORGANISM CONFLICT, RESOLVED 2026-08-11 FROM THE PAPER THE RECORD "
            "CITES, AND THE RECORD'S /organism IS THE HOST. Until then this row "
            "named no organism, because UniProt P33967 gives Bacillus cereus while "
            "ENA S81409 carries /organism=\"Escherichia coli\" /strain=\"TK121\". "
            "Both are right about different things. Kobayashi, Kamakura, Tanaka, "
            "Yamaguchi & Endo 1991, Agric Biol Chem 55:3155-3157, PMID 1368770, is "
            "the article S81409 was created from -- it has no PubMed abstract, "
            "which is why this stayed open -- and it says in its own words that "
            "the authors isolated a blasticidin S resistant Bacillus cereus "
            "K55-S1, obtained from it the plasmid pBSR8 encoding the deaminase, "
            "and located the gene on a fragment of pBSR8 subcloned into pUC19 as "
            "pTK17; TK121 is the Escherichia coli transformant that pTK17 was "
            "grown in. Its own footnote reads 'Inactivation of Blasticidin S by "
            "Bacillus cereus. Part IV.'. Four corroborations, each re-derivable "
            "from this record: (1) Nawa, Tanaka, Kamakura, Yamaguchi & Endo 1998, "
            "Biol Pharm Bull 21:893-898, PMID 10607416, titled for a "
            "blasticidin-S-resistant Bacillus cereus, reports the promoter as "
            "91TTGATC and 113TAAAAT with the start point 7 bases downstream, and "
            "S81409 positions 91-96, 113-118 and 125 are TTGATC, TAAAAT and A "
            "exactly; (2) that same paper calls them sigmaA and sigmaB promoters, "
            "and sigmaB is a Bacillus general-stress factor that Escherichia coli "
            "does not have; (3) base composition -- this CDS is 37.4% GC with 25.5% "
            "GC at third positions and the 181 bases upstream are 24.3% GC, which "
            "is not an Escherichia coli gene; (4) the 1991 paper sequenced 'the "
            "NdeI-HincII fragment' and this 675 bp record begins CATATG and ends "
            "GTTGAC, so the record is that figure. Kimura, Kamakura, Tao, Kaneko & "
            "Yamaguchi 1994, Mol Gen Genet 242:121-129, PMID 8159161 -- the paper "
            "behind the bsd row above -- calls it 'bsr, the BS deaminase gene "
            "isolated from Bacillus cereus'. NOT RESOLVED BY THIS, and left for "
            "the curator: S81409 remains an S-prefixed record created by NLM staff "
            "from the article rather than a depositor submission, and P33967 has "
            "exactly one EMBL cross-reference, so there is no second INSDC record "
            "for this sequence to fall back on. A search for a Bacillus-attributed "
            "bsr found none. CASSETTE, NOT ORF: see the pac row above."
        ),
    ),
    Natural(
        "P00382", "CAA25445.1", "X00926.1", 474, 157,
        "Type I dihydrofolate reductase, the trimethoprim-insensitive enzyme "
        "carried on the Tn7 dfrA1 cassette. It reduces dihydrofolate to "
        "tetrahydrofolate exactly as the chromosomal enzyme does, but is bound by "
        "trimethoprim far more weakly, so one-carbon metabolism continues while "
        "the host enzyme is inhibited. Trimethoprim selection is useful where "
        "beta-lactam and aminoglycoside markers are already spent, and the "
        "cassette travels in integrons and in broad-host-range backbones.",
        caveat=(
            "NEAR-COLLISION WITH THE MOUSE DHFR ROW, MEASURED RATHER THAN "
            "ASSUMED. Both rows are dihydrofolate reductases and both are "
            "selection markers, with different drugs -- trimethoprim here, "
            "methotrexate there -- and they are unrelated proteins. The alias "
            "sets were compared and they do NOT in fact share a string: UniProt "
            "calls this one 'Dihydrofolate reductase type 1' and the mouse "
            "enzyme 'Dihydrofolate reductase', so a lookup resolves each to one "
            "record. The vernacular 'DHFR' on a map resolves to NEITHER, which "
            "is the real gap and is a naming decision for the curator, not a "
            "defect in the sequence."
        ),
    ),
    Natural(
        "P03962", "AAB64498.1", "U18530.1", 804, 267,
        "Orotidine 5'-phosphate decarboxylase of Saccharomyces cerevisiae, the "
        "final step of de novo pyrimidine biosynthesis. Complements a ura3 "
        "auxotroph, and is the standard yeast counter-selectable marker as well: "
        "cells that carry it convert 5-fluoroorotic acid into a toxic product, so "
        "growth on 5-FOA selects for having lost the gene.",
        caveat=(
            "MULTI-ALLELE TRAP on the scale of lacI, and the measured "
            "cross-reference survey above is the evidence rather than this "
            "sentence: four of this entry's EMBL cross-references carry the same "
            "residue-160 polymorphism, and three of those four sit in records "
            "whose own titles say 'cloning vector' -- so the deviating allele is "
            "what a construct is likely to carry. The pin is on the primary "
            "chromosome V record for that reason. CASSETTE, NOT ORF: "
            "'URA3' on a pRS map is the gene with its own promoter and "
            "terminator, so an exact match covers only the middle of it."
        ),
    ),
    Natural(
        "P04173", "CAA42366.2", "X59720.2", 1095, 364,
        "3-isopropylmalate dehydrogenase of Saccharomyces cerevisiae, the third "
        "enzyme of leucine biosynthesis. Complements a leu2 auxotroph, and is one "
        "of the four markers the pRS shuttle-vector series is built on.",
        caveat=(
            "THIS IS THE INTACT GENE, NOT leu2-d. The high-copy leu2-d allele "
            "used to force plasmid amplification differs from this one in how "
            "much upstream sequence it retains, i.e. in a promoter boundary, "
            "which SOURCING.md classes as a convention and not a fact. If leu2-d "
            "is wanted it is a separate row with its own evidence, and it is not "
            "in this database. AND IT HAS THE SAME MULTI-ALLELE TRAP AS URA3, "
            "which this row did not say until 2026-08-11. Every pRS vector "
            "deposited in INSDC carries a LEU2 that differs from this row: "
            "U03437 (pRS305), U03441 (pRS315) and U03449 (pRS415) differ at 4 "
            "nucleotides giving A69V and N300D, and U03445 (pRS405) and U03452 "
            "(pRS425) differ at 6 giving A69V, G78A, V195L and N300D. Those five "
            "records are ONE submitter, D. J. Stillman at the University of Utah, "
            "deposited on 10 and 11 November 1993 -- so a single deposit of a "
            "single vector series does not agree with itself about what LEU2 is, "
            "and no curator can pick between them from the records. A69V is not "
            "confined to vectors either: X03840 (CAA27459), a genomic record and "
            "one of P04173's own four cross-references, carries it too, so at "
            "least that part is allelic rather than an error. The pin stays on "
            "the chromosome III record for the same reason as URA3, and a "
            "construct built from a pRS backbone will mismatch this row at those "
            "positions for a reason that is nothing to do with the construct. "
            "CASSETTE, NOT ORF: see the URA3 row."
        ),
    ),
    Natural(
        "P06633", "CAA99417.1", "Z75110.1", 663, 220,
        "Imidazoleglycerol-phosphate dehydratase of Saccharomyces cerevisiae, the "
        "sixth step of histidine biosynthesis. Complements a his3 auxotroph. The "
        "enzyme is competitively inhibited by 3-aminotriazole, which is what makes "
        "HIS3 a tunable reporter in two-hybrid work: raising the inhibitor raises "
        "the expression threshold a colony has to clear before it grows.",
        caveat=(
            "THE CLASSIC HIS3 CLONE IS NOT THIS SEQUENCE, AND THE SURVEY ABOVE "
            "CANNOT SAY SO. Three of this entry's cross-references, CAA27003 "
            "among them, are 219 aa against this row's 220, and for unequal "
            "lengths the survey reports only that positions are not comparable "
            "-- correctly, since aligning from residue 1 would dress a frame "
            "offset up as point mutations. Aligned from both ends by hand: the "
            "two agree for 108 residues, agree again over the last 109, and "
            "differ only in the window at residue 109. That is a one-residue "
            "indel plus a substitution, not a start-codon convention. HIS3 in "
            "older vectors descends from that clone, so an exact match against "
            "one of them can fail for a reason that is nothing to do with this "
            "row. CASSETTE, NOT ORF: see the URA3 row."
        ),
    ),
    Natural(
        "P00912", "CAA24634.1", "V01341.1", 675, 224,
        "N-(5'-phosphoribosyl)anthranilate isomerase of Saccharomyces cerevisiae, "
        "the third step of tryptophan biosynthesis. Complements a trp1 auxotroph.",
        caveat=(
            "'TRP1' ON A MAP USUALLY MEANS TRP1-ARS1. In YRp7 and its descendants "
            "the label covers this gene TOGETHER WITH the adjacent autonomously "
            "replicating sequence, which is the part that makes the plasmid "
            "replicate at all. This row is the ORF; ARS1 is a separate element, "
            "its boundary is a convention, and it is not in this database."
        ),
    ),
    Natural(
        "P0DTH5", "CAA32315.1", "X14112.1", 1131, 376,
        "Thymidine kinase of herpes simplex virus type 1, gene UL23. Much less "
        "selective than the cellular enzyme, it phosphorylates nucleoside "
        "analogues such as ganciclovir and aciclovir, which are then extended to "
        "triphosphates that poison DNA synthesis. That promiscuity is the point: "
        "the gene is the classic negative-selection and suicide marker, killing "
        "the cells that carry it as soon as the prodrug is supplied.",
        patent_flag=True,
        caveat=(
            "THE DISPLAY NAME IS 'TK', AND THAT IS A LOOKUP GAP. This module's "
            "rule is that the name is UniProt's gene symbol, and here that "
            "symbol is two letters. UniProt lists no other gene name for the "
            "entry, so 'HSV-TK', 'HSVtk' and 'UL23' -- the spellings that "
            "actually appear on maps -- are NOT aliases of this row and will not "
            "resolve to it. Adding them would mean writing names ourselves into "
            "a column this stage sources entirely from UniProt under CC BY, "
            "which is a different decision and is the curator's to make. "
            "STRAIN CHOICE, RECORDED AS A DECISION AND NOT AS A CORRECTION, AND "
            "INVISIBLE TO THE SURVEY ABOVE, which only ever walks ONE entry's "
            "cross-references. UniProt carries a second reviewed 376 aa "
            "thymidine kinase for this virus under a different accession, and "
            "the two were compared here residue by residue: they differ at four "
            "positions and nowhere else. This pin is the strain 17 sequence, "
            "taken through a named-strain primary genome record. A construct "
            "built from the other entry will mismatch at four positions and is "
            "not corrupt. PATENT: "
            "HSV-TK/ganciclovir suicide systems carry a commercial estate. "
            "Flagged, not adjudicated -- no patent database was searched "
            "(SOURCING.md Risk 6)."
        ),
    ),
    Natural(
        "P00375", "AAH05796.1", "BC005796.1", 564, 187,
        "Mouse dihydrofolate reductase. Reduces dihydrofolate to tetrahydrofolate, "
        "the one-carbon donor for thymidylate and purine synthesis. Used as a "
        "selection marker in DHFR-negative CHO lines, and as an AMPLIFICATION "
        "marker: stepping methotrexate up selects for cells that have amplified "
        "the locus, and a linked transgene is amplified with it.",
        caveat=(
            "cDNA, NOT GENOMIC, AND THAT IS THE DELIBERATE CHOICE. The mRNA "
            "record pinned here and the six-exon genomic join both give this "
            "exact 187 aa, and their 564 nucleotides differ at exactly one "
            "position -- 396, C here and T there, a synonymous change. So the "
            "protein cannot tell them apart and a nucleotide match can. A vector "
            "carries the cDNA, which is what this row ships. See "
            "also the alias collision with the bacterial type I DHFR row above: "
            "same reaction, unrelated protein, different drug."
        ),
    ),
    Natural(
        "P0A9M5", "AAC73342.1", "U00096.3", 459, 152,
        "Xanthine-guanine phosphoribosyltransferase of Escherichia coli. Salvages "
        "guanine, xanthine and hypoxanthine into their nucleotides. Mammalian "
        "cells cannot use xanthine this way, so in medium containing mycophenolic "
        "acid, which blocks de novo GMP synthesis, together with xanthine, only "
        "cells expressing this enzyme make GMP and survive.",
        caveat=(
            "TWO DIFFERENT GENES ARE WRITTEN 'gpt'. This is the bacterial "
            "xanthine-guanine enzyme used as a dominant marker (often written "
            "Ecogpt in mammalian work), not the mammalian hypoxanthine-guanine "
            "enzyme HPRT. CASSETTE, NOT ORF: see the pac row."
        ),
    ),
    Natural(
        "P16426", "CAA35093.1", "X17220.1", 552, 183,
        "Phosphinothricin N-acetyltransferase from the bialaphos biosynthesis "
        "cluster of Streptomyces hygroscopicus. Acetylates the free amino group of "
        "phosphinothricin, the glutamine-synthetase inhibitor released from "
        "bialaphos and sold as the herbicide glufosinate. The standard "
        "herbicide-resistance selection marker for plant transformation.",
        patent_flag=True,
        caveat=(
            "THE BOUNDARY HERE IS ONE BASE, AND IT IS BASE 1. Two records hold "
            "this gene: the native-locus record begins GTG, and the record pinned "
            "here -- the cassette a plant-transformation paper published as a "
            "selectable marker -- begins ATG. Both are 552 nt, both give the "
            "identical 183 aa, and they differ at exactly one nucleotide, "
            "position 1. This row ships the ATG form, because that is what plant "
            "vectors were built from; a curator who wants the native gene pins "
            "the other record and must expect a position-1 mismatch against every "
            "construct. PATENT: herbicide-tolerance traits are heavily patented. "
            "Flagged, not adjudicated."
        ),
    ),
    Natural(
        "Q57146", "CAA46314.1", "X65195.2", 552, 183,
        "Phosphinothricin N-acetyltransferase of Streptomyces viridochromogenes. "
        "The same reaction and the same glufosinate selection as the bar gene of "
        "the row above, from a different producer strain. The two are used "
        "interchangeably in plant transformation and are distinct sequences, so a "
        "construct carrying one does not match the other at the nucleotide level.",
        patent_flag=True,
        caveat=(
            "'bar' AND 'pat' ARE USED INTERCHANGEABLY IN THE LITERATURE AND ARE "
            "TWO RECORDS HERE, on purpose: they are two genes. Whether a given "
            "map's 'BlpR' or 'PPT-AT' label means this one or the previous one "
            "cannot be settled from the label and must be settled from the "
            "sequence. PATENT: as for bar. Flagged, not adjudicated."
        ),
    ),
    Natural(
        "P0A7S3", "AAC76367.1", "U00096.3", 375, 124,
        "30S ribosomal protein S12 of Escherichia coli, part of the decoding "
        "centre of the small subunit. The wild-type allele is DOMINANT SENSITIVE "
        "to streptomycin: a streptomycin-resistant host carries a mutant rpsL, and "
        "supplying the wild-type protein in trans restores sensitivity. That "
        "inversion is what makes the gene a counter-selectable marker -- an "
        "rpsL-neo cassette is selected onto a target with kanamycin and selected "
        "off it again with streptomycin.",
        caveat=(
            "NOT A RESISTANCE GENE, AND THE ALIAS SAYS THE OPPOSITE. UniProt "
            "lists 'strA' as a synonym of rpsL, from the early Escherichia coli "
            "genetics in which streptomycin resistance mapped to this locus. "
            "'StrA' is also the NAME of PLF:0023, the plasmid aminoglycoside "
            "phosphotransferase APH(3'')-Ib, which confers streptomycin "
            "RESISTANCE where this gene confers SENSITIVITY. The collision is "
            "real, both usages are real, and features/README.md states it; a "
            "caller resolving the alias to a single record will get one of two "
            "genes with opposite phenotypes."
        ),
    ),
)


# The documented trap, run live on every build so the gate is exercised rather
# than assumed. Both halves matter: without the positive the test could pass by
# failing everything, without the negative it could pass by accepting anything.
CONTROL_ACC = "P62593"
CONTROL_MUST_MATCH = "AAB59737.1"
CONTROL_MUST_NOT_MATCH = "CAA45828.1"


# --------------------------------------------------------------------------
# UniProt: parse named fields, never "everything the API returns"


UNIPROT_FIELDS = (
    "primaryAccession",
    "uniProtkbId",
    "entryType",
    "annotationScore",
    "proteinDescription",
    "genes",
    "organism",
    "sequence",
    "uniProtKBCrossReferences",
)

REVIEWED = "UniProtKB reviewed (Swiss-Prot)"


def is_reviewed(entry: dict) -> bool:
    """Swiss-Prot or TrEMBL — by equality, never by substring.

    `"reviewed" in entryType` is True for TrEMBL, because "unreviewed" contains
    "reviewed". That mistake reports every unreviewed entry as curated, which is
    exactly backwards for a quality gate. It was made while probing this stage
    and Q9T221 was the entry that exposed it.
    """
    return entry.get("entryType") == REVIEWED


def pick_uniprot(raw: dict) -> dict:
    """Field-level allow-list, enforced at ingest.

    These entries carry DrugBank, KEGG, ChEMBL and DrugCentral cross-references
    that UniProt has no power to relicense — KEGG in particular states it is not
    a public database and requires a commercial licence for non-academic use.
    A permissive parser that kept `uniProtKBCrossReferences` wholesale would
    carry them into our provenance table under UniProt's CC BY, which is a
    licence claim we cannot make. So: named fields in, everything else dropped,
    and the EMBL restriction re-asserted below rather than left as an intention.
    """
    entry = {k: raw[k] for k in UNIPROT_FIELDS if k in raw}

    embl = []
    for x in raw.get("uniProtKBCrossReferences", []):
        if x.get("database") != "EMBL":
            continue
        for p in x.get("properties", []):
            if p.get("key") == "ProteinId" and p.get("value"):
                embl.append({"record": x.get("id", ""), "protein_id": p["value"]})
    entry["uniProtKBCrossReferences"] = embl

    # The allow-list has to be checkable, or it is a comment. Anything that is
    # not an EMBL ProteinId must be gone by now.
    stray = [e for e in embl if set(e) != {"record", "protein_id"}]
    if stray:
        raise SystemExit(f"cross-reference allow-list leaked non-EMBL fields: {stray}")
    return entry


GENERIC_ALIAS = re.compile(r"^(protein|gene|product)\s+\S{1,2}$", re.I)


def useful_alias(a: str) -> bool:
    """Is this string usable as a lookup key for one record?

    UniProt's naming fields are faithful to the literature and are transcribed
    verbatim; the question here is narrower and is ours. SOURCING.md §3 makes the
    alias table the mechanism that resolves a spelling on a map to a record, and
    two kinds of genuine UniProt string cannot do that job:

      * A one- or two-character alias. P62554 (ccdB) legitimately lists the gene
        name `G`, which as a query key matches everything and identifies nothing.
      * `Protein <one letter>`. The same entry lists `Protein G` -- and Protein G
        is the streptococcal IgG-binding protein used in fusion constructs, so
        an alias lookup for a common cloning part would resolve to a gyrase
        poison. Faithful transcription, wrong answer.

    Dropped from the alias index only. Nothing here edits what UniProt says: the
    full recommended name is still carried, and the entry is one fetch away.
    """
    return len(a) > 2 and not GENERIC_ALIAS.match(a)


def uniprot_names(entry: dict) -> tuple[str, list[str]]:
    """Display name and aliases, both from UniProt — so both are CC BY 4.0.

    Rule, stated so it can be argued with: the gene symbol is the name when the
    entry has one, because that is what a biologist writes on a map; otherwise
    the recommended full name, and for an unreviewed entry the submission name,
    which is all TrEMBL provides. Everything else UniProt offers becomes an
    alias, plus the accession itself, which is the one alias guaranteed to be
    unambiguous.
    """
    pd = entry.get("proteinDescription", {})
    rec = pd.get("recommendedName", {})
    full = rec.get("fullName", {}).get("value")
    if not full:
        subs = pd.get("submissionNames", [])
        full = subs[0]["fullName"]["value"] if subs else ""

    genes, syn = [], []
    for g in entry.get("genes", []):
        if "geneName" in g:
            genes.append(g["geneName"]["value"])
        syn += [s["value"] for s in g.get("synonyms", [])]

    alts = [s["value"] for s in rec.get("shortNames", [])]
    for an in pd.get("alternativeNames", []):
        if "fullName" in an:
            alts.append(an["fullName"]["value"])
        alts += [s["value"] for s in an.get("shortNames", [])]

    name = genes[0] if genes else full
    pool = genes[1:] + syn + ([full] if full else []) + alts
    pool.append(entry.get("primaryAccession", ""))

    seen, aliases = {name.lower()}, []
    for a in pool:
        a = a.strip()
        if a and a.lower() not in seen and useful_alias(a):
            seen.add(a.lower())
            aliases.append(a)

    # `aliases` is written as a pipe-joined cell, so a pipe inside a name would
    # split one alias into two plausible-looking ones with no error anywhere.
    # UniProt names are free text and we do not control them, so this is
    # checked rather than assumed.
    for a in [name, *aliases]:
        if "|" in a or "\t" in a:
            raise SystemExit(f"UniProt name {a!r} contains a delimiter; refusing to write")
    return name, aliases


# --------------------------------------------------------------------------
# ENA


def ena_cds_nt(protein_id: str, refresh: bool) -> tuple[str, dict]:
    """The CDS nucleotides for one EMBL ProteinId, plus the cache metadata."""
    name = f"ena_cds_{protein_id}.fa"
    text = fetch(ENA_FASTA.format(protein_id), name, refresh).decode("utf8", "replace")
    records = list(parse_fasta(text))
    if len(records) != 1:
        raise ValueError(f"ENA returned {len(records)} FASTA records, expected 1")
    header, seq = records[0]

    # The description on this line is NOT a name source, and this is not a
    # theoretical worry: AAA96581.1 (lambda cI) comes back as
    # "Escherichia phage Lambda rexb (exclusion;144)", inherited from a /product
    # qualifier that three different CDSs in J02459.1 all share. Header kept for
    # the record; names come from UniProt.
    return seq.upper().replace("U", "T"), {"header": header, "cache": cached_meta(name)}


def describe_mismatch(aa: str, canonical: str) -> str:
    """Say how two proteins differ without inventing substitutions.

    Written after a diagnostic lied. Zipping sequences of unequal length aligns
    them from residue 1, so a 363 aa entry that is the same protein read from an
    upstream alternative start reported "333 substitutions at [2, 3, 5, 6, 7]" —
    a frame offset dressed up as point mutations. That number would have sent a
    curator looking for a sequencing disaster instead of a start-codon
    annotation. Positions are only meaningful when the lengths agree.

    NOTATION, stated once and used everywhere in this module: a substitution is
    written CANONICAL-residue, position, VARIANT-residue. `S65G` means the
    pinned UniProt canonical carries Ser at 65 and the record being described
    carries Gly. That is the universal convention and this function has always
    followed it; two hand-written caveats did not, and asserted the inverse.
    `S65G+S72A` was written `G65S+A72S`, which claims wild-type avGFP carries
    Gly65 -- the single residue the whole GFP literature turns on. Hand-written
    arithmetic about fetched records is generated by `survey_xrefs` now.

    The span is computed, not narrated. "37 residues in one contiguous run from
    position 388" was written by hand about a divergence that is neither
    contiguous nor confined to that run.
    """
    if len(aa) != len(canonical):
        return (
            f"length differs by {len(aa) - len(canonical):+d} aa, so residue positions "
            f"are not comparable"
        )
    diffs = [i for i, (x, y) in enumerate(zip(aa, canonical), 1) if x != y]
    if not diffs:
        return "identical"
    shown = ", ".join(f"{canonical[i - 1]}{i}{aa[i - 1]}" for i in diffs[:6])
    out = f"{len(diffs)} substitution(s): {shown}"
    if len(diffs) > 6:
        out += ", ..."
    if len(diffs) > 1:
        runs, start, prev = [], diffs[0], diffs[0]
        for d in diffs[1:]:
            if d != prev + 1:
                runs.append((start, prev))
                start = d
            prev = d
        runs.append((start, prev))
        out += (
            f" [spanning positions {diffs[0]}-{diffs[-1]} in "
            f"{len(runs)} contiguous run(s)]"
        )
    return out


def survey_xrefs(entry: dict, canonical: str, pinned_id: str, refresh: bool) -> str:
    """Translate EVERY EMBL cross-reference of this entry and say what they are.

    The point of pinning one ProteinId is that the others are different
    sequences, and the evidence for that used to live in a hand-written `caveat`
    string. Three of those strings were checked against the records and two were
    wrong: the GFP caveat inverted both substitutions in its engineered-variant
    warning, and the T7 caveat called a divergence "one contiguous run from
    position 388" when it is two clusters plus two isolated changes eighty
    residues further on. Neither error was detectable from inside this file.

    So the arithmetic is done here, from the records, on every build. The
    editorial judgement -- which trap matters, and why a curator should care --
    stays in `caveat`, where a human wrote it and a human can check it.

    A `protein_id` of `-` is UniProt's placeholder for a cross-reference with no
    translation accession. Requesting `.../fasta/-` returns HTTP 500 and
    `build.fetch` turns a failed fetch into SystemExit, so one placeholder in one
    list would take down the whole build. Filtered at source, by shape.
    """
    exact, other, unusable = [], [], []
    for x in entry.get("uniProtKBCrossReferences", []):
        pid = x.get("protein_id", "") or ""
        if not re.fullmatch(r"[A-Z]{3}\d{5,7}\.\d+", pid):
            unusable.append(pid or "(none)")
            continue
        try:
            nt, _ = ena_cds_nt(pid, refresh)
        except Exception as e:  # noqa: BLE001 — an unfetchable xref is reported, not fatal
            other.append(f"{pid}: not fetched ({e.__class__.__name__})")
            continue
        aa = translate_cds(nt).rstrip("*")
        (exact if aa == canonical else other).append(
            pid if aa == canonical else f"{pid}: {describe_mismatch(aa, canonical)}"
        )
    total = len(exact) + len(other) + len(unusable)
    parts = [
        f"MEASURED over all {total} EMBL cross-reference(s) of this entry: "
        f"{len(exact)} translate to the canonical exactly "
        f"({', '.join(exact) or 'none'}); the pinned one is {pinned_id}."
    ]
    if other:
        parts.append("The others differ -- " + "; ".join(other) + ".")
    if unusable:
        parts.append(
            f"{len(unusable)} cross-reference(s) carry no usable translation accession "
            f"({', '.join(sorted(set(unusable)))}) and were never fetched."
        )
    parts.append("Substitutions are written canonical-residue, position, variant-residue.")
    return " ".join(parts)


FT_KEY = re.compile(r"^FT {3}(\S+) {2,}(.*)$")
FT_CONT = re.compile(r"^FT {19}(.*)$")


def ena_cds_record(protein_id: str, refresh: bool) -> tuple[dict, dict]:
    """Parent accession, INSDC location and qualifiers for one CDS.

    The ENA `embl` view of a ProteinId is where the boundary actually lives:
    the `PA` line gives the *versioned* parent accession and the CDS feature
    carries the depositor's own location expression in parent coordinates —
    `complement(U00096.3:366428..367510)`, or for the 2-micron FLP
    `join(J01347.1:5570..6318,J01347.1:1..523)`. Neither the FASTA header nor
    the UniProt xref contains any of that: the xref gives `U00096`, unversioned,
    with no coordinates at all.
    """
    name = f"ena_cds_{protein_id}.embl"
    text = fetch(ENA_EMBL.format(protein_id), name, refresh).decode("utf8", "replace")
    meta = cached_meta(name)

    parents = [ln[5:].strip() for ln in text.splitlines() if ln.startswith("PA   ")]
    declared_bp = None
    for ln in text.splitlines():
        if ln.startswith("ID   "):
            m = re.search(r";\s*(\d+)\s*BP\.", ln)
            if m:
                declared_bp = int(m.group(1))
            break

    # Walk the feature table collecting whole features. A location may wrap onto
    # continuation lines; the first line beginning "/" ends it. Getting this
    # wrong swallows the /translation block into the location string, which then
    # fails to parse and reads like an ENA outage rather than a parser bug.
    features, cur = [], None
    for ln in text.splitlines():
        m = FT_KEY.match(ln)
        if m:
            if cur:
                features.append(cur)
            cur = {"key": m.group(1), "loc": m.group(2).strip(), "quals": {}, "_done": False}
            continue
        c = FT_CONT.match(ln)
        if c and cur is not None:
            body = c.group(1)
            if body.startswith("/"):
                cur["_done"] = True
                q = re.match(r'^/(\w+)=?"?([^"]*)"?', body)
                if q:
                    cur["quals"].setdefault(q.group(1), q.group(2))
            elif not cur["_done"]:
                cur["loc"] += body.strip()
            continue
        if cur and not ln.startswith("FT"):
            features.append(cur)
            cur = None
    if cur:
        features.append(cur)

    # Select the CDS by /protein_id rather than by "the first CDS". A record with
    # one CDS makes the two identical and the difference invisible; a record with
    # two makes the lazy version wrong with no symptom.
    cds = [f for f in features if f["key"] == "CDS" and f["quals"].get("protein_id") == protein_id]
    if len(cds) != 1:
        raise ValueError(
            f"expected exactly 1 CDS with /protein_id={protein_id}, found {len(cds)}"
        )
    f = cds[0]
    return (
        {
            "parent": parents[0] if parents else "",
            "parents": parents,
            "location": f["loc"],
            "codon_start": f["quals"].get("codon_start", "1"),
            "transl_table": f["quals"].get("transl_table", ""),
            "declared_bp": declared_bp,
        },
        meta,
    )


# --------------------------------------------------------------------------
# The control


def run_control(refresh: bool) -> list[str]:
    """Re-run the documented TEM-1 trap on live data, both directions.

    SOURCING.md §2 step 4 names P62593 as a merged multi-allele entry whose EMBL
    cross-references point at different alleles: AAB59737.1 matches the
    canonical, CAA45828.1 differs at three positions. Both halves are asserted
    here. If the negative half ever starts matching, the exact-match gate has
    stopped discriminating and every "verified" line this module prints is
    worthless — so it raises rather than warns.
    """
    entry = pick_uniprot(json.loads(fetch(
        UNIPROT_JSON.format(CONTROL_ACC), f"uniprot_{CONTROL_ACC}.json", refresh
    )))
    canonical = entry["sequence"]["value"].upper()
    out = []
    for pid, must_match in ((CONTROL_MUST_MATCH, True), (CONTROL_MUST_NOT_MATCH, False)):
        nt, _ = ena_cds_nt(pid, refresh)
        aa = translate_cds(nt).rstrip("*")
        matched = aa == canonical
        if matched != must_match:
            raise SystemExit(
                f"CONTROL FAILED: {CONTROL_ACC} vs {pid} "
                f"{'should' if must_match else 'should NOT'} translate to the canonical, "
                f"but {'it does' if matched else 'it does not'}. The exact-match gate is "
                f"not doing what this module claims it does; refusing to emit rows."
            )
        out.append(
            f"  CTRL {pid:12s} {'matches exactly' if matched else describe_mismatch(aa, canonical)}"
            f" — as required"
        )
    return out


# --------------------------------------------------------------------------
# Stage 2


def build(refresh: bool) -> tuple[list, list]:
    """Return (rows, report), the shape `stage_amrfinder` returns.

    Per-field provenance rides on each `Row.provenance`, exactly as it does in
    Stage 1, because that is what `build.main()` reads when it writes
    provenance.tsv. Returning a flat provenance list as the second element
    instead would type-check, print fine, and write an empty provenance file.
    """
    report, rows = [], []
    report += run_control(refresh)

    for i, it in enumerate(ITEMS):
        # `ordinal` is the contract build.allocate() actually reads; `rid` is the
        # same number spelled out, kept so this module's standalone report and
        # its provenance tuples name the id the row will really get. If an item
        # drops out, the ordinal of every later item is unchanged and the gap
        # stays open for it — which is the whole reason the id comes from the
        # declaration and not from the output.
        ordinal = i + 1
        rid = f"PLF:{ID_BASE + i:04d}"
        try:
            raw = json.loads(fetch(
                UNIPROT_JSON.format(it.uniprot_acc),
                f"uniprot_{it.uniprot_acc}.json",
                refresh,
            ))
        except Exception as e:  # noqa: BLE001 — one bad item must not kill the stage
            report.append(f"  DROP {it.uniprot_acc:8s} UniProt fetch failed: {e}")
            continue

        entry = pick_uniprot(raw)
        up_meta = cached_meta(f"uniprot_{it.uniprot_acc}.json")
        canonical = entry.get("sequence", {}).get("value", "").upper()
        if not canonical:
            report.append(f"  DROP {it.uniprot_acc:8s} entry carries no canonical sequence")
            continue

        # The pin must be one of the entry's own cross-references. If UniProt
        # drops it, our ProteinId has become an accession we assert rather than
        # one we followed — which is the failure this whole file is written
        # against — so the item goes, it does not get fetched anyway.
        xrefs = entry["uniProtKBCrossReferences"]
        pinned = [x for x in xrefs if x["protein_id"] == it.protein_id]
        if not pinned:
            report.append(
                f"  DROP {it.uniprot_acc:8s} pinned ProteinId {it.protein_id} is no longer "
                f"an EMBL cross-reference of this entry ({len(xrefs)} present)"
            )
            continue

        try:
            nt, nt_info = ena_cds_nt(it.protein_id, refresh)
            cds, cds_meta = ena_cds_record(it.protein_id, refresh)
        except Exception as e:  # noqa: BLE001
            report.append(f"  DROP {it.uniprot_acc:8s} ENA {it.protein_id}: {e}")
            continue

        if cds["codon_start"] != "1":
            report.append(
                f"  DROP {it.uniprot_acc:8s} {it.protein_id} has /codon_start="
                f"{cds['codon_start']}; this chain reads frame 1 only"
            )
            continue
        if cds["declared_bp"] is not None and cds["declared_bp"] != len(nt):
            report.append(
                f"  DROP {it.uniprot_acc:8s} {it.protein_id}: FASTA is {len(nt)} nt but the "
                f"flat file declares {cds['declared_bp']} BP — two views of one record disagree"
            )
            continue
        if cds["parent"] != it.parent:
            report.append(
                f"  DROP {it.uniprot_acc:8s} {it.protein_id} now cites parent "
                f"{cds['parent'] or '(none)'}, expected {it.parent}"
            )
            continue

        # `cds_matches_protein`, not `translate_cds(...) == canonical`. The
        # latter looks like an exact-match test and is not one: it rewrites
        # residue 1 to Met whenever the first codon is an alternative initiator,
        # so a CDS disagreeing with its protein at position 1 passes silently.
        # Two rows here start GTG (lacI, int) and neither said so. `how` is the
        # sentence describing what was actually accepted, and it goes into the
        # row instead of a bare "translation identical".
        aa = translate_cds(nt).rstrip("*")
        ok, how = cds_matches_protein(nt, canonical)
        if not ok:
            # Never patched. A near-miss is the dangerous case: it is a real
            # protein of the right length under the right name, and it is a
            # different allele.
            report.append(
                f"  DROP {it.uniprot_acc:8s} {it.protein_id} translates to {len(aa)} aa, "
                f"canonical is {len(canonical)} aa: {describe_mismatch(aa, canonical)} "
                f"— dropped, not corrected"
            )
            continue

        # Belt and braces against the allow-list drifting away from the source:
        # these are the numbers a human wrote down, and they must agree with the
        # bytes just fetched.
        if (len(nt), len(aa)) != (it.nt_len, it.aa_len):
            report.append(
                f"  DROP {it.uniprot_acc:8s} {it.protein_id} is {len(nt)} nt / {len(aa)} aa, "
                f"allow-list says {it.nt_len} / {it.aa_len}"
            )
            continue

        name, aliases = uniprot_names(entry)
        organism = entry.get("organism", {})

        notes = (
            f"UniProt {it.uniprot_acc} ({entry.get('uniProtkbId', '?')}, "
            f"{'Swiss-Prot' if is_reviewed(entry) else 'TrEMBL'}, "
            f"annotation_score {entry.get('annotationScore', '?')}) -> EMBL "
            f"cross-reference {it.protein_id} -> ENA CDS {len(nt)} nt -> {len(aa)} aa, "
            f"compared to the UniProt canonical residue by residue: {how} "
            f"(NCBI table {cds['transl_table'] or '1 (unstated)'}). "
            f"{len(nt)} == 3*({len(aa)}+1), so the CDS carries its stop codon. "
            f"The entry has {len(xrefs)} EMBL cross-reference(s); this one was pinned "
            f"after verification and the other {len(xrefs) - 1} were not used. "
            f"Organism: {organism.get('scientificName', '?')} "
            f"(taxon {organism.get('taxonId', '?')}). "
            + survey_xrefs(entry, canonical, it.protein_id, refresh)
        )
        if it.caveat:
            notes += " CURATOR: " + it.caveat

        u_src = (
            "uniprot", it.uniprot_acc, "CC-BY-4.0",
            UNIPROT_JSON.format(it.uniprot_acc),
            up_meta.get("retrieved", TODAY), up_meta.get("sha256", ""),
        )
        n_src = (
            "ena", it.protein_id, "INSDC-free", ENA_FASTA.format(it.protein_id),
            nt_info["cache"].get("retrieved", TODAY), nt_info["cache"].get("sha256", ""),
        )
        b_src = (
            "ena", cds["parent"], "INSDC-free", ENA_EMBL.format(it.protein_id),
            cds_meta.get("retrieved", TODAY), cds_meta.get("sha256", ""),
        )

        rows.append(
            Row(
                id=rid,
                ordinal=ordinal,
                name=name,
                aliases=aliases,
                cls="cds",
                genbank_key="CDS",
                reference_nt=nt,
                reference_aa=aa,
                boundary_rule="orf_atg_to_stop",
                # The depositor's own INSDC location expression, verbatim. It
                # already carries the accession, it survives complement() and it
                # survives a join that wraps a circular origin — none of which
                # the acc:start-end:+ form can express.
                boundary_evidence=cds["location"],
                description=it.description,
                notes=notes,
                patent_flag="1" if it.patent_flag else "0",
                # Three licences in one row, which is the entire reason
                # provenance is keyed on (record, field). reference_aa is
                # UniProt's canonical — it is also reproducible by translating
                # the ENA nucleotides, which is exactly what the check above
                # did, but the string we store is theirs and is credited as
                # theirs.
                provenance=[
                    (rid, "name", *u_src),
                    (rid, "aliases", *u_src),
                    (rid, "reference_aa", *u_src),
                    (rid, "reference_nt", *n_src),
                    (rid, "boundary_evidence", *b_src),
                    (rid, "boundary_rule", "polylinker", "-", "own-work", "-", TODAY, ""),
                    (rid, "description", "polylinker", "-", "own-work", "-", TODAY, ""),
                ],
            )
        )
        report.append(
            f"  OK   {rid} {it.uniprot_acc:8s} {it.protein_id:12s} -> {name:10s} "
            f"{len(nt):5d} nt / {len(aa):4d} aa  {cds['location']}"
            + ("  [patent_flag]" if it.patent_flag else "")
        )

    return rows, report


def main() -> int:
    ap = argparse.ArgumentParser(description="Stage 2 -- UniProt -> ENA, standalone")
    ap.add_argument("--refresh", action="store_true", help="re-fetch every source")
    args = ap.parse_args()

    print("Stage 2 -- UniProt -> ENA CDS (standalone run)")
    print(f"  date {TODAY}   cache {CACHE}")
    print(f"  {len(ITEMS)} allow-listed items\n")

    rows, report = build(args.refresh)
    print("\n".join(report))

    print(f"\n{len(rows)}/{len(ITEMS)} items produced a row.\n")
    for r in rows:
        print(f"{r.id}  {r.name}  [{r.cls}/{r.genbank_key}]  patent_flag={r.patent_flag}")
        print(f"    aliases          {'|'.join(r.aliases)}")
        print(f"    boundary         {r.boundary_rule}  {r.boundary_evidence}")
        print(f"    reference_nt     {len(r.reference_nt)} nt  {r.reference_nt[:45]}…")
        print(f"    reference_aa     {len(r.reference_aa)} aa  {r.reference_aa[:45]}…")
        print(f"    description      {esc(r.description)[:150]}…")
        print(f"    notes            {esc(r.notes)[:150]}…")
        for p in r.provenance:
            print(f"      prov  {p[1]:18s} {p[2]:10s} {p[4]:11s} {p[3]:12s} {p[7][:16]}")
        print()

    lic: dict[str, int] = {}
    for r in rows:
        for p in r.provenance:
            lic[p[4]] = lic.get(p[4], 0) + 1
    print(f"provenance rows {sum(lic.values())}: " + ", ".join(f"{k}={v}" for k, v in sorted(lic.items())))
    print(f"patent-flagged  {sum(1 for r in rows if r.patent_flag == '1')}")
    print("\nEvery row is 'proposed' with no curator; build.main() writes those columns.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
