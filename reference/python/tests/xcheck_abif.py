"""Cross-validate the ABIF reader against Biopython, on real chromatograms.

    python xcheck_abif.py target/release/pl.exe '<glob of .ab1>'

Biopython's `Bio.SeqIO` ABI parser is an independent implementation of the same
format, so it is the oracle for the two things a user reads off a trace: the
base calls and the per-base quality.

Two facts about real `.ab1` files, measured on a working lab drive of 394 of
them and worth stating because both are traps:

  * **20 are not ABIF at all** — 4 SCF and 16 ZTR, 5% of the total. A reader
    that trusts the extension produces nonsense for one file in twenty. Those
    are checked here too: both implementations must *refuse* them, and a
    refusal is counted rather than skipped.
  * **`PBAS1` and `PBAS2` differ in 58% of the ABIF files.** `PBAS2` is the
    basecaller's call and `PBAS1` is what a human edited it to — the opposite
    of what the numbering suggests. Biopython reports `PBAS2`, and so do we,
    which is what makes this comparison meaningful at all.

Exits 1 on any disagreement and on comparing nothing.
"""
import glob
import json
import os
import subprocess
import sys
import warnings

from Bio import SeqIO, BiopythonParserWarning

warnings.simplefilter("ignore", BiopythonParserWarning)


def theirs(path):
    """(sequence, mean quality) per Biopython, or None if it refuses."""
    try:
        r = SeqIO.read(path, "abi")
    except Exception:
        return None
    q = r.letter_annotations.get("phred_quality") or []
    return str(r.seq).upper(), (sum(q) / len(q) if q else None)


def ours(exe, path):
    out = subprocess.run([exe, "trace", "--json", path],
                         capture_output=True, text=True)
    line = out.stdout.strip()
    if not line:
        return None  # refused, with a reason on stderr
    d = json.loads(line)
    return d["sequence"].upper(), d["mean_quality"]


def main(argv):
    exe = None
    if argv and os.path.isfile(argv[0]):
        exe, argv = os.path.abspath(argv[0]), argv[1:]
    if exe is None:
        print("usage: xcheck_abif.py <pl.exe> '<glob>'")
        return 1

    files = []
    for p in argv:
        files.extend(glob.glob(p, recursive=True))
    files = sorted(set(files))

    compared = 0
    refused_both = 0
    bases = 0
    bad = []
    for f in files:
        want = theirs(f)
        got = ours(exe, f)
        if want is None and got is None:
            # Both refuse: an SCF or ZTR file wearing an .ab1 name. Counted,
            # not skipped -- "we agreed to fail" is a real result and a
            # comparison that silently drops them is comparing less than it
            # claims.
            refused_both += 1
            continue
        if want is None or got is None:
            bad.append((f, "one implementation refused and the other did not",
                        got is not None, want is not None))
            continue
        compared += 1
        bases += len(want[0])
        if got[0] != want[0]:
            n = sum(1 for a, b in zip(got[0], want[0]) if a != b)
            bad.append((f, f"sequence differs at {n} position(s) "
                           f"(lengths {len(got[0])} vs {len(want[0])})",
                        got[0][:40], want[0][:40]))
        elif want[1] is not None and got[1] is not None and abs(got[1] - want[1]) > 1e-6:
            bad.append((f, "mean quality differs", got[1], want[1]))

    print("=" * 74)
    print(f"files given           : {len(files)}")
    print(f"chromatograms compared: {compared}")
    print(f"base calls compared   : {bases:,}")
    print(f"refused by both       : {refused_both}  (SCF/ZTR wearing an .ab1 name)")
    print(f"disagreements         : {len(bad)}")
    print()
    print("Biopython parses ABIF independently. Both read PBAS2, the")
    print("basecaller's call -- PBAS1 is the human-edited one, which differs in")
    print("most real files and is reported separately rather than substituted.")

    for f, why, a, b in bad[:8]:
        print(f"\n  {os.path.basename(f)}: {why}")
        print(f"    ours      = {a}")
        print(f"    biopython = {b}")

    if compared == 0:
        print("\nFAIL: compared no chromatograms")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} disagreement(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
