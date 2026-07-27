"""Cross-validate melting temperatures against Biopython.

    python xcheck_tm.py target/release/pl.exe

`Bio.SeqUtils.MeltingTemp.Tm_NN` is an independent implementation of the same
published model, and `docs/PLAN.md` §7.2 names its tables as the licence-clean
source for the parameters (Primer3's `oligotm.c` is GPL-2.0 and off limits). We
extracted the numbers from it; this checks the *arithmetic around them*, which
is the part that was written here.

Concentrations are the fiddly part and the reason this is worth doing.
Biopython takes two strand concentrations in nM and forms
`k = (dnac1 - dnac2/2) * 1e-9` for a non-self-complementary duplex, and
`k = dnac1 * 1e-9` for a self-complementary one. Our model takes a single total
strand concentration `C_T` and divides by `x`, where `x` is 4 for a palindrome
and 1 otherwise. Those agree when `dnac1 = dnac2 = C_T` for the palindrome case
and `dnac1 = 2*C_T`, `dnac2 = 0` otherwise -- a translation that is easy to get
subtly wrong and would shift every number by a fraction of a degree, which is
exactly the size of error nobody notices.

Exits 1 on any disagreement and on comparing nothing.
"""
import json
import math
import os
import subprocess
import sys

from Bio.Seq import Seq
from Bio.SeqUtils import MeltingTemp as mt

# Oligos chosen for what they exercise: GC extremes, palindromes, homopolymer
# runs, the length range primers actually live in, and the AA/TT stack that is
# the one difference between the two parameter sets.
OLIGOS = [
    "ACGTACGTACGTACGTACGT",
    "ATATATATATATATATATAT",
    "GCGCGCGCGCGCGCGCGCGC",
    "GAATTC",
    "GGATCC",
    "AAAACCCCGGGGTTTT",     # its own reverse complement
    "AAAAAAAAAAAAAAAAAAAA",
    "TTTTTTTTTTTTTTTTTTTT",
    "CCCCCCCCCCCCCCCCCCCC",
    "AT",
    "GC",
    "ACGTACGTACGTACGTACGTACGTACGTACGT",
    "GTAAAACGACGGCCAGTGAATT",       # M13 forward
    "CAGGAAACAGCTATGACCATG",        # M13 reverse
    "TAATACGACTCACTATAGGG",         # T7
    "ATTTAGGTGACACTATAG",           # SP6
    "GGGGGGGGGGAAAAAAAAAA",
    "ACGT",
    "AACCGGTT",
    "TGCATGCATGCATGCA",
]

TABLES = {"1998": mt.DNA_NN3, "2004": mt.DNA_NN4}


def theirs(seq, table, na_mM, oligo_nM):
    """Biopython's answer, with the concentration convention translated."""
    selfcomp = str(Seq(seq).reverse_complement()).upper() == seq.upper()
    if selfcomp:
        # Biopython uses k = dnac1 for a palindrome, and the model wants C_T.
        dnac1, dnac2 = oligo_nM, oligo_nM
    else:
        # k = dnac1 - dnac2/2, which is C_T/4 when each strand is at C_T/2.
        dnac1 = dnac2 = oligo_nM / 2
    return mt.Tm_NN(
        seq,
        nn_table=TABLES[table],
        Na=na_mM,
        dnac1=dnac1,
        dnac2=dnac2,
        selfcomp=selfcomp,
        saltcorr=5,      # SantaLucia 1998, the entropy correction
    )


def ours(exe, seqs, table, na_mM, oligo_nM):
    out = subprocess.run(
        [exe, "tm", "--json", "--table", table,
         "--na", str(na_mM), "--oligo", str(oligo_nM), *seqs],
        capture_output=True, text=True,
    )
    if out.returncode != 0:
        raise RuntimeError(f"pl tm: {out.stderr.strip()}")
    return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]


def main(argv):
    exe = None
    if argv and os.path.isfile(argv[0]):
        exe = os.path.abspath(argv[0])
    if exe is None:
        print("usage: xcheck_tm.py <path to pl.exe>")
        return 1

    compared = 0
    worst = 0.0
    worst_case = None
    bad = []
    for table in ("1998", "2004"):
        for na in (10, 50, 200, 1000):
            for conc in (1, 50, 500):
                mine = ours(exe, OLIGOS, table, na, conc)
                for row, seq in zip(mine, OLIGOS):
                    want = theirs(seq, table, na, conc)
                    got = row["tm"]
                    compared += 1
                    d = abs(got - want)
                    if d > worst:
                        worst, worst_case = d, (seq, table, na, conc, got, want)
                    # A thousandth of a degree: this is the same arithmetic on
                    # the same numbers, so anything larger is a real difference
                    # and not floating point.
                    if d > 1e-3:
                        bad.append((seq, table, na, conc, got, want, d))

    print("=" * 74)
    print(f"oligos                : {len(OLIGOS)}")
    print(f"comparisons           : {compared}")
    print(f"disagreements         : {len(bad)}")
    print(f"worst difference      : {worst:.6f} C")
    if worst_case:
        s, t, na, c, g, w = worst_case
        print(f"  at {s} ({t}, {na} mM Na+, {c} nM): ours {g:.4f}, Biopython {w:.4f}")
    print()
    print("Biopython implements the same published model independently; the")
    print("parameters came from it, so this checks the arithmetic written here")
    print("-- especially the concentration convention, where a subtle error")
    print("shifts every number by a fraction of a degree")

    for s, t, na, c, g, w, d in bad[:10]:
        print(f"\n  {s} ({t}, Na {na} mM, oligo {c} nM)")
        print(f"    ours      = {g:.6f}")
        print(f"    biopython = {w:.6f}   (off by {d:.6f})")

    if compared == 0:
        print("\nFAIL: compared nothing")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} disagreement(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
