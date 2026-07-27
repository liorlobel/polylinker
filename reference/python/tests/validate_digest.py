"""Cross-validate our restriction digest against Biopython's Bio.Restriction.

Three implementations, on real plasmids, over the same enzyme set: the Python
transcription below, the shipped Rust binary, and Biopython. Any disagreement
is a bug in ours until proven otherwise.

This is the QA pattern the whole project needs: never trust a hand-rolled
biology routine that has not been checked against an established one.

    python validate_digest.py [pl.exe] '<glob>' ...

**This script used to be unable to fail.** It exited 0 when it compared zero
files, and it exited 0 when it found mismatches — it printed them and returned
success. It also compared only the Python transcription, so it said nothing at
all about the Rust that users actually run; wiring it into the gate as a guard
on a Rust refactor would have added a step that was green by construction.
That is the same shape as the bench step that once reported `ok` for a score of
zero. All three are fixed: mismatches and empty runs both exit 1, and if the
first argument is an executable its answers are compared too.
"""
import sys
import os
import glob
import json
import subprocess

# snapdna lives one level up, in reference/python/
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import snapdna  # noqa: E402

from Bio.Seq import Seq                       # noqa: E402
from Bio import Restriction                   # noqa: E402

# The same set the browser prototype ships, transcribed independently.
OUR_ENZYMES = [
    ("AatII","GACGTC",5), ("AflII","CTTAAG",1), ("AgeI","ACCGGT",1),
    ("ApaI","GGGCCC",5), ("AscI","GGCGCGCC",2), ("AvrII","CCTAGG",1),
    ("BamHI","GGATCC",1), ("BclI","TGATCA",1), ("BglII","AGATCT",1),
    ("BsiWI","CGTACG",1), ("BspEI","TCCGGA",1), ("BsrGI","TGTACA",1),
    ("BstBI","TTCGAA",2), ("ClaI","ATCGAT",2), ("DraI","TTTAAA",3),
    ("EagI","CGGCCG",1), ("EcoRI","GAATTC",1), ("EcoRV","GATATC",3),
    ("FseI","GGCCGGCC",6), ("HindIII","AAGCTT",1), ("HpaI","GTTAAC",3),
    ("KpnI","GGTACC",5), ("MfeI","CAATTG",1), ("MluI","ACGCGT",1),
    ("NcoI","CCATGG",1), ("NdeI","CATATG",2), ("NheI","GCTAGC",1),
    ("NotI","GCGGCCGC",2), ("NruI","TCGCGA",3), ("NsiI","ATGCAT",5),
    ("PacI","TTAATTAA",5), ("PmeI","GTTTAAAC",4), ("PstI","CTGCAG",5),
    ("PvuI","CGATCG",4), ("PvuII","CAGCTG",3), ("SacI","GAGCTC",5),
    ("SacII","CCGCGG",4), ("SalI","GTCGAC",1), ("SbfI","CCTGCAGG",6),
    ("ScaI","AGTACT",3), ("SmaI","CCCGGG",3), ("SnaBI","TACGTA",3),
    ("SpeI","ACTAGT",1), ("SphI","GCATGC",5), ("SspI","AATATT",3),
    ("StuI","AGGCCT",3), ("SwaI","ATTTAAAT",4), ("XbaI","TCTAGA",1),
    ("XhoI","CTCGAG",1), ("XmaI","CCCGGG",1),
    # Type IIS: these cut *outside* their site, on both strands, and are the
    # reason the enzyme model had to grow a stored signed overhang. The Python
    # transcription below scans the forward strand only and would miss their
    # minus-strand sites entirely, so they are compared through the Rust arm
    # and Biopython alone -- see `RUST_ONLY`.
    ("BsaI","GGTCTC",7), ("BsmBI","CGTCTC",7), ("Esp3I","CGTCTC",7),
    ("BbsI","GAAGAC",8), ("SapI","GCTCTTC",8), ("BspQI","GCTCTTC",8),
    ("PaqCI","CACCTGC",11), ("AarI","CACCTGC",11),
]

# Enzymes whose sites are not palindromes. `our_digest` below scans one strand,
# which is correct for every Type IIP enzyme and incomplete for these, so the
# Python arm is skipped for them and the Rust is compared against Biopython
# directly. Skipping *quietly* would be the usual mistake; the summary prints
# how many were compared each way.
RUST_ONLY = {"BsaI", "BsmBI", "Esp3I", "BbsI", "SapI", "BspQI", "PaqCI", "AarI"}


def our_digest(seq, circular):
    """Port of the prototype's digest(). Returns {enzyme: sorted positions}."""
    s = seq.upper()
    n = len(s)
    if not n:
        return {}
    max_site = max(len(e[1]) for e in OUR_ENZYMES)
    ext = s + s[:min(max_site - 1, n)] if circular else s

    out = {}
    for name, site, cut in OUR_ENZYMES:
        pos = []
        i = ext.find(site)
        while i != -1:
            if i < n:
                pos.append(((i + cut) % n) + 1)
            i = ext.find(site, i + 1)
        if pos:
            out[name] = sorted(set(pos))
    return out


def bio_digest(seq, circular):
    """Biopython's answer, restricted to the same enzyme names."""
    names = {e[0] for e in OUR_ENZYMES}
    batch = Restriction.RestrictionBatch([n for n in names
                                          if hasattr(Restriction, n)])
    res = batch.search(Seq(seq.upper()), linear=not circular)
    out = {}
    for enz, sites in res.items():
        if sites:
            out[str(enz)] = sorted(set(sites))
    return out


def rust_digest(exe, path):
    """The shipped binary's answer, in the same shape.

    This arm is the one that guards `pl_enzymes::cut_positions`, which is now a
    thin wrapper over `pl_core::iupac::find_all` so that the library's motif
    search and the digest share one scan. Without it, this file cross-checks a
    Python transcription that no user ever runs.
    """
    out = subprocess.run([exe, "digest", path, "--json"],
                         capture_output=True, text=True)
    if out.returncode != 0:
        raise RuntimeError(f"{exe} digest {path}: {out.stderr.strip()}")
    doc = json.loads(out.stdout)
    return {d["enzyme"]: sorted(set(d["positions"]))
            for d in doc["digests"] if d["positions"]}


def compare(path, exe=None):
    doc = snapdna.load(path)
    if not doc.sequence or doc.length > 300000:
        return None
    ours = our_digest(doc.sequence, doc.is_circular)
    theirs = bio_digest(doc.sequence, doc.is_circular)
    rust = rust_digest(exe, path) if exe else None

    names = set(ours) | set(theirs) | set(rust or {})
    mismatches = []
    for nme in sorted(names):
        a, b = ours.get(nme, []), theirs.get(nme, [])
        if nme not in RUST_ONLY and a != b:
            mismatches.append((nme, "python", a, b))
        if rust is not None:
            r = rust.get(nme, [])
            if r != b:
                mismatches.append((nme, "rust", r, b))
    return doc, ours, theirs, mismatches


def main(argv):
    # An executable as the first argument turns on the Rust arm.
    exe = None
    if argv and os.path.isfile(argv[0]) and not glob.has_magic(argv[0]):
        low = argv[0].lower()
        if low.endswith(".exe") or os.access(argv[0], os.X_OK):
            # Absolute, and with native separators: CreateProcess does not
            # resolve a relative program path spelled with forward slashes, so
            # `target/release/pl.exe` fails on Windows with "cannot find the
            # file specified" while os.path.isfile has just said it is there.
            exe, argv = os.path.abspath(argv[0]), argv[1:]

    files = []
    for p in argv:
        files.extend(glob.glob(p, recursive=True))
    files = sorted(set(files))

    total_sites = 0
    total_mm = 0
    checked = 0

    print(f"{'bp':>9} {'top':>5} {'ours':>6} {'biopy':>6} {'diff':>5}  file")
    detail = []
    for f in files:
        r = compare(f, exe)
        if r is None:
            continue
        doc, ours, theirs, mm = r
        checked += 1
        n_ours = sum(len(v) for v in ours.values())
        n_theirs = sum(len(v) for v in theirs.values())
        total_sites += n_ours
        total_mm += len(mm)
        print(f"{doc.length:>9,} {'circ' if doc.is_circular else 'lin':>5} "
              f"{n_ours:>6} {n_theirs:>6} {len(mm):>5}  {os.path.basename(f)[:44]}")
        if mm:
            detail.append((os.path.basename(f), mm))

    print(f"\n{'='*74}")
    print(f"files compared        : {checked}")
    print(f"cut sites cross-checked: {total_sites:,}")
    print(f"enzymes disagreeing   : {total_mm}")
    print(f"implementations       : python + biopython"
          + (" + rust" if exe else "  (rust arm OFF -- pass pl.exe to enable)"))
    print(f"type IIS enzymes      : {len(RUST_ONLY)} (rust vs biopython only;")
    print(f"                        the python arm scans one strand and cannot")
    print(f"                        see their minus-strand sites)")

    for fname, mm in detail[:6]:
        print(f"\n  {fname}")
        for nme, side, a, b in mm[:6]:
            print(f"    {nme:<9} {side}={a[:8]}  biopython={b[:8]}")

    # Zero comparisons is a failure, not a pass. A cross-check that quietly
    # does nothing reports success for having compared nothing at all.
    if checked == 0:
        print("\nFAIL: compared 0 files. Pass a glob of .dna files, e.g."
              "\n  python validate_digest.py target/release/pl.exe 'corpus/**/*.dna'")
        return 1
    if total_mm:
        print(f"\nFAIL: {total_mm} disagreement(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
