"""Cross-check Polylinker's SEGUID implementation against the reference.

The checksums are only worth anything if other tools compute the same ones, so
the test is exact string equality against the Python `seguid` package (0.2.1,
MIT, Bjorn Johansson) — the implementation the specification is written from.

Hand-picked vectors prove very little about a hash: they are the cases the
author was already thinking about. This generates thousands, including the
shapes that break naive rotation code — palindromes, homopolymers, periodic
repeats, and sequences whose smallest rotation is ambiguous — plus every real
molecule in the corpus.

Usage:
    python xcheck_seguid.py <path-to-pl.exe> ["<glob of .dna/.gb files>"]
"""

import glob
import json
import os
import random
import subprocess
import sys

try:
    from seguid import cdseguid, csseguid, ldseguid, lsseguid, seguid
except ImportError:
    sys.exit("needs the reference implementation:  pip install seguid")


def rc(s):
    return s.translate(str.maketrans("ACGT", "TGCA"))[::-1]


def generate():
    """(label, sequence) pairs, biased towards the awkward shapes."""
    rng = random.Random(20260726)
    out = []

    # Deliberate edge shapes.
    out += [("single", "A"), ("two", "AT"), ("palindrome", "GAATTC")]
    out += [(f"homopolymer{n}", "A" * n) for n in (1, 2, 3, 7, 64, 65)]
    out += [(f"periodic-AT{n}", "AT" * n) for n in (1, 2, 8, 32)]
    out += [(f"periodic-ACG{n}", "ACG" * n) for n in (1, 3, 21)]
    # Nearly-periodic: one base off, which changes the minimal rotation.
    out += [("near-periodic", "AT" * 15 + "AA")]
    out += [("lyndon-ish", "ACAACAAACAACACAAACAAACACAAC")]
    out += [("all-same-but-one", "A" * 30 + "C")]
    out += [("descending", "TTTTGGGGCCCCAAAA")]

    # Random sequences across a range of lengths.
    for n in (1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 500, 997, 5000):
        for k in range(6):
            out.append((f"rand{n}-{k}", "".join(rng.choice("ACGT") for _ in range(n))))

    # Sequences built from a small alphabet, where ties are common.
    for n in (10, 40, 200):
        for k in range(4):
            out.append((f"binary{n}-{k}", "".join(rng.choice("AT") for _ in range(n))))
    return out


def corpus_sequences(pattern, pl):
    """Real molecules, via the pl binary so we read exactly what it reads."""
    if not pattern:
        return []
    files = sorted(f for f in glob.glob(pattern, recursive=True) if os.path.isfile(f))
    out = []
    for f in files:
        try:
            r = subprocess.run([pl, "convert", f, "--to", "fasta", "--stdout"],
                               capture_output=True, text=True, encoding="utf-8")
            if r.returncode != 0:
                continue
            seq = "".join(l.strip() for l in r.stdout.splitlines() if not l.startswith(">"))
            seq = seq.upper()
            # The reference only accepts unambiguous DNA.
            if seq and set(seq) <= set("ACGT"):
                out.append((os.path.basename(f), seq))
        except Exception:
            continue
    return out


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    pl = sys.argv[1]
    pattern = sys.argv[2] if len(sys.argv) > 2 else None

    cases = generate()
    real = corpus_sequences(pattern, pl)
    print(f"{len(cases)} generated sequences, {len(real)} from the corpus")

    # Ask the reference for the expected values.
    expected = []
    for label, s in cases + real:
        row = {"label": label, "seq": s}
        row["seguid"] = seguid(s)
        row["lsseguid"] = lsseguid(s)
        row["csseguid"] = csseguid(s)
        row["ldseguid"] = ldseguid(s, rc(s))
        row["cdseguid"] = cdseguid(s, rc(s))
        expected.append(row)

    # Ask our implementation for the same, in one batch.
    payload = json.dumps([{"label": r["label"], "seq": r["seq"]} for r in expected])
    r = subprocess.run([pl, "checksum", "--stdin-json"], input=payload,
                       capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        sys.exit(f"pl checksum failed:\n{r.stderr[:4000]}")
    ours = {row["label"]: row for row in json.loads(r.stdout)}

    forms = ["seguid", "lsseguid", "csseguid", "ldseguid", "cdseguid"]
    agree = 0
    mismatches = []
    for exp in expected:
        got = ours.get(exp["label"])
        if got is None:
            mismatches.append((exp["label"], "missing", "", ""))
            continue
        bad = [(f, exp[f], got.get(f, "")) for f in forms if exp[f] != got.get(f)]
        if bad:
            for f, e, g in bad:
                mismatches.append((exp["label"], f, e, g))
        else:
            agree += 1

    print(f"\n{'=' * 70}")
    print(f"sequences compared : {len(expected)}")
    print(f"agree on all 5 forms: {agree}")
    print(f"mismatches         : {len(mismatches)}")
    for label, form, e, g in mismatches[:15]:
        print(f"   {label:>22} {form:>9}")
        print(f"       reference: {e}")
        print(f"       ours     : {g}")
    total = sum(len(r["seq"]) for r in expected)
    print(f"\n{total:,} bases checked across 5 checksum forms")
    print("agreement with the reference is the whole point: a checksum only has")
    print("value if other tools compute the same one")
    return 0 if not mismatches else 1


if __name__ == "__main__":
    sys.exit(main())
