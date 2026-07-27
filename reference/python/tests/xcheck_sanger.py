"""Cross-validate Sanger read placement against Biopython's PairwiseAligner.

    python xcheck_sanger.py target/release/pl.exe

**The alignment score is the thing compared, deliberately.** An optimal
alignment is usually not unique — when the bases flanking a deletion repeat, two
placements of the gap score identically and both are correct — so comparing
traceback columns would be comparing tie-breaks between two implementations that
never agreed to break ties the same way. The score is well defined, and any
error in the recurrence, the initialisation, the affine bookkeeping or the
windowing changes it.

Conventions determined by probing rather than assumed, because getting either
backwards produces a consistent, plausible, wrong number:

  * A **query gap** in Biopython is a gap inserted *into the query*, so free
    query end gaps are what let the reference stick out past the read on both
    sides. That is our semi-global shape; `query_end_gap_score = 0`.
  * Biopython charges `open_gap_score` for the first gap character and
    `extend_gap_score` for each one after, so a gap of length L costs
    `open + (L-1)*extend`. Ours charges `gap_open + L*gap_extend`. Passing our
    `gap_open + gap_extend` as Biopython's `open_gap_score` makes the two
    agree; passing `gap_open` alone is off by exactly one extension per gap,
    which is invisible on reads with no indels.

Reads are generated without `N` and without lower case. Both are handled by
this crate, and handled *differently* from Biopython -- an `N` is scored as a
mismatch here because a basecaller that gave up is not evidence of a match,
while Biopython's match/mismatch matrix scores `N` against `N` as a match.
That difference is deliberate, is covered by unit tests, and would otherwise
show up here as a disagreement about nothing.

Exits 1 on any disagreement and on comparing nothing.
"""
import json
import os
import random
import subprocess
import sys

from Bio import Align
from Bio.Seq import Seq

MATCH, MISMATCH, GAP_OPEN, GAP_EXTEND = 1, -2, -5, -1
rng = random.Random(20260728)


def aligner():
    a = Align.PairwiseAligner()
    a.mode = "global"
    a.match_score = MATCH
    a.mismatch_score = MISMATCH
    # See the module docstring: Biopython's "open" covers the first gap
    # character, ours does not.
    a.open_gap_score = GAP_OPEN + GAP_EXTEND
    a.extend_gap_score = GAP_EXTEND
    # The reference may hang off both ends of the read for free; the read may
    # not hang off the reference.
    # Renamed in recent Biopython; both spellings mean the same thing.
    try:
        a.end_deletion_score = 0.0
    except AttributeError:
        a.query_end_gap_score = 0.0
    return a


def rand_seq(n):
    return "".join(rng.choice("ACGT") for _ in range(n))


def mutate(s):
    """A read with a plausible mix of damage."""
    s = list(s)
    for _ in range(rng.randint(0, 4)):  # substitutions
        i = rng.randrange(len(s))
        s[i] = rng.choice([b for b in "ACGT" if b != s[i]])
    if rng.random() < 0.4:  # one indel, sometimes
        i = rng.randrange(10, len(s) - 20)
        if rng.random() < 0.5:
            del s[i:i + rng.randint(1, 9)]
        else:
            s[i:i] = list(rand_seq(rng.randint(1, 9)))
    return "".join(s)


def build_cases():
    cases = []
    for i in range(50):
        n = rng.randint(400, 1500)
        ref = rand_seq(n)
        circular = i % 4 == 0
        rlen = rng.randint(80, 300)
        at = rng.randrange(0, n - rlen)
        read = mutate(ref[at:at + rlen])
        if i % 3 == 0:  # sequenced with a reverse primer
            read = str(Seq(read).reverse_complement())
        cases.append({"id": f"c{i}", "ref": ref, "circular": circular, "read": read})
    # Reads that cross the origin: only meaningful on a circle, and the case a
    # linear aligner cannot represent at all.
    for i in range(10):
        n = rng.randint(400, 900)
        ref = rand_seq(n)
        half = rng.randint(40, 120)
        read = mutate(ref[-half:] + ref[:half])
        cases.append({"id": f"o{i}", "ref": ref, "circular": True, "read": read})
    return cases


def theirs(case):
    """Biopython's optimal score, best of the two orientations."""
    a = aligner()
    # A circular reference is doubled, exactly as we do it, so a read spanning
    # the origin has somewhere to land.
    target = case["ref"] * 2 if case["circular"] else case["ref"]
    read = case["read"]
    return max(
        a.score(target, read),
        a.score(target, str(Seq(read).reverse_complement())),
    )


def ours(exe, case):
    args = [exe, "sanger", "--json", "--ref-seq", case["ref"], "--read", case["read"]]
    if case["circular"]:
        args.append("--circular")
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"pl sanger: {r.stderr.strip()}")
    return json.loads(r.stdout)["reads"][0]


def main(argv):
    exe = None
    if argv and os.path.isfile(argv[0]):
        exe = os.path.abspath(argv[0])
    if exe is None:
        print("usage: xcheck_sanger.py <path to pl.exe>")
        return 1

    cases = build_cases()
    compared = 0
    bases = 0
    bad = []
    for c in cases:
        got = ours(exe, c)
        if not got["placed"]:
            bad.append((c, "we could not place this read", None, None))
            continue
        want = theirs(c)
        compared += 1
        bases += len(c["read"])
        if got["score"] != want:
            bad.append((c, "score", got["score"], want))
            continue
        # The reported orientation must also be the one that scores: a read
        # placed on the wrong strand with a coincidentally equal score would
        # otherwise pass.
        a = aligner()
        target = c["ref"] * 2 if c["circular"] else c["ref"]
        fwd = a.score(target, c["read"])
        rev = a.score(target, str(Seq(c["read"]).reverse_complement()))
        if fwd != rev:
            expect_rev = rev > fwd
            if got["reversed"] != expect_rev:
                bad.append((c, "orientation", got["reversed"], expect_rev))

    print("=" * 74)
    print(f"reads compared    : {compared}")
    print(f"read bases        : {bases:,}")
    print(f"disagreements     : {len(bad)}")
    print()
    print("Biopython's PairwiseAligner solves the same semi-global affine")
    print("problem. The score is compared, not the traceback: an optimal")
    print("alignment is not unique, so comparing columns would compare")
    print("tie-breaks. Circular references are doubled on both sides.")

    for c, why, got, want in bad[:6]:
        print(f"\n  {c['id']} ({len(c['ref'])} bp "
              f"{'circular' if c['circular'] else 'linear'}, "
              f"{len(c['read'])} nt read): {why}")
        print(f"    ours      = {got}")
        print(f"    biopython = {want}")

    if compared == 0:
        print("\nFAIL: compared nothing")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} disagreement(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
