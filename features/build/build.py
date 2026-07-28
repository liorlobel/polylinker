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

**IDs are allocated centrally, from the declaration and not from the outcome.**
See `STAGES` and `allocate()` below. A PLF id is a permanent name for a record;
if adding a stage, or dropping one marker that failed verification, renumbered
the rows after it, every downstream reference would silently come to mean a
different sequence. That is the worst failure this file can have, so the id is
a function of *where a row is declared*, never of how many rows happened to
survive this run, and the previous `features.tsv` is re-read at the end and
compared row by row to prove no id changed meaning.

Sources cleared for use are recorded, with the evidence, in `features/SOURCING.md`.

Usage
-----
    python features/build/build.py            # build from cache, fetching what is missing
    python features/build/build.py --refresh  # re-fetch everything

Exit status is 1 if any row was rejected or any stage failed. The TSV written is
always loadable — rejected rows are reported and left out — but a non-zero exit
means the build is incomplete and something needs a human.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib
import json
import os
import re
import sys
import time
import traceback
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
CACHE = HERE / ".cache"
TODAY = os.environ.get("PLF_BUILD_DATE") or time.strftime("%Y-%m-%d")

# The stage modules sit beside this file. Putting HERE on the path at import
# time rather than under `if __name__ == "__main__"` means `import build` and
# `python features/build/build.py` from any working directory both find
# lib_columns and the stage modules; without it the build works from the repo
# root and mysteriously does not from inside features/build.
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from lib_columns import FEATURE_COLUMNS, PROVENANCE_COLUMNS  # noqa: E402

UA = "polylinker-features-build/0.1 (https://github.com/polylinker/polylinker)"

AMR_BASE = (
    "https://ftp.ncbi.nlm.nih.gov/pathogen/Antimicrobial_resistance/"
    "AMRFinderPlus/database/latest"
)

# NCBI's standard genetic code, as the four-line NCBI form rather than 64
# hand-written entries. Index is 16*b1 + 4*b2 + b3 over "TCAG".
AAS = "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG"
ORDER = "TCAG"


# Initiation codons other than ATG. GTG and TTG are common real starts in
# transposon-borne markers — tet(A) begins GTG — and they are read as
# formyl-Met when they initiate, not as valine. Translating a CDS under table 1
# therefore disagrees with the catalogue at exactly residue 1, which looks like
# a data error and is actually a genetic-code error.
#
# This is load-bearing for six of the 24 markers below, not decorative: tet(A),
# aph(7'')-Ia and aac(3)-IVa start GTG; tet(K) and aph(3'')-Ib start TTG.
ALT_INITIATORS = {"TTG", "CTG", "ATT", "ATC", "ATA", "GTG"}


def initiator(seq: str) -> str:
    return seq[:3].upper().replace("U", "T")


def translate_cds(seq: str) -> str:
    """Translate a CDS, reading an alternative initiation codon as Met.

    KEEP THIS OFF THE VERIFICATION PATH. It rewrites residue 1, so
    `translate_cds(cds) == protein` is not the exact-match test it reads as: a
    CDS that disagrees with its protein at position 1 passes it silently. Use
    `cds_matches_protein()` for any comparison whose outcome is written into a
    row, or into a `notes` field claiming a match was checked.
    """
    aa = translate(seq)
    if aa and initiator(seq) in ALT_INITIATORS | {"ATG"}:
        aa = "M" + aa[1:]
    return aa


def cds_matches_protein(seq: str, protein: str) -> tuple[bool, str]:
    """Does this CDS translate to this protein? Returns (ok, what was accepted).

    This is the gate the whole database rests on, so it reports what it accepted
    instead of normalising the difference away.

    `translate_cds` used to *be* the gate. It rewrites residue 1 to Met whenever
    the first codon is an alternative initiator — correct for a bacterial CDS,
    and the reason PLF:0006 and five siblings verify at all — but it means
    "translates to the canonical exactly" was never true of residue 1 for those
    rows. One row then shipped a `notes` field asserting a whole-CDS exact match
    that had not been performed, on a *human* MYC transcript with a CTG
    initiator, reached by a rule whose justifying comment names six bacterial
    markers. The check was fine; the sentence describing it was false.

    So an exact whole-CDS match and an initiator-only difference are two
    different results with two different sentences, and the caller is expected
    to put the sentence it got into the row. Anything else is a mismatch and the
    row is dropped — including a difference at position 2, which no initiation
    rule excuses. `self_test()` asserts exactly that.
    """
    aa = translate(seq).rstrip("*")
    want = protein.rstrip("*")
    cod = initiator(seq)
    if aa == want:
        return True, f"initiator {cod}; the whole CDS translates residue for residue"
    if (
        want[:1] == "M"
        and cod in ALT_INITIATORS
        and len(aa) == len(want)
        and aa[1:] == want[1:]
    ):
        return True, (
            f"initiator {cod} is a non-AUG initiation codon read as Met, per the "
            f"reference entry's own annotation; residues 2-{len(want)} match the "
            f"reference protein exactly, and residue 1 is the only position at which "
            f"a naive table-1 translation would differ"
        )
    return False, ""


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


# Hosts SOURCING.md §1 cleared as *data* sources, and nothing else. A pinned
# accession is only as safe as the host it is pinned on: this build fetches by
# accession, and an accession is a string somebody types, so "fetch whatever URL
# the caller built" is one typo away from pulling a sequence out of Addgene or
# PlasMapper and stamping our own provenance on it. stage_rfam already guarded
# its own fetches this way; the other three stages did not, so the guard lives
# here where every stage inherits it.
#
# NOT here, deliberately, and every one of them is a NO_GO or HOLD in §1:
# addgene.org, plasmapper.ca, seva-plasmids.com, fpbase.org, the UniVec files on
# the NCBI FTP root, and raw.githubusercontent.com (pLannotate's snapgene.csv).
ALLOWED_FETCH_HOSTS = {
    "ftp.ncbi.nlm.nih.gov",   # AMRFinderPlus catalogue
    "eutils.ncbi.nlm.nih.gov",  # E-utilities, for GenBank records
    "rest.uniprot.org",       # UniProt entries
    "www.ebi.ac.uk",          # ENA browser API
    "ftp.ebi.ac.uk",          # Rfam CURRENT
}


def check_fetch_host(url: str) -> None:
    host = urllib.parse.urlsplit(url).hostname or ""
    if host.lower() not in ALLOWED_FETCH_HOSTS:
        raise SystemExit(
            f"refusing to fetch {url}: {host!r} is not a source features/SOURCING.md "
            f"§1 cleared. Cleared hosts: {sorted(ALLOWED_FETCH_HOSTS)}. Adding one is "
            f"a sourcing decision with a licence behind it, not a build fix."
        )


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
    check_fetch_host(url)
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
# Rules every stage's output is held to, in one place


def assert_ascii(rid: str, field: str, text: str) -> str:
    """Return "" if `text` is ASCII, else the reason it is not.

    Not aesthetic, and not optional. Two of the stages decode **latin-1** source
    files, and a downstream reader that opens `features.tsv` under the Windows
    default codepage rather than UTF-8 mangles the first non-ASCII byte it
    meets — silently, because the provenance sha256 covers the upstream file and
    not our table, so the mangled cell verifies clean. PLF:1000's description
    read `O1 <mojibake> with auxiliary sites O2` under cp1252 and nothing
    complained.

    A high byte in a field we author is therefore either a typo or, much worse,
    evidence that source bytes have leaked into our own prose — the boundary the
    whole licence story rests on. stage_rfam had this check and self-tested it;
    stage_uniprot did not have it at all and shipped 16 em dashes and a section
    sign. A rule one stage can opt out of by omission is not a rule, so it lives
    here, in `validate_row`, where the stage does not get a vote.
    """
    if text.isascii():
        return ""
    bad = sorted({c for c in text if not c.isascii()})
    return (
        f"{field} carries non-ASCII {bad}. Either a typo, or latin-1 source bytes "
        f"leaking into a field we author; {rid} would mangle under cp1252."
    )


def merge_aliases(*parts) -> list:
    """Flatten alias sources into one list, case-insensitively de-duplicated.

    Appending the catalogue's allele symbol to a hand-written alias tuple
    produced `catP|cmR|thiamphenicol|catP` — the same string twice — and five
    case-variant pairs such as `ANT(9)-Ia` beside `ant(9)-Ia`. Neither is wrong
    as data, but SOURCING.md §3 makes the alias table the mechanism that
    collapses spellings onto one record, so a table that carries a spelling
    twice is the one artefact that mechanism must not produce.

    First spelling seen wins, so the hand-written display form survives and the
    catalogue symbol is added only when it is genuinely new.
    """
    out, seen = [], set()
    for part in parts:
        for a in [part] if isinstance(part, str) else list(part):
            a = str(a).strip()
            if a and a.casefold() not in seen:
                seen.add(a.casefold())
                out.append(a)
    return out


# Which (source_db, licence) pairs may appear in provenance.tsv, from
# SOURCING.md §1. This is the NO_GO list made mechanical.
#
# It existed only as author discipline before, and discipline is not a control:
# two provenance rows citing `addgene` and `plasmapper` were appended to a copy
# of the shipped table by hand and every checker in the project passed them,
# counted them, and certified the result green. The Rust loader was no better —
# it requires `licence` to be non-empty and nothing else.
#
# `polylinker / own-work` is us. `insdc-ft` is the INSDC feature-table
# specification, whose own licence SOURCING.md Risk 4 records as [UNVERIFIED];
# that is why its licence string says so out loud rather than reading like a
# grant we have.
CLEARED_SOURCES: dict[str, set[str]] = {
    "polylinker": {"own-work"},
    "amrfinderplus": {"INSDC-free"},
    "ena": {"INSDC-free"},
    "genbank": {"INSDC-free"},
    "uniprot": {"CC-BY-4.0"},
    "rfam": {"CC0-1.0"},
    "insdc-ft": {"unresolved-see-SOURCING-Risk-4"},
}

# Licences whose provenance rows legitimately carry no sha256, because nothing
# was fetched. `own-work` is us. The INSDC feature-table specification is the
# other, and its blank hash is the disclosure rather than an oversight:
# SOURCING.md Risk 4 records that specification's own licence as [UNVERIFIED],
# and insdc.org is not on the cleared fetch list, so it was deliberately not
# retrieved. Everything else must show the hash of the bytes it was read from.
UNFETCHED_LICENCES = {"own-work", "unresolved-see-SOURCING-Risk-4"}


def check_provenance(rid: str, prov) -> list:
    """Every rule a provenance row must satisfy. Returns a list of reasons."""
    bad = []
    for p in prov:
        field, source_db, licence = p[1], p[2], p[4]
        # A `field` value that is not a column covers nothing, and nothing
        # noticed: 40 shipped rows keyed on `citation` and `peptide_anchor`,
        # neither of which is in the schema, while the column the sourced text
        # actually landed in was labelled own-work. A misspelling had the same
        # effect and was equally silent.
        if field not in FEATURE_COLUMNS:
            bad.append(
                f"{rid}: provenance names field {field!r}, which is not a column of "
                f"features.tsv, so it attributes nothing. Attribute the column the "
                f"text is actually written into."
            )
        allowed = CLEARED_SOURCES.get(source_db)
        if allowed is None:
            bad.append(
                f"{rid} field {field}: source_db {source_db!r} is not a source "
                f"features/SOURCING.md §1 cleared for use as data"
            )
        elif licence not in allowed:
            bad.append(
                f"{rid} field {field}: {source_db} is cleared under {sorted(allowed)}, "
                f"not {licence!r}"
            )
    return bad


# Columns that are the build's own bookkeeping rather than sourced content, and
# so are exempt from "every populated field needs provenance". `id` is ours by
# construction (SOURCING.md §0.5 naming firewall), `review_status`/`curator` are
# the sign-off protocol, and `date_added` is the clock.
PROVENANCE_EXEMPT = {"id", "review_status", "curator", "date_added"}


# --------------------------------------------------------------------------
# Measuring relatedness, because three descriptions asserted it and were wrong


# BLOSUM62, as the upper triangle in one string. Written out rather than
# imported so this file keeps its "no dependencies" property, and stored
# triangular so the two halves cannot disagree.
_B62_ORDER = "ARNDCQEGHILKMFPSTWYVBZX*"
_B62 = """
 4-1-2-2 0-1-1 0-2-1-1-1-1-2-1 1 0-3-2 0-2-1 0-4
  5 0-2-3 1 0-2 0-3-2 2-1-3-2-1-1-3-2-3-1 0-1-4
    6 1-3 0 0 0 1-3-3 0-2-3-2 1 0-4-2-3 3 0-1-4
      6-3 0 2-1-1-3-4-1-3-3-1 0-1-4-3-3 4 1-1-4
        9-3-4-3-3-1-1-3-1-2-3-1-1-2-2-1-3-3-2-4
          5 2-2 0-3-2 1 0-3-1 0-1-2-1-2 0 3-1-4
            5-2 0-3-3 1-2-3-1 0-1-3-2-2 1 4-1-4
              6-2-4-4-2-3-3-2 0-2-2-3-3-1-2-1-4
                8-3-3-1-2-1-2-1-2-2 2-3 0 0-1-4
                  4 2-3 1 0-3-2-1-3-1 3-3-3-1-4
                    4-2 2 0-3-2-1-2-1 1-4-3-1-4
                      5-1-3-1 0-1-3-2-2 0 1-1-4
                        5 0-2-1-1-1-1 1-3-1-1-4
                          6-4-2-2 1 3-1-3-3-1-4
                            7-1-1-4-3-2-2-1-2-4
                              4 1-3-2-2 0 0 0-4
                                5-2-2 0-1-1 0-4
                                 11 2-3-4-3-2-4
                                    7-1-3-2-1-4
                                      4-3-2-1-4
                                        4 1-1-4
                                          4-1-4
                                            1-4
                                              1
"""


def _blosum62() -> dict:
    m, rows = {}, [r for r in _B62.strip("\n").split("\n") if r.strip()]
    for i, row in enumerate(rows):
        vals = re.findall(r"-?\d+", row)
        for k, v in enumerate(vals):
            a, b = _B62_ORDER[i], _B62_ORDER[i + k]
            m[(a, b)] = m[(b, a)] = int(v)
    return m


B62 = _blosum62()


def percent_identity(a: str, b: str) -> tuple:
    """Global alignment identity, BLOSUM62 with affine gaps (-11 open, -1 extend).

    Here because three shipped descriptions made a claim about sequence
    relatedness and all three were wrong in the same direction: CatP was called
    "a distinct protein family from the CatA enzymes", ANT(9)-Ia "unrelated in
    sequence" to AadA, and APH(3')-IIIa said to share "almost no sequence" with
    the Tn5 and Tn903 enzymes. Each is a *negative* claim about two sequences
    this database already holds, which makes it the cheapest possible thing to
    check and the most embarrassing thing to get wrong. So `Marker.homology`
    declares the band, and the build measures it.

    Returns (identical, aligned_length, percent).
    """
    n, m = len(a), len(b)
    NEG = float("-inf")
    prev_m = [NEG] * (m + 1)
    prev_x = [NEG] * (m + 1)
    prev_y = [NEG] * (m + 1)
    prev_m[0] = 0.0
    for j in range(1, m + 1):
        prev_y[j] = -11.0 - (j - 1)
    # Traceback pointers, one row per i, packed as small ints.
    ptr = [[(0, 0, 0)] * (m + 1) for _ in range(n + 1)]
    for i in range(1, n + 1):
        cur_m = [NEG] * (m + 1)
        cur_x = [NEG] * (m + 1)
        cur_y = [NEG] * (m + 1)
        cur_x[0] = -11.0 - (i - 1)
        for j in range(1, m + 1):
            s = B62.get((a[i - 1], b[j - 1]), -4)
            best = max(prev_m[j - 1], prev_x[j - 1], prev_y[j - 1])
            cur_m[j] = best + s
            pm = 0 if best == prev_m[j - 1] else (1 if best == prev_x[j - 1] else 2)
            o, e = prev_m[j] - 11.0, prev_x[j] - 1.0
            cur_x[j] = max(o, e)
            px = 0 if o >= e else 1
            o, e = cur_m[j - 1] - 11.0, cur_y[j - 1] - 1.0
            cur_y[j] = max(o, e)
            py = 0 if o >= e else 2
            ptr[i][j] = (pm, px, py)
        prev_m, prev_x, prev_y = cur_m, cur_x, cur_y
    i, j = n, m
    state = max(range(3), key=lambda k: (prev_m, prev_x, prev_y)[k][m])
    ident = alen = 0
    while i > 0 and j > 0:
        pm, px, py = ptr[i][j]
        if state == 0:
            alen += 1
            if a[i - 1] == b[j - 1]:
                ident += 1
            state, i, j = pm, i - 1, j - 1
        elif state == 1:
            alen += 1
            state, i = px, i - 1
        else:
            alen += 1
            state, j = py, j - 1
    alen += i + j
    return ident, alen, 100.0 * ident / alen if alen else 0.0


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
    """Written here, from the primary literature — never from SnapGene.

    Kept to plain ASCII, unlike the comments around it. The shipped TSV has been
    ASCII-only since the first eight rows, and a downstream reader that opens it
    under the Windows default codepage rather than UTF-8 mangles the first
    non-ASCII byte it meets. Not worth an em dash.
    """
    why: str = ""
    """Why this allele earns a row: MEASURED against a fetched record, or ASSERTED.

    Appended to the row's `notes`, which is curator-facing, and kept out of
    `description`, which is user-facing — so the distinction reaches the person
    who has to sign the row off without reaching a user as settled fact. It used
    to be source-only, and the cost of that was concrete: 20 of 24 descriptions
    rested on an assertion and nothing in the shipped table said so, while one
    `why` reading "that clause needs a citation or removal" sat in this file for
    a full release with the clause shipping unqualified.
    """
    note: str = ""
    """Curator-facing caveat, appended to the row's generated `notes`."""
    homology: tuple = ()
    """Relatedness the description asserts, MEASURED against the shipped protein.

    Each entry is `(other_allele, low, high)`: the build aligns this row's
    protein against that allele's, writes the measured identity into `notes`,
    and FAILS if it lands outside the declared band.

    Every entry here exists because a shipped description made a claim about two
    proteins this database already holds and got it backwards — CatP called "a
    distinct protein family from the CatA enzymes" at 44% identity, ANT(9)-Ia
    "unrelated in sequence" to AadA at 34%, APH(3')-IIIa sharing "almost no
    sequence" with enzymes it is 30% identical to, and a `why` asserting "roughly
    17%" for a pair that measures 31%. That is the cheapest possible claim to
    check and the most embarrassing one to get wrong, so it is checked.
    """
    patent: str = ""
    """If non-empty: sets `patent_flag` to 1 and is appended to `notes`.

    A flag, never a determination. No patent database was searched by anything
    in this repository, and SOURCING.md Risk 6 is explicit that patent status is
    a question for counsel.
    """


# Chosen because they appear in ordinary academic cloning backbones. This list
# is deliberately short and hand-justified: §8.3 prefers a small, fully
# defensible set to a large one with soft provenance.
#
# ORDER IS IDENTITY. A marker's position in this tuple is its PLF id (see
# `allocate()`), so this list is APPEND-ONLY: never reorder it, never delete
# from it. To retire a marker, leave the entry and stop it verifying — a hole in
# the id space is free, a renumbering is a silent data corruption. Positions
# 1-8 are the eight markers already published in features.tsv and must stay put.
#
# The 16 entries after them come from a survey that re-derived the allow-list
# against the cached catalogue and, where it could, tested the vector claim by
# six-frame searching fetched backbone records rather than recalling it. Two of
# its results contradict what this list would otherwise have asserted, and both
# are now rows: pBR322/pACYC184 carry tet(C), not the tet(A) at PLF:0006, and
# the pUC lineage carries blaTEM-116, not the TEM-1 at PLF:0001. Every `why`
# below preserves that survey's own MEASURED/ASSERTED verdict verbatim in
# spirit; nothing here upgrades an assertion into a measurement.
#
# FLAG FOR THE CURATOR, on rows 1-8 only. Their shipped descriptions attribute
# blaTEM-1 to Tn3, aph(3')-Ia to Tn903, aph(3')-IIa and ble_Tn5 to Tn5, and
# catA1 to Tn9. The NG_ reference records the builder actually selects do not
# state those provenances (ble_Tn5's record is an E. coli plasmid, not Tn5), so
# each clause needs a primary-literature citation or removal. They are left
# unedited deliberately: rewriting the description of an already-published id is
# a curator's call, not the build script's.
MARKERS: tuple[Marker, ...] = (
    Marker(
        "blaTEM-1", "AmpR", ("bla", "ampR", "TEM-1", "beta-lactamase"),
        "Class A beta-lactamase; hydrolyses the beta-lactam ring of penicillins, "
        "giving resistance to ampicillin and carbenicillin. Carried on Tn3 and "
        "the selection marker of the pUC and pBR322 lineages.",
        why="MEASURED: the survey found this protein exactly in fetched pBR322 "
            "and pGEX-2T records. Note it is NOT the pUC19 allele -- see blaTEM-116.",
    ),
    Marker(
        "aph(3')-Ia", "KanR", ("aphA1", "nptI", "neo", "kanR"),
        "Aminoglycoside 3'-O-phosphotransferase from Tn903; phosphorylates "
        "kanamycin and neomycin, giving resistance in bacteria.",
        why="ASSERTED: the kanamycin phosphotransferase of pACYC177 and the "
            "pUC4K cassette. Reference record is Escherichia coli.",
        note="This allele's catalogue entries span roughly 96.7% identity, so "
             "the deterministic tie-break is picking one member of a variant "
             "family, not the only sequence available.",
    ),
    Marker(
        "aph(3')-IIa", "NeoR/KanR", ("nptII", "neo", "aphA2"),
        "Aminoglycoside 3'-O-phosphotransferase from Tn5; the marker behind "
        "kanamycin selection in bacteria and G418 selection in eukaryotes.",
        why="ASSERTED: kanamycin selection in the pET series, G418 selection in "
            "mammalian vectors. Reference record is Escherichia coli.",
    ),
    Marker(
        "aadA1", "SmR", ("aadA", "specR", "strepR", "spc"),
        "Bifunctional aminoglycoside ANT(3'')(9) adenylyltransferase; "
        "adenylylates streptomycin at the 3''-hydroxyl and spectinomycin at the "
        "9-hydroxyl. The classic integron-borne cassette marker.",
        why="ASSERTED: the interposon cassette used wherever a marker orthogonal "
            "to ampicillin and kanamycin is wanted. Record is titled "
            "'Escherichia coli R6-5 aadA1 gene for ANT(3'')-Ia family "
            "aminoglycoside nucleotidyltransferase AadA1'. 'ANT(3'')(9)' replaces "
            "a plain '3''-adenylyltransferase' that understated the enzyme by "
            "omitting the spectinomycin half, which is the half PLF:0011 shares.",
    ),
    Marker(
        "catA1", "CmR", ("cat", "cml", "cmR", "chloramphenicol acetyltransferase"),
        "Chloramphenicol acetyltransferase from Tn9; acetylates chloramphenicol "
        "so it can no longer bind the ribosomal peptidyl transferase centre.",
        why="MEASURED: the survey found this protein exactly in a fetched "
            "pACYC184 record.",
    ),
    Marker(
        "tet(A)", "TetA", ("tetA(A)",),
        "Tetracycline efflux pump of the major facilitator superfamily; exports "
        "tetracycline in exchange for a proton.",
        why="ASSERTED: the RP4/Tn1721-type pump of broad-host-range vectors. "
            "Record is Pseudomonas aeruginosa RP4. NOT the pBR322 tet -- that is "
            "tet(C), and the survey measured the difference.",
        note="Catalogue entries for this allele span roughly 92.5% identity; the "
             "selected sequence is one member of a variant family.",
    ),
    Marker(
        "aac(3)-Ia", "GenR", ("aacC1",),
        "Aminoglycoside 3-N-acetyltransferase Ia; acetylates gentamicin. Used "
        "where a marker orthogonal to ampicillin and kanamycin is needed.",
        why="ASSERTED: the gentamicin marker of broad-host-range constructs. "
            "Record is Serratia marcescens pUO901.",
    ),
    Marker(
        "ble_Tn5", "BleoR", ("ble", "bleR"),
        "Bleomycin-binding protein of the transposon Tn5 ble determinant; "
        "sequesters the drug stoichiometrically rather than modifying it, so "
        "resistance is dose-limited. Zeocin selection uses a different protein "
        "again: PLF:0018 carries Sh ble, from the actinomycete "
        "Streptoalloteichus hindustanus, which belongs to the same family but "
        "is not a variant of this one.",
        why="MEASURED, after the Tn5 clause was challenged. The NG_ record the "
            "builder selects is an Escherichia coli plasmid ble gene and does not "
            "corroborate Tn5, but the shipped 126 aa protein is byte-identical to "
            "UniProt P13081 (Klebsiella pneumoniae), whose MISCELLANEOUS comment "
            "reads 'This enzyme is encoded by the kanamycin and neomycin "
            "resistance transposon Tn5'. That entry, fetched, is the citation the "
            "clause needed. The zeocin sentence was rewritten at the same time: "
            "PLF:0018 is byte-identical to UniProt P17493 and is a separate gene "
            "from a separate phylum, not a variant of this one.",
        homology=(("ble-Sh", 18.0, 32.0),),
        patent="Phleomycin/bleomycin selection reagents are commercially "
               "exploited. Flagged for counsel; no patent record was fetched and "
               "this is not a determination.",
    ),
    # ---- appended below this line; positions 9+ are new ids ----
    Marker(
        "blaTEM-116", "AmpR (TEM-116)", ("bla", "ampR", "TEM-116", "beta-lactamase"),
        "Class A beta-lactamase of the TEM family. It hydrolyses the beta-lactam "
        "ring exactly as TEM-1 does, differing from it by a couple of "
        "substitutions that leave the catalytic mechanism unchanged. It is a "
        "separate record rather than a synonym because the two are told apart "
        "only by exact protein identity.",
        why="MEASURED, and the single most valuable addition: the survey found "
            "this, not TEM-1, to be the exact AmpR protein of both independent "
            "pUC19 records and of pBluescript II KS(+). A TEM-1-only database "
            "exact-matches none of the pUC lineage.",
        note="Sibling of PLF:0001. A construct carrying one will not exact-match "
             "the other, which is why both are here; the survey also found a "
             "yeast shuttle backbone that matches neither, so the Tier 2 matcher "
             "must tolerate near-misses rather than assume exact identity.",
    ),
    Marker(
        "aph(3')-IIIa", "KanR (aphA-3)", ("aphA-3", "aphA3", "kanR"),
        "Aminoglycoside 3'-O-phosphotransferase III; transfers the gamma-"
        "phosphate of ATP onto the 3'-hydroxyl of kanamycin and neomycin, so the "
        "drug can no longer bind its ribosomal site. A Gram-positive member of "
        "the same APH(3') family as the Tn903 and Tn5 enzymes, and about as far "
        "from each of them as the two of them are from each other; distant "
        "enough that nucleotide matching will not cross between the three, close "
        "enough that calling it unrelated to them would be wrong.",
        why="ASSERTED as to the vector claim: the aphA-3 cassette used in "
            "Gram-positive and Campylobacter shuttle vectors, where Tn5 nptII "
            "expresses poorly. Record is Enterococcus faecalis. The relatedness "
            "clause is MEASURED, and it replaces two earlier statements that were "
            "both wrong: the description said this protein shares 'almost no "
            "sequence' with the other two, and this note asserted 'roughly 17% "
            "identical to aph(3')-IIa'. See the measured identities below.",
        homology=(("aph(3')-Ia", 25.0, 38.0), ("aph(3')-IIa", 25.0, 38.0)),
    ),
    Marker(
        "ant(9)-Ia", "SpcR", ("spc", "spec", "ANT(9)-Ia"),
        "Aminoglycoside 9-O-adenylyltransferase; adenylylates the 9-hydroxyl of "
        "spectinomycin, blocking the drug's interaction with the 30S subunit. A "
        "staphylococcal, spectinomycin-only enzyme. The integron AadA at "
        "PLF:0004 modifies spectinomycin at the same 9-hydroxyl and is a "
        "recognisable homologue of it; what separates the two is that AadA is "
        "bifunctional and also adenylylates streptomycin at the 3''-hydroxyl.",
        why="ASSERTED as to the vector claim, and the weakest justification in "
            "this list: the survey could not tie it to a named vector by "
            "measurement, only to the Gram-positive spc lineage. Record is "
            "Staphylococcus aureus, titled 'ant(9)-Ia gene for aminoglycoside "
            "nucleotidyltransferase ANT(9)-Ia'. First candidate to cut if the "
            "curator wants the list shorter. The relatedness clause is MEASURED "
            "and replaces an earlier 'unrelated in sequence to the integron aadA "
            "adenylyltransferases that inactivate the same drug at a different "
            "position', which was wrong on both halves.",
        homology=(("aadA1", 26.0, 42.0),),
    ),
    Marker(
        "catP", "CatP", ("catP", "cmR"),
        "Chloramphenicol acetyltransferase of the clostridial CatP type; "
        "acetylates chloramphenicol and thiamphenicol so that neither can bind "
        "the peptidyl transferase centre. A type A chloramphenicol "
        "acetyltransferase like CatA1 at PLF:0005, and clearly homologous to it; "
        "what distinguishes it is its clostridial origin and its activity "
        "against thiamphenicol, not membership of a different family.",
        why="ASSERTED as to the vector claim: the chloramphenicol/thiamphenicol "
            "marker of the modular Clostridium shuttle-vector series. The record "
            "is titled 'Clostridium perfringens CP590 catP gene for type A-11 "
            "chloramphenicol O-acetyltransferase CatP', and PLF:0005's is 'catA1 "
            "gene for type A-1 chloramphenicol O-acetyltransferase' -- A-1 and "
            "A-11 are subtypes of one family, which is why the earlier clause "
            "'a distinct protein family from the CatA enzymes rather than a "
            "variant of them' had to go. Relatedness MEASURED below.",
        homology=(("catA1", 35.0, 55.0),),
    ),
    Marker(
        "tet(B)", "TetA(B)", ("tetA(B)", "tetB", "Tn10 tet"),
        "Tetracycline/proton antiporter of the major facilitator superfamily; "
        "pumps the drug back across the inner membrane so it never accumulates "
        "at the ribosome. This is the efflux protein of the Tn10 tetracycline "
        "determinant, the same operon whose repressor and operator were the "
        "starting material for tetracycline-controlled expression systems.",
        why="ASSERTED, record-corroborated: the fetched record is titled "
            "'Transposon Tn10 tet(B)', so the transposon clause above rests on "
            "the record and not on recall.",
    ),
    Marker(
        "tet(C)", "TetA(C)", ("tetA(C)", "tetC", "TcR"),
        "Tetracycline efflux pump of the major facilitator superfamily, "
        "exchanging the drug for a proton. This is the tetracycline determinant "
        "of pBR322 and pACYC184, and a different protein from TetA(A) despite "
        "the shared mechanism and the shared vernacular name 'tetA'.",
        why="MEASURED, and a correction to what this database shipped: the "
            "survey found this protein byte-identical to the tetracycline "
            "product of fetched pBR322 and pACYC184 records. PLF:0006 (tet(A)) "
            "matches neither. The catalogue's own reference record happens to be "
            "Francisella tularensis, which is irrelevant to the identity.",
    ),
    Marker(
        "tet(K)", "TetK", ("tetK",),
        "Tetracycline efflux protein of the staphylococcal pT181 family; a "
        "major-facilitator antiporter that exports the drug in exchange for a "
        "proton. Encoded on a rolling-circle replicon rather than a transposon.",
        why="ASSERTED, record-corroborated: the record is 'Staphylococcus aureus "
            "pT181 tet(K)', and pT181 is the replicon behind a large family of "
            "staphylococcal cloning and shuttle vectors.",
    ),
    Marker(
        "tet(M)", "TetM", ("tetM",),
        "Ribosomal protection protein: a translational GTPase that binds the "
        "ribosome and dislodges bound tetracycline, restoring elongation. It "
        "confers resistance without touching the drug, which is why it works "
        "against tetracyclines that defeat the efflux pumps.",
        why="ASSERTED: the tetracycline marker of the conjugative transposons "
            "used for delivery and mutagenesis across Gram-positive genetics. "
            "Record is Enterococcus faecalis -- consistent with that lineage but "
            "not itself a transposon record, so the description does not name one.",
    ),
    Marker(
        "aac(3)-IVa", "AprR", ("aac(3)IV", "apr", "aacIV", "ApraR"),
        "Aminoglycoside 3-N-acetyltransferase IV; acetylates the 3-amino group "
        "of apramycin and gentamicin using acetyl-CoA, destroying the drug's "
        "affinity for the 30S subunit. Apramycin selection is the workhorse for "
        "actinomycete cloning, where the usual Gram-negative markers behave badly.",
        why="ASSERTED: the apramycin marker of the Streptomyces PCR-targeting "
            "cassettes and integrative vectors. NOTE the catalogue symbol is "
            "aac(3)-IVa; the commonly written 'aac(3)-IV' returns zero hits, "
            "which is how this row nearly went missing.",
    ),
    Marker(
        "ble-Sh", "ZeoR", ("Sh ble", "ble", "zeoR", "bleomycin binding protein"),
        "Bleomycin-family binding protein; confers resistance by binding the "
        "drug one-to-one and sequestering it, so protection is stoichiometric "
        "rather than catalytic and fails if the drug is in excess. The gene is "
        "from Streptoalloteichus hindustanus and is the resistance determinant "
        "used for phleomycin/bleomycin-family selection in bacteria, yeast and "
        "mammalian cells alike.",
        why="ASSERTED, record-corroborated decisively: the record is "
            "'Streptoalloteichus hindustanus ATCC 31158 ble-Sh', i.e. the "
            "genuine Sh ble. NOTE the catalogue symbol is ble-Sh; 'Sh ble' "
            "returns zero hits. This resolves half of SOURCING.md section 6's top open "
            "question in the affirmative.",
        patent="The selection drug is sold under a trademark and expression "
               "vectors carrying this gene are distributed under commercial "
               "licence. Flagged for counsel; not a determination, and the "
               "description above deliberately names the gene, not the product.",
    ),
    Marker(
        "aph(4)-Ia", "HygR", ("hph", "hpt", "hygB", "APH(4)-Ia"),
        "Hygromycin B 4-O-phosphotransferase; phosphorylates hygromycin B, which "
        "otherwise locks the ribosome and blocks translocation. The enterobacterial "
        "hygromycin marker, used in plant binary vectors and in mammalian "
        "selection cassettes.",
        why="ASSERTED, record-corroborated: the record is 'Escherichia coli K-12 "
            "aph(4)-Ia'. NOTE the catalogue symbol is aph(4)-Ia and the "
            "vernacular 'hph' returns zero hits -- which is precisely why "
            "SOURCING.md section 6 read this as an unresolved open question.",
        patent="Historical hygromycin-resistance expression-vector patents are "
               "widely believed long expired, but no patent record was fetched, "
               "so this flag records an unverified question rather than a clearance.",
    ),
    Marker(
        "aph(7'')-Ia", "HygR (Streptomyces)", ("hyg", "hygB", "APH(7'')-Ia"),
        "Hygromycin B 7''-O-phosphotransferase from Streptomyces hygroscopicus, "
        "the organism that makes the drug; it phosphorylates a different hydroxyl "
        "from the enterobacterial enzyme and shares little sequence with it. Used "
        "for hygromycin selection in actinomycete vectors.",
        why="ASSERTED, record-corroborated: record is 'Streptomyces hygroscopicus "
            "aph(7'')-Ia'. A genuinely different protein from aph(4)-Ia, so "
            "codon-blind matching against the E. coli hph would miss it entirely.",
    ),
    Marker(
        "erm(B)", "ErmB", ("ermB", "MLS", "EmR"),
        "23S rRNA adenine N-6 methyltransferase; dimethylates a single adenine "
        "in the peptide exit tunnel, which sterically excludes macrolides, "
        "lincosamides and streptogramin B at one stroke: the MLS phenotype. "
        "Resistance is to the ribosome, not to any one drug.",
        why="ASSERTED: the erythromycin marker of the enterococcal/lactococcal "
            "vector families and the retrotransposition-activated marker of "
            "group II intron mutagenesis systems. Record is Enterococcus hirae.",
    ),
    Marker(
        "erm(C)", "ErmC", ("ermC", "MLS", "EmR"),
        "23S rRNA adenine N-6 methyltransferase of the ErmC type; methylates the "
        "macrolide binding site, giving combined macrolide-lincosamide-"
        "streptogramin B resistance. Expression is inducible through a leader "
        "peptide, so the resistance appears only once the drug is present.",
        why="ASSERTED, record-corroborated decisively: the record is "
            "'Staphylococcus aureus pE194 erm(C)', and pE194 is exactly the "
            "plasmid the erythromycin cassettes of Bacillus and Staphylococcus "
            "shuttle and integration vectors are taken from.",
    ),
    Marker(
        "aph(3'')-Ib", "StrA", ("strA", "APH(3'')-Ib", "SmR"),
        "Aminoglycoside 3''-O-phosphotransferase; phosphorylates streptomycin at "
        "the 3'' position. Almost always found immediately beside aph(6)-Id, and "
        "the two together are what a map usually labels as one streptomycin marker.",
        why="ASSERTED, record-corroborated: the record is 'Escherichia coli "
            "RSF1010 aph(3'')-Ib'. RSF1010 is the IncQ replicon underlying the "
            "mobilisable broad-host-range vector families.",
    ),
    Marker(
        "aph(6)-Id", "StrB", ("strB", "APH(6)-Id", "SmR"),
        "Aminoglycoside 6-O-phosphotransferase; phosphorylates streptomycin at "
        "the 6 position, a second and independent inactivation of the same drug. "
        "The partner of aph(3'')-Ib on the same broad-host-range replicon; the "
        "pair is drawn as one block on most maps but they are two proteins and "
        "get two records here.",
        why="ASSERTED, record-corroborated: the record is 'Escherichia coli "
            "RSF1010 aph(6)-Id', the partner gene of the row above.",
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
    ordinal: int = 0
    """Position of this row in its stage's *declaration*, 1-based.

    Not its position in the output. `allocate()` turns it into a PLF id, so a
    row that fails verification leaves a gap rather than pulling every later id
    down by one. See the ORDER IS IDENTITY note on MARKERS.
    """


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

    Header layout is the one thing that silently breaks a reader of these two
    files: the allele symbol is field 4 of a protein header but field 5 of a CDS
    header, and protein field 5 is the gene *family*. Keying the protein side on
    the family index loses every allele whose family symbol differs from its own
    — blaTEM-1 among them, which then reads as absent rather than as mis-keyed.
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
            # AMRProt writes the terminal stop as `*`; our translation does too,
            # and `cds_matches_protein` strips it from both sides rather than
            # one — a mistake that looks like "nothing verifies" and reads like
            # a data problem.
            ok, how = cds_matches_protein(c["seq"], aa)
            if ok:
                verified.append(dict(c, how=how))

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

        # "53/53 entries passed" does NOT mean 53 copies of one sequence — every
        # catalogue entry for an allele is a distinct protein, and some families
        # are wide (tet(A) spans ~92.5% identity, aph(3')-Ia ~96.7%). The wording
        # below says "distinct" for exactly that reason: the earlier phrasing
        # invited a reader to think the tie-break was cosmetic when for a wide
        # family it is choosing one member out of several materially different ones.
        notes = (
            f"CDS verified by translation against {best['protein_acc']}: "
            f"{len(best['seq'])} nt -> {len(aa)} aa, NCBI table 11, {best['how']}. "
            f"{len(verified)}/{len(candidates)} distinct catalogue entries for this "
            f"allele passed the same check."
        )
        if m.note:
            notes += " " + m.note
        # `why` used to be source-only, on the reasoning that a MEASURED /
        # ASSERTED verdict belongs in the diff a reviewer reads rather than in a
        # description a user reads as settled fact. That was half right and the
        # wrong half was load-bearing: it left the curator who has to SIGN each
        # row unable to see which descriptions rest on a measurement, and it let
        # a `why` reading "that clause needs a citation or removal" sit in the
        # source for a full release while the clause shipped unqualified. The
        # verdict goes in `notes`, which is curator-facing, and stays out of
        # `description`, which is user-facing. stage_rfam does the same with its
        # `caveat`.
        if m.why:
            notes += " CURATOR (vector claim in the description above): " + m.why
        if m.patent:
            notes += " PATENT FLAG (not a determination): " + m.patent

        rows.append(
            Row(
                id="",  # assigned centrally by allocate(); see ordinal below
                ordinal=i,
                name=m.name,
                aliases=merge_aliases(m.aliases, m.allele),
                cls="cds",
                genbank_key="CDS",
                reference_nt=best["seq"],
                reference_aa=aa,
                boundary_rule="orf_atg_to_stop",
                boundary_evidence=f"{acc}:{start}-{end}:+",
                description=m.description,
                notes=notes,
                patent_flag="1" if m.patent else "0",
                # One row per field, each pointing at the file the bytes
                # actually came from. Attributing all three to AMR_CDS.fa
                # stamped that file's sha256 on the amino acids, which live in
                # AMRProt.fa -- so a reviewer doing exactly what features/NOTICE
                # promises would verify the hash and then fail to find the
                # protein in the file. `aliases` are hand-written in MARKERS
                # above, so they are our work, like `name` below.
                #
                # No record id here: allocate() prefixes the assigned PLF id, so
                # a stage cannot bind provenance to an id it invented locally.
                provenance=[
                    ("reference_nt", "amrfinderplus", best["protein_acc"],
                     "INSDC-free", f"{AMR_BASE}/AMR_CDS.fa",
                     cds_meta.get("retrieved", TODAY), cds_meta.get("sha256", "")),
                    ("reference_aa", "amrfinderplus", best["protein_acc"],
                     "INSDC-free", f"{AMR_BASE}/AMRProt.fa",
                     prot_meta.get("retrieved", TODAY), prot_meta.get("sha256", "")),
                    ("boundary_evidence", "amrfinderplus", best["nucl_acc"],
                     "INSDC-free", f"{AMR_BASE}/AMR_CDS.fa",
                     cds_meta.get("retrieved", TODAY), cds_meta.get("sha256", "")),
                    ("aliases", "polylinker", "-", "own-work", "-", TODAY, ""),
                    ("boundary_rule", "polylinker", "-", "own-work", "-", TODAY, ""),
                    ("description", "polylinker", "-", "own-work", "-", TODAY, ""),
                    ("name", "polylinker", "-", "own-work", "-", TODAY, ""),
                ],
            )
        )
        report.append(
            f"  OK   {m.allele:14s} -> {m.name:20s} {len(best['seq']):5d} nt  "
            f"{acc}:{start}-{end}  ({len(verified)}/{len(candidates)} verified)"
        )

    # Second pass: measure the relatedness the descriptions assert. It has to be
    # a second pass because a marker may name a sibling declared after it, and
    # both proteins have to exist before either can be aligned.
    protein_of = {MARKERS[r.ordinal - 1].allele: r.reference_aa for r in rows}
    id_of = {MARKERS[r.ordinal - 1].allele: r.ordinal for r in rows}
    for r in rows:
        m = MARKERS[r.ordinal - 1]
        for other, lo, hi in m.homology:
            aa = protein_of.get(other)
            if aa is None:
                raise SystemExit(
                    f"{m.allele}: declares homology to {other!r}, which produced no "
                    f"row this build, so the claim in its description is unmeasured"
                )
            ident, alen, pct = percent_identity(r.reference_aa, aa)
            if not (lo <= pct <= hi):
                raise SystemExit(
                    f"{m.allele}: measured {pct:.1f}% identity to {other} "
                    f"({ident}/{alen}), outside the declared {lo}-{hi}% band. Either "
                    f"the sequences moved or the description's claim about them is "
                    f"wrong; both are reasons to stop, not to widen the band."
                )
            r.notes += (
                f" MEASURED: {pct:.1f}% amino-acid identity to {other} "
                f"(PLF:{id_of[other]:04d}), {ident}/{alen} aligned positions, "
                f"global alignment under BLOSUM62 with affine gaps."
            )
    return rows, report


# --------------------------------------------------------------------------
# Stages, and the id space each one owns


@dataclass(frozen=True)
class Stage:
    key: str
    module: str | None
    """Module to import, or None for the stage defined in this file."""
    attr: str
    base: int
    """First PLF number reserved for this stage."""
    size: int
    """How many numbers are reserved. Reserved by DECLARATION, not by output."""
    title: str


# Disjoint, permanently reserved blocks. This is the mechanism that makes "add a
# stage" a safe operation: stage N+1 draws from numbers stage N can never reach,
# so no row's identity depends on how many rows any other stage produced. A
# stage that yields far fewer rows than its block leaves the rest of the block
# empty forever, and that waste is the point — the alternative is packing ids
# tightly and renumbering the world every time an allow-list grows.
#
# Widening a block is fine. MOVING a base is not: it renames every row the stage
# has ever published.
STAGES: tuple[Stage, ...] = (
    Stage("amrfinder", None, "stage_amrfinder", 1, 999,
          "AMRFinderPlus resistance and selection markers"),
    Stage("uniprot", "stage_uniprot", "build", 1000, 1000,
          "UniProt -> ENA natural proteins"),
    Stage("rfam", "stage_rfam", "build", 2000, 1000,
          "Rfam structured RNA elements"),
    Stage("curated", "stage_curated", "build", 3000, 1000,
          "Hand-curated tags, linkers, protease sites, 2A"),
)

VALID_CLASSES = {"cds", "regulatory", "origin", "repeat", "synthetic_part", "misc"}
VALID_RULES = {
    "orf_atg_to_stop",
    "orf_mature_peptide",
    "literature_defined",
    "consensus_of_insdc",
    "designed_sequence",
}
# Mirrors crates/pl-features/src/lib.rs. Checking here rather than only there
# means a bad row is named by the tool that produced it, at the moment it is
# produced, instead of surfacing as an anonymous parse error in Rust.
NT_ALPHABET = set("ACGTRYSWKMBDHVN")


def load_stage(stage: Stage):
    """Resolve a stage's build function, or None with a printed reason.

    Deliberately catches every exception, not just ImportError. These stage
    modules are written independently and concurrently; a half-saved file raises
    SyntaxError, a missing dependency raises ModuleNotFoundError, and a
    module-level fetch raises whatever the network felt like. None of those is a
    reason to throw away the three stages that *are* ready — but every one of
    them is reported and makes the build exit non-zero, because a stage that
    silently contributes nothing is indistinguishable from a stage that has no
    rows to contribute.
    """
    if stage.module is None:
        return globals()[stage.attr]
    try:
        mod = importlib.import_module(stage.module)
    except ImportError as e:
        print(f"  (absent: {stage.module} — {e})")
        return None
    except Exception as e:  # noqa: BLE001 — see docstring
        print(f"  !! {stage.module} failed to import: {e.__class__.__name__}: {e}")
        return None
    fn = getattr(mod, stage.attr, None)
    if not callable(fn):
        print(f"  !! {stage.module} has no callable {stage.attr}(refresh)")
        return None

    # The stage modules were written in parallel against this harness's *prose*
    # description of the id contract rather than against its code, and two of
    # three got the base wrong: stage_uniprot numbered from 101 and stage_rfam
    # from 300, while STAGES reserves 1000 and 2000. allocate() reads those local
    # numbers as ordinals, so the build stayed internally consistent and simply
    # issued PLF:1100 as the first UniProt row and PLF:2299 as the *first* Rfam
    # row — deterministic, stable, and silently wrong about which block each row
    # sits in. Nothing warned, because nothing compared the two numbers.
    #
    # So compare them. A stage that names its block must name the block this file
    # reserved for it or it does not run, because the failure mode is a permanent
    # id whose number says something other than what the id means.
    #
    # MANDATORY, not opt-in. `if base is not None` made the check something a
    # stage escaped by saying nothing, and the stage most likely to reintroduce
    # the bug is exactly the one that says nothing: a new module whose author did
    # not know the constants were expected. Deleting both constants from
    # stage_rfam and restoring its old local `300` reproduced the original bug
    # with no warning and exit 0.
    base = getattr(mod, "PLF_BLOCK_BASE", None)
    size = getattr(mod, "PLF_BLOCK_SIZE", None)
    if base is None or size is None:
        print(f"  !! {stage.module} does not declare PLF_BLOCK_BASE/PLF_BLOCK_SIZE. "
              f"Every stage must name the id block it was reserved "
              f"(PLF_BLOCK_BASE = {stage.base}, PLF_BLOCK_SIZE = {stage.size}), so "
              f"that this file can refuse a stage that disagrees with it")
        return None
    if base != stage.base:
        print(f"  !! {stage.module} declares PLF_BLOCK_BASE={base}, but STAGES "
              f"reserves {stage.base} for it; refusing to run a stage that "
              f"disagrees with the allocator about its own id space")
        return None
    if size != stage.size:
        print(f"  !! {stage.module} declares PLF_BLOCK_SIZE={size}, but STAGES "
              f"reserves {stage.size} for it")
        return None
    return fn


def _get(obj, *names, default=None):
    """Read a field from a Row, a look-alike object, or a dict."""
    for n in names:
        if isinstance(obj, dict):
            if n in obj:
                return obj[n]
        elif hasattr(obj, n):
            return getattr(obj, n)
    return default


def coerce_row(obj) -> Row:
    """Accept a Row, an object that quacks like one, or a dict.

    Raises ValueError with the reason if the object cannot be a feature row.

    Tolerant about *shape* and unmovable about *content*. A stage written
    against a slightly different Row definition is not a reason to lose forty
    verified rows; a stage that hands back a row already marked `reviewed`, or
    with a curator's name on it, is a reason to stop, because that is the one
    rule this project cannot bend (§8.3 rule 6, and the loader enforces the
    other half at crates/pl-features/src/lib.rs).
    """
    status = str(_get(obj, "review_status", default="proposed") or "proposed").strip().lower()
    curator = str(_get(obj, "curator", default="") or "").strip()
    if status != "proposed":
        raise ValueError(f"stage emitted review_status={status!r}; AI may propose, never assert")
    if curator:
        raise ValueError(f"stage emitted curator={curator!r}; only a human may sign a row")

    if isinstance(obj, Row):
        return obj

    name = str(_get(obj, "name", default="") or "")
    cls = str(_get(obj, "cls", "class", "class_", default="") or "").strip().lower()
    row = Row(
        id=str(_get(obj, "id", default="") or ""),
        ordinal=int(_get(obj, "ordinal", default=0) or 0),
        name=name,
        aliases=list(_get(obj, "aliases", default=[]) or []),
        cls=cls,
        genbank_key=str(_get(obj, "genbank_key", default="") or "misc_feature"),
        reference_nt=str(_get(obj, "reference_nt", default="") or "").upper(),
        reference_aa=str(_get(obj, "reference_aa", default="") or "").upper(),
        boundary_rule=str(_get(obj, "boundary_rule", default="") or "").strip().lower(),
        boundary_evidence=str(_get(obj, "boundary_evidence", default="") or ""),
        description=str(_get(obj, "description", default="") or ""),
        notes=str(_get(obj, "notes", default="") or ""),
        patent_flag=str(_get(obj, "patent_flag", default="0") or "0").strip(),
        provenance=list(_get(obj, "provenance", default=[]) or []),
    )
    if row.patent_flag.lower() in ("true", "yes"):
        row.patent_flag = "1"
    if row.patent_flag.lower() in ("false", "no", ""):
        row.patent_flag = "0"
    return row


def validate_row(r: Row) -> str:
    """Return "" if the row is loadable, else the reason it is not.

    Every clause here mirrors a refusal in crates/pl-features/src/lib.rs. The
    point is not to duplicate the loader but to fail at the point of production,
    naming the stage and the record, rather than shipping a features.tsv that
    the Rust reader rejects wholesale with no idea who wrote the row.
    """
    if not r.name:
        return "name is empty"
    if r.cls not in VALID_CLASSES:
        return f"class {r.cls!r} is not one of {sorted(VALID_CLASSES)}"
    if r.boundary_rule not in VALID_RULES:
        return f"boundary_rule {r.boundary_rule!r} is not one of {sorted(VALID_RULES)}"
    if not r.boundary_evidence:
        return "boundary_evidence is empty: a boundary with no evidence is what this database replaces"
    if not r.reference_nt:
        # Not a nit. The reader requires nucleotides on every record, so a row
        # that carries only a protein cannot be loaded at all — it is a schema
        # conversation to have before the row is written, not a warning to skip.
        return "reference_nt is empty; the loader requires nucleotides on every record"
    bad = sorted(set(r.reference_nt) - NT_ALPHABET)
    if bad:
        return f"reference_nt contains non-nucleotide code(s) {bad}"
    if r.reference_aa and r.cls != "cds":
        return f"class {r.cls} carries a protein reference; only cds may"
    if r.patent_flag not in ("0", "1"):
        return f"patent_flag {r.patent_flag!r} is not 0 or 1"
    if not r.provenance:
        return "no provenance: an unsourced row is the thing this database exists to replace"

    # ASCII, on every field we author. See assert_ascii().
    for field, text in (
        ("name", r.name), ("aliases", "|".join(r.aliases)), ("class", r.cls),
        ("genbank_key", r.genbank_key), ("boundary_rule", r.boundary_rule),
        ("boundary_evidence", r.boundary_evidence),
        ("description", r.description), ("notes", r.notes),
    ):
        why = assert_ascii(r.id, field, text)
        if why:
            return why

    # The one expression tell SOURCING.md Risk 1 names by measurement. Their
    # Description column is human-written editorial prose in which "et" and "al"
    # each occur 392 times, i.e. inline literature citations, and Risk 1's
    # conclusion is that this is the copyrightable layer to stay away from. Our
    # descriptions never used it; eight `boundary_evidence` fields did, in
    # exactly that "(Author et al. Year, Journal vol:pages)" form, while the
    # `notes` on the same rows already carried full author lists. The
    # information was duplicated in the safe form, so only the tell was lost.
    for field, text in (("description", r.description),
                        ("boundary_evidence", r.boundary_evidence)):
        if re.search(r"\bet\s+al\b", text, re.I):
            return (
                f"{field} uses the 'et al.' citation construction. Write the citation "
                f"as PMID/DOI plus journal, volume and pages, and put the author list "
                f"in notes -- see SOURCING.md Risk 1 on why this particular habit is "
                f"the one to avoid."
            )

    # Source and licence, against SOURCING.md §1.
    bad_prov = check_provenance(r.id, r.provenance)
    if bad_prov:
        return bad_prov[0]

    # Per-field coverage. features/NOTICE promises "which source each individual
    # field came from and under what licence" for every field of every row, and
    # the only thing enforcing any of it was `Db::audit`'s check that
    # `reference_nt` had at least one row. Measured against that promise, four
    # populated columns had no provenance at all — including `genbank_key`, the
    # one column SOURCING.md Risk 4 flags as legally unresolved.
    covered = {p[1] for p in r.provenance}
    populated = {
        "name": r.name, "aliases": "|".join(r.aliases), "class": r.cls,
        "genbank_key": r.genbank_key, "reference_nt": r.reference_nt,
        "reference_aa": r.reference_aa, "boundary_rule": r.boundary_rule,
        "boundary_evidence": r.boundary_evidence, "description": r.description,
        "patent_flag": r.patent_flag, "notes": r.notes,
    }
    missing = sorted(
        f for f, v in populated.items()
        if v and f not in covered and f not in PROVENANCE_EXEMPT
    )
    if missing:
        return (
            f"populated field(s) {missing} carry no provenance row, so features/NOTICE's "
            f"per-field promise is false for this record"
        )
    return ""


def fill_structural_provenance(rid: str, r: Row) -> None:
    """Attribute the columns that are this project's own editorial layer.

    Only fills a field NO stage claimed. A stage that knows better — stage_rfam
    takes `class` from Rfam's own type column, and its `notes` carry Rfam's
    bibliographic text — says so itself and is left alone.

    `genbank_key` is the awkward one and is stated awkwardly on purpose. The
    values are INSDC feature keys, which come from the INSDC feature-table
    specification, whose licence SOURCING.md Risk 4 records as [UNVERIFIED]
    while routing the column away from Sequence Ontology precisely because SO's
    own licence is contested. Writing `polylinker / own-work` there would be a
    claim we cannot make; writing nothing left the single field the sourcing
    document flags as unresolved as the single field with no provenance at all.
    """
    covered = {p[1] for p in r.provenance}
    add = []
    if "class" not in covered:
        add.append((rid, "class", "polylinker", "-", "own-work", "-", TODAY, ""))
    if "genbank_key" not in covered and r.genbank_key:
        add.append((rid, "genbank_key", "insdc-ft", r.genbank_key,
                    "unresolved-see-SOURCING-Risk-4",
                    "https://www.insdc.org/submitting-standards/feature-table/",
                    TODAY, ""))
    if "patent_flag" not in covered:
        # Ours, and a flag rather than a determination: no patent database was
        # searched by anything in this repository (SOURCING.md Risk 6).
        add.append((rid, "patent_flag", "polylinker", "-", "own-work", "-", TODAY, ""))
    if "notes" not in covered and r.notes:
        add.append((rid, "notes", "polylinker", "-", "own-work", "-", TODAY, ""))
    r.provenance.extend(add)


def allocate(stage: Stage, raw_rows, loose_prov, defects: list) -> list:
    """Assign PLF ids from `stage`'s reserved block, deterministically.

    The id comes from the row's *ordinal* — its position in the stage's
    declaration — never from its position in this run's output. Preference order,
    most stable first:

      1. an explicit `ordinal` attribute (the contract stages should use);
      2. the digits of a locally-assigned `PLF:NNNN` id — a stage that copied
         stage_amrfinder's old `PLF:{i:04d}` idiom is numbering by its own
         declaration index, which is exactly what we want, just relocated;
      3. position in the returned list, which IS outcome-dependent and is
         therefore reported as a defect rather than accepted quietly.

    Collisions and block overflow are refused, not resolved. Two rows claiming
    one id, or a row spilling into the next stage's block, would both end in one
    accession answering to two names.
    """
    rows, taken = [], {}
    for pos, obj in enumerate(raw_rows, start=1):
        try:
            r = coerce_row(obj)
        except ValueError as e:
            defects.append(f"{stage.key}: row {pos} rejected — {e}")
            continue

        local_id = r.id
        if r.ordinal:
            ordinal = r.ordinal
        elif re.fullmatch(r"PLF:\d+", local_id or ""):
            ordinal = int(local_id.split(":")[1])
        else:
            ordinal = pos
            defects.append(
                f"{stage.key}: {r.name!r} has no ordinal and no PLF: id, so its id was "
                f"taken from output position {pos}. That is stable only while the stage "
                f"drops nothing — give the row an `ordinal` from its allow-list index."
            )

        n = stage.base + ordinal - 1
        if not (stage.base <= n < stage.base + stage.size):
            defects.append(
                f"{stage.key}: {r.name!r} ordinal {ordinal} lands on PLF:{n:04d}, outside "
                f"the block {stage.base}..{stage.base + stage.size - 1}. Widen the block "
                f"deliberately; do not let it spill into the next stage's namespace."
            )
            continue
        rid = f"PLF:{n:04d}"
        if rid in taken:
            defects.append(
                f"{stage.key}: {r.name!r} and {taken[rid]!r} both claim {rid}; "
                f"duplicate ordinal {ordinal}"
            )
            continue
        taken[rid] = r.name

        r.provenance = bind_provenance(rid, r.provenance, stage, defects)
        # A stage may return provenance separately from its rows, keyed on the id
        # it used locally. Re-key it onto the id we assigned.
        for p in loose_prov.pop(local_id, []) if local_id else []:
            r.provenance.extend(bind_provenance(rid, [p], stage, defects))
        r.id = rid
        fill_structural_provenance(rid, r)

        why = validate_row(r)
        if why:
            defects.append(f"{stage.key}: {rid} {r.name!r} rejected — {why}")
            continue
        rows.append(r)
    return rows


def bind_provenance(rid: str, raw, stage: Stage, defects: list) -> list:
    """Stamp `rid` onto provenance tuples, whichever shape the stage used."""
    out = []
    for p in raw:
        t = tuple(str(x) for x in p)
        if len(t) == len(PROVENANCE_COLUMNS):
            t = (rid,) + t[1:]           # carried a record id; ours wins
        elif len(t) == len(PROVENANCE_COLUMNS) - 1:
            t = (rid,) + t               # omitted the record id; supply it
        else:
            defects.append(
                f"{stage.key}: {rid} provenance row has {len(t)} fields, expected "
                f"{len(PROVENANCE_COLUMNS)} or {len(PROVENANCE_COLUMNS) - 1}: {t}"
            )
            continue
        out.append(t)
    return out


def self_test() -> list:
    """Run every gate in this file against input that must trip it.

    None of these needs the network or the cache, so they run on every build.
    They exist because each one is otherwise unfalsifiable in a green build: the
    real rows all pass, so "no error" is equally consistent with "the check does
    nothing", and this project has already shipped one gate — the translation
    check — whose stated strength was greater than its real one.

    The nucleotide strings below are synthetic test fixtures, not references to
    any organism, which is why they may appear literally here and nowhere in a
    row.
    """
    out, fails = [], []

    def want(label: str, cond: bool) -> None:
        (out if cond else fails).append(
            f"  {'PASS' if cond else 'FAIL'} {label}"
        )

    # 1. The translation gate. An initiator-only difference is accepted and
    #    *reported*; a difference one residue further in is not excusable by any
    #    initiation rule and must be refused. That second case is the property
    #    the old translate_cds gate did not have and was described as having.
    ok, how = cds_matches_protein("ATGAAATAA", "MK")
    want("exact whole-CDS match accepted", ok and "residue for residue" in how)
    ok, how = cds_matches_protein("GTGAAATAA", "MK")
    want("alternative initiator accepted, and named in the reason",
         ok and "GTG" in how and "residue 1" in how)
    ok, _ = cds_matches_protein("GTGGGGTAA", "MK")
    want("a difference at residue 2 refused despite a valid initiator", not ok)
    ok, _ = cds_matches_protein("GTGAAATAA", "AK")
    want("initiator rule applies only when the protein really starts Met", not ok)
    ok, how = cds_matches_protein("GTGAAATAA", "VK")
    want("a GTG-start protein annotated as Val is still an exact match",
         ok and "residue for residue" in how)

    # 2. ASCII, on the exact characters that shipped: 16 em dashes and one
    #    section sign. Written as escapes rather than as the glyphs, because a
    #    source-wide "replace the em dashes" pass would otherwise disarm this
    #    test by fixing its own fixture — which is exactly what happened once.
    want("em dash refused in an authored field",
         bool(assert_ascii("PLF:0000", "description", "a — b")))
    want("section sign refused in an authored field",
         bool(assert_ascii("PLF:0000", "notes", "SOURCING.md § 6")))
    want("plain ASCII accepted", not assert_ascii("PLF:0000", "description", "a - b"))

    # 3. Alias hygiene.
    want("aliases de-duplicated case-insensitively",
         merge_aliases(("catP", "cmR"), "catP") == ["catP", "cmR"]
         and merge_aliases(("ANT(9)-Ia",), "ant(9)-Ia") == ["ANT(9)-Ia"])

    # 4. The NO_GO list, as the two rows that were planted into a copy of the
    #    shipped table by hand and passed every checker in the project.
    want("an Addgene-sourced provenance row refused", bool(check_provenance(
        "PLF:0001", [("PLF:0001", "description", "addgene", "Addgene-52961",
                      "noncommercial-informational-only",
                      "https://www.addgene.org/52961/", TODAY, "deadbeef")])))
    want("a PlasMapper-sourced provenance row refused", bool(check_provenance(
        "PLF:0001", [("PLF:0001", "reference_nt", "plasmapper", "FeatureDB",
                      "GPL-3.0-scraped", "https://plasmapper.ca/featuredb",
                      TODAY, "cafebabe")])))
    want("a cleared source under the wrong licence refused", bool(check_provenance(
        "PLF:0001", [("PLF:0001", "name", "uniprot", "P00000", "CC0-1.0",
                      "https://rest.uniprot.org/", TODAY, "beef")])))
    want("a provenance field that is not a column refused", bool(check_provenance(
        "PLF:0001", [("PLF:0001", "citation", "rfam", "RF00000", "CC0-1.0",
                      "https://ftp.ebi.ac.uk/", TODAY, "beef")])))
    want("a well-formed provenance row accepted", not check_provenance(
        "PLF:0001", [("PLF:0001", "name", "polylinker", "-", "own-work", "-",
                      TODAY, "")]))

    # 5. The fetch host allow-list, on the three hosts SOURCING.md forbids by
    #    name and the one it clears.
    for host_url in ("https://www.addgene.org/52961/",
                     "https://plasmapper.ca/featuredb",
                     "https://raw.githubusercontent.com/x/y/snapgene.csv"):
        try:
            check_fetch_host(host_url)
            fails.append(f"  FAIL fetch host {host_url} was allowed")
        except SystemExit:
            out.append(f"  PASS fetch host refused: {host_url}")
    try:
        check_fetch_host(f"{AMR_BASE}/AMR_CDS.fa")
        out.append("  PASS fetch host allowed: the AMRFinderPlus catalogue")
    except SystemExit:
        fails.append("  FAIL a cleared host was refused")

    # 6. Per-field provenance coverage, which features/NOTICE promises and
    #    nothing enforced.
    bare = Row(
        id="PLF:0000", ordinal=1, name="x", aliases=[], cls="cds",
        genbank_key="CDS", reference_nt="ATGTAA", reference_aa="M",
        boundary_rule="orf_atg_to_stop", boundary_evidence="X.1:1-6:+",
        description="d", notes="", patent_flag="0",
        provenance=[("PLF:0000", "reference_nt", "polylinker", "-", "own-work",
                     "-", TODAY, "")],
    )
    want("a populated field with no provenance refused",
         "carry no provenance row" in validate_row(bare))
    fill_structural_provenance("PLF:0000", bare)
    covered = {p[1] for p in bare.provenance}
    want("structural fill covers class/genbank_key/patent_flag",
         {"class", "genbank_key", "patent_flag"} <= covered)
    want("structural fill records genbank_key's licence as unresolved",
         any(p[1] == "genbank_key" and p[4] == "unresolved-see-SOURCING-Risk-4"
             for p in bare.provenance))

    if fails:
        for f in fails:
            print(f)
        raise SystemExit(f"SELF-TEST FAILED: {len(fails)} gate(s) did not behave")
    return out


def split_second_return(second):
    """A stage's second return value is either a report or a provenance table.

    stage_amrfinder has always returned `(rows, report_lines)`; the stage
    contract as written elsewhere says `(rows, provenance)`. Both exist in this
    build, so sniff rather than assume — guessing wrong either prints tuples as
    if they were log lines or drops a whole stage's provenance on the floor,
    and the second failure is silent.
    """
    report, prov = [], {}
    for item in second or []:
        if isinstance(item, str):
            report.append(item)
        elif isinstance(item, (tuple, list)) and item:
            prov.setdefault(str(item[0]), []).append(item)
    return report, prov


# --------------------------------------------------------------------------
# The id-stability audit


def read_previous(path: Path) -> dict | None:
    """Read the features.tsv this build is about to overwrite, if any."""
    if not path.exists():
        return None
    prev = {}
    for line in path.read_text(encoding="utf8").splitlines():
        if not line or line.startswith("#"):
            continue
        f = line.split("\t")
        if len(f) < len(FEATURE_COLUMNS) or f[0] == "id":
            continue
        prev[f[0]] = {"name": f[1], "nt": f[5]}
    return prev


def audit_ids(prev: dict | None, rows: list) -> tuple[list, list]:
    """Compare the ids we are about to write against the ones already published.

    Returns (fatal, warnings). A PLF id is a permanent name. If the sequence
    under an existing id changes, every citation of that id now points at
    something else and nothing anywhere says so — which is precisely the failure
    the reserved-block scheme exists to prevent, so this check is what proves
    the scheme worked rather than merely looking like it did.

    A name change is a warning, not a failure: renaming a record is a legitimate
    curatorial edit and does not repoint the sequence. A *missing* id is fatal
    for the same reason a changed sequence is — the id has stopped meaning what
    it meant, it has just stopped meaning anything at all.

    KNOW WHAT THE BASELINE IS. By default `prev` is the file this run is about
    to overwrite, which is the *published* table only on a clean checkout. Run
    the build twice locally and the second run audits the output against itself:
    green, and proving nothing. Pass `--baseline` (or `PLF_BASELINE`) pointing
    at the released table when that distinction matters — a rebuild that
    deliberately re-pins an unpublished row is exactly when it does.
    """
    if prev is None:
        return [], ["no previous features.tsv, so id stability could not be checked "
                    "this run -- that is an unchecked assumption, not a pass"]
    fatal, warn = [], []
    now = {r.id: r for r in rows}
    for rid, old in sorted(prev.items()):
        new = now.get(rid)
        if new is None:
            fatal.append(f"{rid} ({old['name']}) was published and is now absent")
            continue
        if new.reference_nt != old["nt"]:
            fatal.append(
                f"{rid} changed sequence: {old['name']!r} ({len(old['nt'])} nt) -> "
                f"{new.name!r} ({len(new.reference_nt)} nt). An id is a permanent name."
            )
        elif new.name != old["name"]:
            warn.append(f"{rid} renamed {old['name']!r} -> {new.name!r} (sequence unchanged)")
    return fatal, warn


# --------------------------------------------------------------------------


def write_outputs(out: Path, rows: list) -> int:
    out.mkdir(parents=True, exist_ok=True)
    with (out / "features.tsv").open("w", encoding="utf8", newline="\n") as fh:
        fh.write(f"#!version {TODAY.replace('-', '.')}\n")
        fh.write(
            "# Generated by features/build/build.py. Every row is 'proposed':\n"
            "# machine-extracted, no human has signed off, NOT shippable.\n"
            "# IDs are allocated from per-stage reserved blocks and never move.\n"
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

    n = 0
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
                if p[4] not in UNFETCHED_LICENCES and not p[7]:
                    raise SystemExit(
                        f"{p[0]} field {p[1]}: cites {p[2]} with no sha256 — the "
                        f"source cache is unverified, refusing to write"
                    )
                fh.write("\t".join(str(x) for x in p) + "\n")
                n += 1
    return n


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--refresh", action="store_true", help="re-fetch every source")
    ap.add_argument("--out", default=str(ROOT), help="output directory")
    ap.add_argument(
        "--allow-id-drift",
        action="store_true",
        help="write even if a published PLF id changed meaning (it should not; "
             "if you need this flag, say why in the commit message)",
    )
    ap.add_argument(
        "--baseline",
        default=os.environ.get("PLF_BASELINE"),
        help="features.tsv to audit ids against. Defaults to the file about to be "
             "overwritten, which is the PUBLISHED table only on a clean checkout: "
             "after one local build the audit compares the output against itself "
             "and cannot fail. Point this at the released table to keep it honest.",
    )
    args = ap.parse_args()

    out = Path(args.out)
    print("polylinker-features build")
    print(f"  date {TODAY}")

    print("\nSelf-test -- every gate in build.py, against input that must trip it")
    print("\n".join(self_test()))

    rows, defects = [], []
    for n, stage in enumerate(STAGES, start=1):
        print(f"\nStage {n} — {stage.title}  [PLF:{stage.base:04d}"
              f"..PLF:{stage.base + stage.size - 1:04d}]")
        fn = load_stage(stage)
        if fn is None:
            if stage.module is not None:
                defects.append(f"{stage.key}: stage unavailable, contributed no rows")
            continue
        try:
            raw_rows, second = fn(args.refresh)
        except SystemExit:
            raise
        except Exception:  # noqa: BLE001 — one stage's failure must not erase the rest
            print(f"  !! {stage.key} raised while building:")
            print("".join("     " + ln for ln in traceback.format_exc().splitlines(True)))
            defects.append(f"{stage.key}: raised while building, contributed no rows")
            continue

        report, loose = split_second_return(second)
        staged = allocate(stage, raw_rows, loose, defects)
        for leftover in sorted(loose):
            defects.append(
                f"{stage.key}: provenance keyed on {leftover!r} matched no row and was dropped"
            )
        if report:
            print("\n".join(report))
        print(f"  {len(staged)} row(s) from this stage")
        rows.extend(staged)

    rows.sort(key=lambda r: int(r.id.split(":")[1]))

    baseline = Path(args.baseline) if args.baseline else (out / "features.tsv")
    prev = read_previous(baseline)
    fatal, warn = audit_ids(prev, rows)
    print(f"\nID stability audit  [baseline: {baseline}]")
    for w in warn:
        print(f"  warn  {w}")
    for f in fatal:
        print(f"  FATAL {f}")
    if not warn and not fatal:
        print(f"  {len(prev or {})} previously published id(s) still mean the same sequence")
    if fatal and not args.allow_id_drift:
        print("\nRefusing to write. A published id now means a different sequence, and")
        print("nothing downstream would be told. Fix the allocation, or pass")
        print("--allow-id-drift if the change is deliberate and documented.")
        return 2

    if defects:
        print(f"\n{len(defects)} defect(s):")
        for d in defects:
            print(f"  - {d}")

    n_prov = write_outputs(out, rows)

    print(f"\nwrote {len(rows)} records to {out / 'features.tsv'}")
    print(f"      {n_prov} provenance rows")
    print("\nAll rows are 'proposed'. Db::reviewed() will ship none of them until")
    print("a curator signs each one off. That is the intended state.")
    if defects:
        print(f"\nexit 1: {len(defects)} defect(s) above. features.tsv holds only the")
        print("rows that passed, so it loads -- but this build is incomplete.")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
