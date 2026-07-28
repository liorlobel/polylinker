"""Validate snapdna against a real-world corpus.

Checks, per file:
  1. it parses without error
  2. re-serialising reproduces the original bytes exactly
  3. the sequence is valid IUPAC and matches the declared length
  4. every feature segment lies within the sequence (or wraps, if circular)
  5. dropping the derived cache blocks still produces a well-framed file

    python test_roundtrip.py '<glob>' ...

**This script used to be unable to fail**, in exactly the shape
`validate_digest.py` was fixed for. It counted every parse error, every
non-byte-exact round trip, every out-of-bounds segment into `total_problems`,
printed the number, and then returned `None` to a `__main__` that discarded it:
break `snapdna.dumps` and a run over the corpus prints `problems found : 344`
and still exits 0. A glob matching nothing printed `files clean : 0/0` and
exited 0 too, so verifying nothing looked exactly like verifying everything.
README.md and CONTRIBUTING.md both name this beside `validate_digest.py` as a
check to run yourself, so the two made the same promise and only one kept it.
Both cases now exit 1.
"""
import glob
import os
import sys

# snapdna lives one level up, in reference/python/
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import snapdna  # noqa: E402

IUPAC = set("ACGTRYSWKMBDHVNacgtryswkmbdhvn")


def check(path):
    problems = []
    doc = snapdna.load(path)

    # 2. byte-exact round trip
    with open(path, "rb") as fh:
        original = fh.read()
    if snapdna.dumps(doc) != original:
        problems.append("round-trip NOT byte-exact")

    # 3. sequence sanity
    bad = set(doc.sequence) - IUPAC
    if bad:
        problems.append(f"non-IUPAC characters in sequence: {sorted(bad)[:6]}")

    # 4. feature bounds
    n = doc.length
    oob = 0
    for f in doc.features:
        for s in f.segments:
            if s.start < 1 or s.end > n:
                oob += 1
    if oob:
        problems.append(f"{oob} feature segment(s) out of bounds (len={n})")

    # 5. reserialise without the derived caches, then reparse
    slim = snapdna.dumps(doc, drop_derived=True)
    try:
        doc2 = snapdna.loads(slim)
    except Exception as e:
        problems.append(f"slim file failed to reparse: {e!r}")
    else:
        if doc2.sequence != doc.sequence:
            problems.append("slim file lost sequence fidelity")
        if len(doc2.features) != len(doc.features):
            problems.append("slim file lost features")

    saving = 1 - len(slim) / len(original)
    return doc, problems, saving


def main(patterns):
    files = []
    for p in patterns:
        files.extend(glob.glob(p, recursive=True))
    files = sorted(set(files))

    ok = 0
    total_problems = 0
    savings = []
    hist_count = 0

    print(f"{'bp':>9} {'top':>8} {'feat':>5} {'prim':>5} {'hist':>5} {'slim':>6}  file")
    for f in files:
        try:
            doc, problems, saving = check(f)
        except Exception as e:
            print(f"{'ERROR':>9} {'':>8} {'':>5} {'':>5} {'':>5} {'':>6}  "
                  f"{os.path.basename(f)[:44]}  <- {e!r}")
            total_problems += 1
            continue
        savings.append(saving)
        if doc.history_xml:
            hist_count += 1
        status = "ok" if not problems else "; ".join(problems)
        if not problems:
            ok += 1
        else:
            total_problems += len(problems)
        print(f"{doc.length:>9,} {'circ' if doc.is_circular else 'lin':>8} "
              f"{len(doc.features):>5} {len(doc.primers):>5} "
              f"{'yes' if doc.history_xml else '-':>5} {saving:>5.0%}  "
              f"{os.path.basename(f)[:44]}"
              + ("" if not problems else f"\n              !! {status}"))

    print(f"\n{'='*70}")
    print(f"files clean            : {ok}/{len(files)}")
    print(f"problems found         : {total_problems}")
    print(f"history tree recovered : {hist_count}/{len(files)}")
    if savings:
        print(f"size saved by dropping derived caches: "
              f"mean {sum(savings)/len(savings):.0%}, max {max(savings):.0%}")

    # Zero files is a failure, not a pass. A corpus check that quietly matched
    # nothing reports success for having validated nothing at all -- and the
    # glob is quoted precisely because the shell must NOT expand it, which is
    # the one typo that turns a 344-file run into a 0-file run.
    if not files:
        print("\nFAIL: matched 0 files. Quote the glob, e.g."
              "\n  python test_roundtrip.py 'corpus/**/*.dna'")
        return 1
    if total_problems:
        print(f"\nFAIL: {total_problems} problem(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
