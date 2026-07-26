"""Cross-validate our restriction digest against Biopython's Bio.Restriction.

Two wholly independent implementations, on real plasmids, over the same
enzyme set. Any disagreement is a bug in ours until proven otherwise.

This is the QA pattern the whole project needs: never trust a hand-rolled
biology routine that has not been checked against an established one.
"""
import sys
import os
import glob

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
]


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


def compare(path):
    doc = snapdna.load(path)
    if not doc.sequence or doc.length > 300000:
        return None
    ours = our_digest(doc.sequence, doc.is_circular)
    theirs = bio_digest(doc.sequence, doc.is_circular)

    names = set(ours) | set(theirs)
    mismatches = []
    for nme in sorted(names):
        a, b = ours.get(nme, []), theirs.get(nme, [])
        if a != b:
            mismatches.append((nme, a, b))
    return doc, ours, theirs, mismatches


def main(patterns):
    files = []
    for p in patterns:
        files.extend(glob.glob(p, recursive=True))
    files = sorted(set(files))

    total_sites = 0
    total_mm = 0
    checked = 0

    print(f"{'bp':>9} {'top':>5} {'ours':>6} {'biopy':>6} {'diff':>5}  file")
    detail = []
    for f in files:
        r = compare(f)
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

    for fname, mm in detail[:6]:
        print(f"\n  {fname}")
        for nme, a, b in mm[:6]:
            print(f"    {nme:<9} ours={a[:8]}  biopython={b[:8]}")


if __name__ == "__main__":
    main(sys.argv[1:])
