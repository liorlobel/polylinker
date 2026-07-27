"""Cross-validate primer binding-site detection against pydna.

    python xcheck_primers.py target/release/pl.exe

pydna's `Anneal` finds the same thing by different means, and `docs/PLAN.md`
§7.3 names its `limit` as the reference for the seed length. It is an exact
matcher, so the comparison runs `pl primers --exact`, the mode that implements
the same rule. Our default extends through *isolated*
mismatches — right for a mutagenesis primer — and that is a different question,
covered by unit tests rather than quietly skipped here.

That distinction was found the hard way: with a random 5' tail, the base next to
the footprint mismatches and the one beyond it matches by chance about a quarter
of the time, so the lenient extension absorbed two bases of tail in six of
eighty cases. Neither implementation was wrong; the comparison was.

Coordinate conventions were determined by probing pydna rather than assumed:

  * a forward primer's `position` is the **1-based end** of the footprint;
  * a reverse primer's `position` is the **0-based start** of the footprint on
    the plus strand — one less than the 1-based start.

Getting that pair backwards is the sort of error that produces a beautifully
consistent off-by-one across every case, so it is written down here.

Exits 1 on any disagreement and on comparing nothing.
"""
import json
import os
import random
import subprocess
import sys

from Bio.Seq import Seq
from pydna.amplify import Anneal
from pydna.dseqrecord import Dseqrecord
from pydna.primer import Primer

SEED = 14
rng = random.Random(20260727)


def rand_seq(n):
    return "".join(rng.choice("ACGT") for _ in range(n))


def build_cases():
    """Templates with primers planted at known places.

    Deliberately random rather than hand-written: a hand-written template is how
    this project has repeatedly ended up with an accidental palindrome, and a
    self-complementary run gives a primer a second site that looks like a bug in
    whichever implementation reports it.
    """
    cases = []
    for i in range(60):
        n = rng.randint(120, 400)
        t = rand_seq(n)
        circular = i % 3 == 0
        plen = rng.randint(SEED + 2, 30)
        at = rng.randint(0, n - plen - 1)
        fwd = t[at:at + plen]
        # A reverse primer from a non-overlapping stretch.
        at2 = rng.randint(0, n - plen - 1)
        rev = str(Seq(t[at2:at2 + plen]).reverse_complement())
        cases.append({"id": f"c{i}", "seq": t, "circular": circular,
                      "primers": [fwd, rev]})
    # Tailed primers: the case the footprint/tail split exists for.
    for i in range(20):
        n = rng.randint(120, 300)
        t = rand_seq(n)
        plen = rng.randint(SEED + 2, 24)
        at = rng.randint(0, n - plen - 1)
        tail = rand_seq(rng.randint(6, 20))
        cases.append({"id": f"t{i}", "seq": t, "circular": False,
                      "primers": [tail + t[at:at + plen]]})
    return cases


def theirs(case):
    """pydna's answer as a set of (strand, footprint, three_prime_position)."""
    rec = Dseqrecord(case["seq"], circular=case["circular"])
    prims = [Primer(p, id=str(i)) for i, p in enumerate(case["primers"])]
    a = Anneal(prims, rec, limit=SEED)
    out = set()
    for x in a.forward_primers:
        out.add(("+", str(x.footprint).upper(), int(x.position)))
    for x in a.reverse_primers:
        out.add(("-", str(x.footprint).upper(), int(x.position)))
    return out


def ours(exe, case):
    # `--exact` is the rule pydna implements: stop the footprint at the first
    # mismatch. Without it our extension walks through isolated mismatches,
    # which is right for a mutagenesis primer and is a different question from
    # the one this file asks.
    args = [exe, "primers", "--json", "--exact", "--seed", str(SEED),
            "--seq", case["seq"]]
    if case["circular"]:
        args.append("--circular")
    for p in case["primers"]:
        args += ["--primer", p]
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"pl primers: {r.stderr.strip()}")
    doc = json.loads(r.stdout)
    out = set()
    for b in doc["bindings"]:
        fp = b["footprint"].upper()
        if b["strand"] == "+":
            out.add(("+", fp, b["end"]))
        else:
            # pydna reports the 0-based start for a reverse primer, and our
            # footprint is written 5'->3' along the primer, so it is the
            # reverse complement of the plus-strand span.
            out.add(("-", fp, b["start"] - 1))
    return out


def main(argv):
    exe = None
    if argv and os.path.isfile(argv[0]):
        exe = os.path.abspath(argv[0])
    if exe is None:
        print("usage: xcheck_primers.py <path to pl.exe>")
        return 1

    cases = build_cases()
    compared = 0
    sites = 0
    bad = []
    for c in cases:
        want = theirs(c)
        got = ours(exe, c)
        compared += 1
        sites += len(want)
        if got != want:
            bad.append((c, sorted(got), sorted(want)))

    print("=" * 74)
    print(f"templates compared    : {compared}")
    print(f"binding sites (pydna) : {sites}")
    print(f"disagreements         : {len(bad)}")
    print()
    print("pydna finds the same sites by different means, run through")
    print("`pl primers --exact`, the mode implementing the same rule.")
    print("Tailed primers are included: the footprint must come back without")
    print("the tail, or the Tm reported next to it is a different oligo's.")

    for c, got, want in bad[:5]:
        print(f"\n  {c['id']} ({'circular' if c['circular'] else 'linear'}, "
              f"{len(c['seq'])} bp), primers {c['primers']}")
        print(f"    ours  = {got}")
        print(f"    pydna = {want}")

    if compared == 0 or sites == 0:
        print("\nFAIL: compared nothing")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} disagreement(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
