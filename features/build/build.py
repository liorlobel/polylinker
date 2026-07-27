#!/usr/bin/env python3
"""Build polylinker-features from primary sources.

`docs/PLAN.md` §8.3 rule 5: *publish the build script, not just the output*. The
pipeline is the reproducibility claim, so this file is the deliverable as much
as the TSV it writes.

What it guarantees, and how
---------------------------

**No sequence is ever written from memory.** Every base comes from a fetched
record, and the fetch is cached with its sha256 so a reviewer can check that the
bytes this ran on are the bytes upstream served.

**Every coding boundary is *derived*, never chosen.** A CDS record is accepted
only if translating the nucleotides reproduces the reference protein exactly.
That is not a sanity check bolted on the end — it is the entire provenance story
for Class A features. We did not copy a boundary from anyone; we computed one,
and the arithmetic is reproducible from the accession.

That check is load-bearing rather than theoretical. UniProt's P62593 is a single
merged entry covering TEM-1/2/3/4/5/6/8/16/24, whose EMBL cross-references point
at *different alleles with different sequences* — taking the first one blindly
plants a wrong sequence under a right name.

**Everything it emits is `proposed`.** §8.3 rule 6: AI may propose, never
assert. Nothing here has a curator, and `Db::reviewed()` will refuse to ship any
of it until a human puts their name against each row. That is the intended
state of this output, not an unfinished one.

Sources cleared for use are recorded, with the evidence, in `features/SOURCING.md`.

Usage
-----
    python features/build/build.py            # build from cache, fetching what is missing
    python features/build/build.py --refresh  # re-fetch everything
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
CACHE = HERE / ".cache"
TODAY = os.environ.get("PLF_BUILD_DATE") or time.strftime("%Y-%m-%d")

UA = "polylinker-features-build/0.1 (https://github.com/polylinker/polylinker)"

AMR_BASE = (
    "https://ftp.ncbi.nlm.nih.gov/pathogen/Antimicrobial_resistance/"
    "AMRFinderPlus/database/latest"
)

# NCBI's standard genetic code, as the four-line NCBI form rather than 64
# hand-written entries. Index is 16*b1 + 4*b2 + b3 over "TCAG".
AAS = "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG"
ORDER = "TCAG"


# NCBI table 11 (bacterial) initiation codons. GTG and TTG are common real
# starts in transposon-borne markers — tet(A) begins GTG — and they are read as
# formyl-Met when they initiate, not as valine. Translating a CDS under table 1
# therefore disagrees with the catalogue at exactly residue 1, which looks like
# a data error and is actually a genetic-code error.
BACTERIAL_STARTS = {"TTG", "CTG", "ATT", "ATC", "ATA", "ATG", "GTG"}


def translate_cds(seq: str) -> str:
    """Translate a CDS, honouring alternative initiation codons."""
    aa = translate(seq)
    if aa and seq[:3].upper().replace("U", "T") in BACTERIAL_STARTS:
        aa = "M" + aa[1:]
    return aa


def translate(seq: str) -> str:
    out = []
    s = seq.upper().replace("U", "T")
    for i in range(0, len(s) - len(s) % 3, 3):
        try:
            idx = 16 * ORDER.index(s[i]) + 4 * ORDER.index(s[i + 1]) + ORDER.index(s[i + 2])
        except ValueError:
            out.append("X")
            continue
        out.append(AAS[idx])
    return "".join(out)


# --------------------------------------------------------------------------
# Fetching, with a cache that records what it got


def fetch(url: str, name: str, refresh: bool = False) -> bytes:
    """Fetch `url`, caching it, and never trust a cache we cannot verify.

    A cache hit used to be returned unchecked, and the payload was written
    before its metadata with no atomic rename. Interrupting a download therefore
    left a truncated file that the next run consumed silently: it emitted blank
    sha256 columns, stamped today's date on them, printed a false statement
    about upstream ("not present in this AMRFinderPlus release"), exited 0 --
    and, because the deterministic tie-break then saw a different candidate set,
    shipped *different nucleotide sequences* under the same marker names, with
    notes still asserting an exact translation match.
    """
    CACHE.mkdir(parents=True, exist_ok=True)
    path = CACHE / name
    meta = CACHE / (name + ".meta.json")
    if path.exists() and not refresh:
        data = path.read_bytes()
        recorded = cached_meta(name).get("sha256")
        if recorded and hashlib.sha256(data).hexdigest() == recorded:
            return data
        # Re-fetch rather than abort: someone with a pre-verification cache
        # should not be hard-blocked, they should just pay for one download.
        print(f"  cache for {name} is unverified or stale; re-fetching")

    req = urllib.request.Request(url, headers={"User-Agent": UA})
    for attempt in range(4):
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                data = r.read()
            break
        except (urllib.error.URLError, TimeoutError) as e:
            if attempt == 3:
                raise SystemExit(f"failed to fetch {url}: {e}")
            time.sleep(2 * (attempt + 1))

    # Metadata first, then an atomic rename of the payload. In that order a
    # crash can leave an orphaned meta file (harmless -- the payload is absent,
    # so the next run fetches) but never a payload the meta does not describe.
    digest = hashlib.sha256(data).hexdigest()
    meta.write_text(
        json.dumps(
            {"url": url, "retrieved": TODAY, "bytes": len(data), "sha256": digest},
            indent=2,
        )
    )
    part = path.with_suffix(path.suffix + ".part")
    part.write_bytes(data)
    os.replace(part, path)
    return data


def cached_meta(name: str) -> dict:
    p = CACHE / (name + ".meta.json")
    return json.loads(p.read_text()) if p.exists() else {}


def parse_fasta(text: str):
    header, chunks = None, []
    for line in text.splitlines():
        if line.startswith(">"):
            if header is not None:
                yield header, "".join(chunks)
            header, chunks = line[1:], []
        elif header is not None:
            chunks.append(line.strip())
    if header is not None:
        yield header, "".join(chunks)


# --------------------------------------------------------------------------
# The allow-list: which markers, and why each one is here


@dataclass(frozen=True)
class Marker:
    """A resistance/selection marker used in cloning vectors."""

    allele: str
    """Exact AMRFinderPlus allele symbol, e.g. `blaTEM-1`."""
    name: str
    """What a biologist calls it on a map."""
    aliases: tuple[str, ...]
    description: str
    """Written here, from the primary literature — never from SnapGene."""


# Chosen because they appear in ordinary academic cloning backbones. This list
# is deliberately short and hand-justified: §8.3 prefers a small, fully
# defensible set to a large one with soft provenance.
MARKERS: tuple[Marker, ...] = (
    Marker(
        "blaTEM-1", "AmpR", ("bla", "ampR", "TEM-1", "beta-lactamase"),
        "Class A beta-lactamase; hydrolyses the beta-lactam ring of penicillins, "
        "giving resistance to ampicillin and carbenicillin. Carried on Tn3 and "
        "the selection marker of the pUC and pBR322 lineages.",
    ),
    Marker(
        "aph(3')-Ia", "KanR", ("aphA1", "nptI", "neo", "kanR"),
        "Aminoglycoside 3'-O-phosphotransferase from Tn903; phosphorylates "
        "kanamycin and neomycin, giving resistance in bacteria.",
    ),
    Marker(
        "aph(3')-IIa", "NeoR/KanR", ("nptII", "neo", "aphA2"),
        "Aminoglycoside 3'-O-phosphotransferase from Tn5; the marker behind "
        "kanamycin selection in bacteria and G418 selection in eukaryotes.",
    ),
    Marker(
        "aadA1", "SmR", ("aadA", "specR", "strepR", "spc"),
        "Aminoglycoside 3''-adenylyltransferase; adenylylates streptomycin and "
        "spectinomycin. The classic integron-borne cassette marker.",
    ),
    Marker(
        "catA1", "CmR", ("cat", "cml", "cmR", "chloramphenicol acetyltransferase"),
        "Chloramphenicol acetyltransferase from Tn9; acetylates chloramphenicol "
        "so it can no longer bind the ribosomal peptidyl transferase centre.",
    ),
    Marker(
        "tet(A)", "TetA", ("tetA",),
        "Tetracycline efflux pump of the major facilitator superfamily; exports "
        "tetracycline in exchange for a proton.",
    ),
    Marker(
        "aac(3)-Ia", "GenR", ("aacC1", "gentamicin"),
        "Aminoglycoside 3-N-acetyltransferase Ia; acetylates gentamicin. Used "
        "where a marker orthogonal to ampicillin and kanamycin is needed.",
    ),
    Marker(
        "ble_Tn5", "BleoR", ("ble", "bleR", "phleomycin"),
        "Bleomycin-binding protein from Tn5; sequesters the drug "
        "stoichiometrically rather than modifying it, so resistance is "
        "dose-limited. The Sh ble variant is the basis of zeocin selection.",
    ),
)


@dataclass
class Row:
    id: str
    name: str
    aliases: list
    cls: str
    genbank_key: str
    reference_nt: str
    reference_aa: str
    boundary_rule: str
    boundary_evidence: str
    description: str
    notes: str = ""
    patent_flag: str = "0"
    provenance: list = field(default_factory=list)


def esc(s: str) -> str:
    return s.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n")


# --------------------------------------------------------------------------
# Stage 1 — AMRFinderPlus


def stage_amrfinder(refresh: bool) -> tuple[list, list]:
    """Resistance markers with verified CDS, protein, and real coordinates.

    Chosen as the first stage because it is the best value per unit of legal
    risk: an actively maintained NCBI catalogue that ships *both* the nucleotide
    CDS and the protein, with standard allele nomenclature and the coordinates
    of each CDS in a real record — so the boundary is derivable rather than
    inherited from anyone's curation.
    """
    cds_raw = fetch(f"{AMR_BASE}/AMR_CDS.fa", "AMR_CDS.fa", refresh).decode("utf8", "replace")
    prot_raw = fetch(f"{AMR_BASE}/AMRProt.fa", "AMRProt.fa", refresh).decode("utf8", "replace")
    cds_meta = cached_meta("AMR_CDS.fa")
    prot_meta = cached_meta("AMRProt.fa")

    # >WP_000018321.1|NG_056047.1|1|1|aph(3')-Ia|aph(3')-Ia|product NG_056047.1:101-916
    proteins: dict[str, str] = {}
    for h, seq in parse_fasta(prot_raw):
        proteins[h.split("|", 1)[0]] = seq.upper()

    by_allele: dict[str, list] = {}
    for h, seq in parse_fasta(cds_raw):
        parts = h.split("|")
        if len(parts) < 7:
            continue
        prot_acc, nucl_acc, allele = parts[0], parts[1], parts[4]
        coords = re.search(r"(\S+):(\d+)-(\d+)", parts[6])
        by_allele.setdefault(allele, []).append(
            {
                "protein_acc": prot_acc,
                "nucl_acc": nucl_acc,
                "product": parts[6].split(" ")[0],
                "coords": coords.groups() if coords else None,
                "seq": seq.upper(),
            }
        )

    rows, report = [], []
    for i, m in enumerate(MARKERS, start=1):
        candidates = by_allele.get(m.allele, [])
        if not candidates:
            report.append(f"  SKIP {m.allele:14s} not present in this AMRFinderPlus release")
            continue

        # Deterministic choice: verified entries only, then the lexicographically
        # smallest nucleotide accession. Arbitrary but *stated* and stable, so
        # two builds of the same release agree byte for byte.
        verified = []
        for c in candidates:
            aa = proteins.get(c["protein_acc"])
            if not aa:
                continue
            # AMRProt writes the terminal stop as `*`; our translation does too.
            # Strip it from both sides rather than one, which is a mistake that
            # looks like "nothing verifies" and reads like a data problem.
            if translate_cds(c["seq"]).rstrip("*") == aa.rstrip("*"):
                verified.append(c)

        if not verified:
            report.append(
                f"  SKIP {m.allele:14s} {len(candidates)} candidate(s), none whose CDS "
                f"translates to its own protein"
            )
            continue

        best = sorted(verified, key=lambda c: (c["nucl_acc"], c["protein_acc"]))[0]
        # Stored without the stop: the reference is the protein, and a `*` in
        # the index would be a symbol the query's frames only produce at a stop.
        aa = proteins[best["protein_acc"]].rstrip("*")
        acc, start, end = best["coords"] if best["coords"] else (best["nucl_acc"], "?", "?")

        rid = f"PLF:{i:04d}"
        rows.append(
            Row(
                id=rid,
                name=m.name,
                aliases=list(m.aliases) + [m.allele],
                cls="cds",
                genbank_key="CDS",
                reference_nt=best["seq"],
                reference_aa=aa,
                boundary_rule="orf_atg_to_stop",
                boundary_evidence=f"{acc}:{start}-{end}:+",
                description=m.description,
                notes=(
                    f"CDS verified by translation against {best['protein_acc']}: "
                    f"{len(best['seq'])} nt -> {len(aa)} aa, exact match "
                    f"(start codon {best['seq'][:3]}, NCBI table 11). "
                    f"{len(verified)}/{len(candidates)} catalogue entries for this allele "
                    f"passed the same check."
                ),
                # One row per field, each pointing at the file the bytes
                # actually came from. Attributing all three to AMR_CDS.fa
                # stamped that file's sha256 on the amino acids, which live in
                # AMRProt.fa -- so a reviewer doing exactly what features/NOTICE
                # promises would verify the hash and then fail to find the
                # protein in the file. `aliases` are hand-written in MARKERS
                # above, so they are our work, like `name` below.
                provenance=[
                    (rid, "reference_nt", "amrfinderplus", best["protein_acc"],
                     "INSDC-free", f"{AMR_BASE}/AMR_CDS.fa",
                     cds_meta.get("retrieved", TODAY), cds_meta.get("sha256", "")),
                    (rid, "reference_aa", "amrfinderplus", best["protein_acc"],
                     "INSDC-free", f"{AMR_BASE}/AMRProt.fa",
                     prot_meta.get("retrieved", TODAY), prot_meta.get("sha256", "")),
                    (rid, "boundary_evidence", "amrfinderplus", best["nucl_acc"],
                     "INSDC-free", f"{AMR_BASE}/AMR_CDS.fa",
                     cds_meta.get("retrieved", TODAY), cds_meta.get("sha256", "")),
                    (rid, "aliases", "polylinker", "-", "own-work", "-", TODAY, ""),
                    (rid, "boundary_rule", "polylinker", "-", "own-work", "-", TODAY, ""),
                    (rid, "description", "polylinker", "-", "own-work", "-", TODAY, ""),
                    (rid, "name", "polylinker", "-", "own-work", "-", TODAY, ""),
                ],
            )
        )
        report.append(
            f"  OK   {m.allele:14s} -> {m.name:10s} {len(best['seq']):5d} nt  "
            f"{acc}:{start}-{end}  ({len(verified)}/{len(candidates)} verified)"
        )
    return rows, report


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--refresh", action="store_true", help="re-fetch every source")
    ap.add_argument("--out", default=str(ROOT), help="output directory")
    args = ap.parse_args()

    print("polylinker-features build")
    print(f"  date {TODAY}")
    print("\nStage 1 — AMRFinderPlus resistance markers")
    rows, report = stage_amrfinder(args.refresh)
    print("\n".join(report))

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    from lib_columns import FEATURE_COLUMNS, PROVENANCE_COLUMNS  # noqa: E402

    with (out / "features.tsv").open("w", encoding="utf8", newline="\n") as fh:
        fh.write(f"#!version {TODAY.replace('-', '.')}\n")
        fh.write(
            "# Generated by features/build/build.py. Every row is 'proposed':\n"
            "# machine-extracted, no human has signed off, NOT shippable.\n"
            "# See features/SOURCING.md for how each source was cleared.\n"
        )
        fh.write("\t".join(FEATURE_COLUMNS) + "\n")
        for r in rows:
            fh.write(
                "\t".join(
                    [
                        r.id, r.name, "|".join(r.aliases), r.cls, r.genbank_key,
                        r.reference_nt, r.reference_aa, r.boundary_rule,
                        r.boundary_evidence, esc(r.description),
                        "proposed", "", TODAY, r.patent_flag, esc(r.notes),
                    ]
                )
                + "\n"
            )

    with (out / "provenance.tsv").open("w", encoding="utf8", newline="\n") as fh:
        fh.write("\t".join(PROVENANCE_COLUMNS) + "\n")
        for r in rows:
            for p in r.provenance:
                # A row citing an external source must carry the hash of the
                # file it was read from; that hash is the entire claim. Checked
                # here rather than in the loader, because half the shipped rows
                # are own-work and legitimately have no hash — adding the column
                # to the loader's required-field list would reject our own
                # database.
                if p[4] != "own-work" and not p[7]:
                    raise SystemExit(
                        f"{p[0]} field {p[1]}: cites {p[2]} with no sha256 — the "
                        f"source cache is unverified, refusing to write"
                    )
                fh.write("\t".join(str(x) for x in p) + "\n")

    print(f"\nwrote {len(rows)} records to {out / 'features.tsv'}")
    print(f"      {sum(len(r.provenance) for r in rows)} provenance rows")
    print("\nAll rows are 'proposed'. Db::reviewed() will ship none of them until")
    print("a curator signs each one off. That is the intended state.")
    return 0


if __name__ == "__main__":
    sys.path.insert(0, str(HERE))
    raise SystemExit(main())
