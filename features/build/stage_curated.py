#!/usr/bin/env python3
"""Stage 5 — hand-curated designed parts: epitope tags, protease sites, 2A, linkers.

SOURCING.md Gap 1. These are *Class C* features: "synthetic/designed (boundaries
are stipulated by a paper)". There is no catalogue to harvest — UniProt returns
nothing for FLAG or His6 — so the allow-list below is written by hand, one paper
per row, and that citation *is* the provenance.

Why most of this table does not become a row yet
-----------------------------------------------

The loader is explicit on two points, and together they make a protein-only
Class C row inexpressible:

    crates/pl-features/src/lib.rs:556   reference_nt is empty            -> reject
    crates/pl-features/src/lib.rs:573   class {} carries a protein
                                        reference; only cds may          -> reject

A tag is a peptide. FLAG is `DYKDDDDK`, and SOURCING.md §3 says so itself:
"Why translated matching is the *only* sane option for tags [...] At the
nucleotide level it has dozens of synonymous encodings". But `synthetic_part`
may not carry `reference_aa`, and every row must carry `reference_nt`. So the
only way a tag becomes a loadable row today is with nucleotides.

There are exactly two ways to obtain those nucleotides, and only one of them is
allowed here:

  * **Back-translate the peptide.** Forbidden outright. Choosing codons is
    writing a sequence that no record contains — the precise failure this whole
    build exists to prevent. It would also be useless: it would match only the
    vectors that happened to make the same codon choices.

  * **Take the codons out of the natural gene the peptide came from**, verified
    by translation. Legitimate, and that is what `build()` does — but it only
    exists for the tags that *have* a natural parent. FLAG, His6, Strep-tag,
    SBP, AviTag, ALFA and the GS/EAAAK linkers were designed or selected; there
    is no gene to read them out of. Those rows are declared here, are reported
    every run with the reason, and emit nothing.

So this stage yields eight rows out of twenty-eight, and the shortfall is a
schema question for the curator, not a sourcing failure. It is stated in
features/README.md rather than left in this docstring.

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
produced this allow-list (PDB polymer entities and UniProt canonicals; see
`witness` on each part). This file does not take that on faith. For every part
it tries to build, the peptide must be found — exactly once — inside a UniProt
canonical sequence fetched at build time, and the codons sliced out of the ENA
CDS must translate back to it. A single wrong residue in this table drops the
row; it cannot ship. That is what makes a hand-written sequence table safe to
have at all.

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
from dataclasses import dataclass
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
    "" if it was designed and has no gene. Only parts with one can be built."""
    hold: str = "designed or selected peptide; there is no gene to take codons from"
    """Why this part emits no row, in one line, printed every run. Three
    different situations hide behind an empty `parent_uniprot` and they are not
    interchangeable: a peptide that never had a gene, a peptide whose gene exists
    but whose accession nobody established from a fetched record, and an
    engineered variant of a natural junction. Only the middle one is one lookup
    away from buildable, and a generic message would bury that."""
    patent_flag: str = "0"
    caveat: str = ""
    """Something the curator must decide or must not assume. Appended to
    `notes`, where it will be read."""


# ORDER IS IDENTITY. `ordinal` is this list's index, and a PLF id is a permanent
# name. Append; never insert, never reorder, never delete a line — retire a part
# by leaving it here and giving it a caveat.
PARTS: tuple[Part, ...] = (
    Part(
        name="FLAG tag",
        aliases=("FLAG", "FLAG epitope", "DYKDDDDK tag"),
        aa="DYKDDDDK",
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
               "annotator's output. The matcher should extend the histidine run greedily "
               "and report the length it observed.",
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
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="DOI:10.1016/s0021-9258(18)70031-8 (Richards & Vithayathil 1959, "
                          "J Biol Chem 234:1459-1465) - the subtilisin-generated S-peptide, "
                          "residues 1-15 of mature bovine RNase A",
        citation="Richards FM, Vithayathil PJ (1959) The preparation of "
                 "subtilisin-modified ribonuclease and the separation of the peptide and "
                 "protein components. J Biol Chem 234:1459-1465.",
        description="The first fifteen residues of bovine pancreatic ribonuclease A, "
                    "released by a single subtilisin cut. Neither the peptide nor the "
                    "remaining S-protein is active alone, but they reassociate with "
                    "nanomolar affinity and restore activity - so a fusion carrying this "
                    "peptide can be both captured and assayed. One of the oldest "
                    "protein-fragment complementation systems in biochemistry.",
        witness="PDB 1A2W entity 1, 'RIBONUCLEASE A', whose 124-residue chain begins "
                "KETAAAKFERQHMDSSTSAASSSNYCNQMM - i.e. the boundary is read off residues "
                "1-15 of a deposited chain rather than chosen.",
        hold="parent is bovine pancreatic RNase A, but no accession for it was "
             "established from a fetched record; one lookup away",
        caveat="NOT BUILT: the parent is bovine pancreatic RNase A, but no UniProt "
               "accession for it was established from a fetched record in the session that "
               "produced this table - only the PDB entity above. Sourcing its nucleotides "
               "means first looking the parent up, not recalling it. One lookup away from "
               "buildable.",
    ),
    Part(
        name="AviTag",
        aliases=("Avi tag", "BAP", "biotin acceptor peptide", "BirA substrate peptide",
                 "GLNDIFEAQKIEWHE"),
        aa="GLNDIFEAQKIEWHE",
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
        description="Fifteen-residue peptide selected from a library as a substrate for "
                    "E. coli biotin ligase BirA, which attaches biotin to the single lysine "
                    "at position ten. It gives site-specific, stoichiometric, "
                    "enzymatically installed biotin on a recombinant protein - replacing "
                    "chemical biotinylation, which hits every surface lysine at random.",
        witness="RCSB seqmotif returns 537 entities for the 15-mer; PDB 11ZV entities 1 "
                "and 2 both carry it in the canonical GGS-flanked cassette. Schatz 1993's "
                "abstract confirms the library-selection provenance of the motif. NOTE: no "
                "PDB entity is *named* AviTag, so the name-to-sequence link rests on the "
                "two papers, not on a database label.",
        patent_flag="1",
        caveat="PATENT/TRADEMARK FLAG (not a determination): AviTag is an Avidity LLC "
               "brand and site-specific BirA biotinylation has been patented. Not assessed "
               "- no patent database was searched.",
    ),
    Part(
        name="SBP-tag",
        aliases=("SBP", "streptavidin-binding peptide"),
        aa="MDEKTTGWRGGHVVEGLAGELEQLRARLEHHPQGQREP",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="designed_sequence",
        boundary_evidence="PMID:11722181 (2001, "
                          "Protein Expr Purif 23:440-446)",
        citation="Keefe AD, Wilson DS, Seelig B, Szostak JW (2001) One-step purification "
                 "of recombinant proteins using a nanomolar-affinity streptavidin-binding "
                 "peptide, the SBP-Tag. Protein Expr Purif 23:440-446.",
        description="Thirty-eight-residue peptide isolated by mRNA display that binds "
                    "streptavidin about a hundred-fold more tightly than Strep-tag II, and "
                    "elutes with free biotin under native conditions. Long enough to be a "
                    "real domain rather than a linear epitope, which is the cost of the "
                    "affinity.",
        witness="PDB 4JO6 entity 2, pdbx_description 'SBP-Tag', 38-residue sequence, the "
                "entire entity. The length agrees with the paper's own abstract.",
        caveat="NOT BUILT: selected by mRNA display, so there is no natural gene to read "
               "codons out of. Patent status not assessed (Szostak lab).",
    ),
    Part(
        name="Calmodulin-binding peptide",
        aliases=("CBP", "CBP tag", "MLCK M13 peptide"),
        aa="KRRWKKNFIAVSAANRFKKISSSGAL",
        cls="synthetic_part",
        genbank_key="misc_feature",
        boundary_rule="literature_defined",
        boundary_evidence="PMID:1318232 (1992, "
                          "FEBS Lett 302:274-278) - the third unit of the kfc cassette",
        citation="Stofko-Hahn RE, Carr DW, Scott JD (1992) A single step purification for "
                 "recombinant proteins. Characterization of a microtubule associated "
                 "protein (MAP 2) fragment which associates with the type II "
                 "cAMP-dependent protein kinase. FEBS Lett 302:274-278.",
        description="The calmodulin-binding helix of skeletal-muscle myosin light-chain "
                    "kinase. It binds calmodulin only in the presence of calcium, so the "
                    "fusion is captured on a calmodulin resin and released by chelating "
                    "with EGTA - an elution step mild enough to keep complexes intact, "
                    "which is why this tag is the second half of the tandem affinity "
                    "purification tag.",
        witness="PDB 2BBM entity 2, 'MYOSIN LIGHT CHAIN KINASE', a 26-residue entity that "
                "is an exact match to the tag. The 1992 abstract describes the three-unit "
                "kfc cassette this peptide terminates.",
        hold="parent is myosin light-chain kinase, but no accession for it was "
             "established from a fetched record; one lookup away",
        caveat="NOT BUILT: same shape as S-tag. The parent is myosin light-chain kinase, "
               "but the session that produced this table established only the PDB entity, "
               "not a UniProt accession, and picking one from recall is exactly what this "
               "build forbids.",
    ),
    Part(
        name="ALFA-tag",
        aliases=("ALFA", "SRLEEELRRRLTE"),
        aa="SRLEEELRRRLTE",
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
        hold="parent is the TEV polyprotein, but no accession for it was established "
             "from a fetched record; one lookup away",
        caveat="NOT BUILT: no UniProt accession for the TEV polyprotein was established "
               "from a fetched record in the session that produced this table, so there is "
               "no verified parent to slice codons out of.",
    ),
    Part(
        name="TEV protease cleavage site (Ser variant)",
        aliases=("ENLYFQS", "ENLYFQ/S", "TEV site S variant"),
        aa="ENLYFQS",
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
        hold="same as the Gly variant: the TEV polyprotein accession was not "
             "established from a fetched record",
        caveat="NOT BUILT: same reason as the Gly variant.",
    ),
    Part(
        name="HRV 3C protease cleavage site",
        aliases=("PreScission site", "3C site", "LEVLFQGP", "LEVLFQ/GP"),
        aa="LEVLFQGP",
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
        hold="engineered P4 variant of the natural LETLFQ/GP junction, so no gene "
             "encodes this octamer",
        caveat="DISCREPANCY THE CURATOR MUST SEE, found by checking rather than assuming. "
               "The vector sequence is LEVLFQ/GP, but the natural HRV14 2C/3A junction is "
               "LETLFQ/GP: UniProt P03303 carries LETLFQGP at 1424-1431 and annotates the "
               "3C cleavage there, and Cordingley's abstract names ETLFQ/GP. LEVLFQ/GP is "
               "a P4 Thr-to-Val engineered variant and no paper located here publishes it. "
               "That is also why it is NOT BUILT: an engineered variant has no natural "
               "gene to take codons from. Do not let this row imply Cordingley published "
               "this octamer.",
    ),
    Part(
        name="Thrombin cleavage site",
        aliases=("thrombin site", "LVPRGS", "LVPR/GS"),
        aa="LVPRGS",
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
        hold="not a natural junction - checked absent from five human thrombin "
             "substrates - so no gene encodes it",
        caveat="WEAKEST ATTRIBUTION IN THIS TABLE, stated plainly. No paper was found that "
               "first publishes the exact hexamer LVPR/GS: Chang 1985 establishes the "
               "apolar requirement, and Smith & Johnson 1988 mention thrombin without "
               "printing residues. It is also NOT a natural junction - LVPR is absent from "
               "human prothrombin (P00734), fibrinogen alpha (P02671), factor XIII A "
               "(P00488), PAR1 (P25116) and factor V (P12259), all checked. Sequence "
               "verified; attribution unresolved; and with no natural parent there are no "
               "codons to take, so NOT BUILT.",
    ),
    Part(
        name="Factor Xa cleavage site",
        aliases=("factor Xa site", "IEGR", "Ile-Glu-Gly-Arg"),
        aa="IEGR",
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
        caveat="NOT BUILT, and held back by MIN_NT rather than by sourcing: the parent is "
               "clean, but four residues is twelve base pairs. As protein it is ~60,000x "
               "over the false-positive budget SOURCING.md sets; as nucleotide it is "
               "shorter than any k-mer seed a tier-1 index would use. It must never be "
               "indexed standalone - index the SSGHIEGRHM-style cassette context, or "
               "suppress it unless it abuts another annotated feature.",
    ),
    Part(
        name="Enterokinase cleavage site",
        aliases=("enteropeptidase site", "DDDDK", "Asp4-Lys"),
        aa="DDDDK",
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
        caveat="NOT BUILT, held back by MIN_NT: five residues is fifteen base pairs. "
               "SECOND PROBLEM the annotator must handle regardless of how this row is "
               "eventually sourced: DDDDK is the C-terminal half of the FLAG tag, so in "
               "any FLAG-tagged construct it will always co-hit FLAG and report a protease "
               "site the designer never put there. That needs an explicit containment "
               "rule.",
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


def build(refresh: bool) -> tuple[list, list]:
    """Return (rows, report), the shape every stage in this build returns."""
    rows, report = [], []
    built = blocked = 0

    for i, p in enumerate(PARTS):
        ordinal = i + 1
        rid = f"PLF:{PLF_BLOCK_BASE + i:04d}"
        tag = f"{rid} {p.name}"

        if not p.parent_uniprot:
            blocked += 1
            report.append(f"  HOLD {rid} {p.name:40s} {p.hold}")
            continue

        nt_needed = 3 * len(p.aa)
        if nt_needed < MIN_NT:
            blocked += 1
            report.append(
                f"  HOLD {rid} {p.name:40s} {len(p.aa)} aa = {nt_needed} bp, below the "
                f"{MIN_NT} bp floor; parent {p.parent_uniprot} is clean but the reference "
                f"would be unindexable"
            )
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
            f"nucleotide matching will MISS most real occurrences. The peptide is what "
            f"should be matched, and this row cannot carry it: the loader accepts "
            f"reference_aa only on class 'cds' (crates/pl-features/src/lib.rs:573) and "
            f"requires reference_nt on every row (lib.rs:556). Until that is decided, "
            f"treat this row as a placeholder that is right rather than useful. "
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
            # Empty, and not merely unset. See the note above and the module
            # docstring: this is the schema limit, not an omission.
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
        built += 1
        report.append(
            f"  OK   {rid} {p.name:34s} {len(nt):4d} bp  {pid} codons "
            f"{offset * 3 + 1}-{(offset + len(p.aa)) * 3}  (residues {offset + 1}-"
            f"{offset + len(p.aa)} of {p.parent_uniprot})"
        )

    report.append(
        f"  -- {built} built, {blocked} declared but not built. The held rows keep their "
        f"ordinals, so their PLF ids stay reserved and unissued."
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

    # Every part must be internally coherent, checked here rather than trusted.
    for i, p in enumerate(PARTS):
        if p.cls != "synthetic_part":
            raise SystemExit(f"SELF-TEST FAILED: {p.name} is class {p.cls!r}")
        if not p.aa or set(p.aa) - set("ACDEFGHIKLMNPQRSTVWY"):
            raise SystemExit(f"SELF-TEST FAILED: {p.name} has a non-amino-acid residue")
        if not p.citation or not p.boundary_evidence or not p.witness:
            raise SystemExit(f"SELF-TEST FAILED: {p.name} lacks a citation, boundary "
                             f"evidence or witness; Class C requires all three")
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
