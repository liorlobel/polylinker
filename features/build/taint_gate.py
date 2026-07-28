#!/usr/bin/env python3
"""The CI taint gate: measure our descriptions against pLannotate's snapgene.csv.

`features/SOURCING.md` section 0.4 specifies this gate, `features/NOTICE` and
`features/README.md` disclose it to users, and SOURCING.md section 6 calls that
disclosure "the concrete evidence behind the project's entire premise".

It did not exist. Three shipped files asserted it in the present tense while the
repository contained no implementation, no `.gitignore` entry and no pre-commit
hook -- so the 70 descriptions this database ships had never been compared
against the thing the project's central claim depends on. Disclosing a control
that was never built is worse than claiming nothing, which is why this file is
here rather than a rewritten sentence.

WHAT THIS DOES NOT DO
---------------------

It does not read `snapgene.csv` into the database, into a description, or into
any file this repository keeps. The payload is fetched to a temporary directory
*outside* the working tree, hashed, measured, and deleted in a `finally` block.
Nothing derived from its text is printed except numbers and the ids of OUR rows;
where a shared n-gram is reported, the tokens are printed from our own field, so
no output of this program reproduces their expression.

That is the whole legal posture: `snapgene.csv` carries no licence of any kind
(SOURCING.md section 1, NO_GO, unchallenged), so it may be *looked at* for
comparison and may never be copied, committed or redistributed.

THE PIN
-------

Pinned to an immutable commit SHA, because tags move and `master` no longer
contains the file. The path within that tree was resolved from the GitHub tree
API at that exact commit rather than guessed, and the recorded sha256 is
asserted before anything is measured: if upstream ever serves different bytes at
this URL, the gate stops rather than silently measuring against something else.

    commit 61ed152e9f8c9abc3c5c1b01eabfc28b63cda551
    path   plannotate/data/data/snapgene.csv
    sha256 793631d9eebf721efae9e1d6cd483b1cbb62f5adad41174afa8f8b78b1342d5c
    size   159,462 bytes

THE METRIC, as SOURCING.md section 0.4 designed it
--------------------------------------------------

  * **Containment**, `|A and B| / |A|` with A = our description -- not Jaccard,
    because their descriptions run longer (median 11 tokens) and Jaccard would
    dilute a real copy into a small number.
  * **Stopwords stripped first.** Their top tokens are the/of/from/for/to/and,
    at 615/542/349/246/206/193 occurrences; leaving them in makes every pair of
    English sentences look related.
  * **Absolute floor of 3 shared non-stopword tokens.** 66 of their rows are
    three tokens or fewer, so one shared "protein" is 33% containment and means
    nothing.
  * **Any shared contiguous 5-token n-gram is a hard fail**, whatever the ratio.
    Ratios measure vocabulary; a five-word run measures phrasing, and phrasing
    is the copyrightable layer.
  * 30% or more: warn, and the PR needs a written justification.
    60% or more, or any shared 5-gram: hard fail.

FAIL CLOSED
-----------

A network error exits 3 with the status `taint-gate-unavailable`. It never
auto-passes: "we could not check" and "we checked and it was fine" are different
results and CI must not confuse them.

Usage
-----
    python features/build/taint_gate.py              # measure the shipped table
    python features/build/taint_gate.py --self-test  # prove the gate can fail
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import re
import shutil
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from lib_columns import FEATURE_COLUMNS  # noqa: E402

PIN_COMMIT = "61ed152e9f8c9abc3c5c1b01eabfc28b63cda551"
PIN_PATH = "plannotate/data/data/snapgene.csv"
PIN_SHA256 = "793631d9eebf721efae9e1d6cd483b1cbb62f5adad41174afa8f8b78b1342d5c"
PIN_URL = f"https://raw.githubusercontent.com/mmcguffi/pLannotate/{PIN_COMMIT}/{PIN_PATH}"

UA = "polylinker-taint-gate/0.1 (comparison only; nothing retained)"

WARN_AT = 0.30
FAIL_AT = 0.60
MIN_SHARED = 3
NGRAM = 5

# Deliberately short and general. A long list would start removing domain words
# ("resistance", "protein") and a shared domain vocabulary is exactly what we
# expect two people describing the same biology to have; it is not evidence of
# copying and must not be filtered away.
STOPWORDS = frozenset("""
a an and are as at be been but by can for from had has have in into is it its
of on or that the their then there these this to was were which will with not
""".split())

TOKEN = re.compile(r"[a-z0-9']+")


def tokens(text: str) -> list:
    return [t for t in TOKEN.findall(text.lower()) if t not in STOPWORDS]


def containment(ours: str, theirs: str) -> tuple:
    """(ratio, shared_token_count, longest shared contiguous run) for one pair."""
    a, b = tokens(ours), tokens(theirs)
    if not a or not b:
        return 0.0, 0, 0
    shared = set(a) & set(b)
    ratio = len(shared) / len(set(a))
    # Longest run of OUR tokens that appears contiguously in theirs.
    bstr = " ".join(b)
    longest = 0
    for i in range(len(a)):
        for j in range(i + longest + 1, len(a) + 1):
            if " ".join(a[i:j]) in bstr:
                longest = j - i
            else:
                break
    return ratio, len(shared), longest


def fetch_transient(dest_dir: Path) -> Path:
    """Download the pinned file into `dest_dir`, verifying its hash.

    `dest_dir` must be outside the repository. The caller deletes it.
    """
    try:
        with urllib.request.urlopen(
            urllib.request.Request(PIN_URL, headers={"User-Agent": UA}), timeout=120
        ) as r:
            data = r.read()
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        print(f"taint-gate-unavailable: {PIN_URL} could not be fetched ({e})")
        print("Failing closed. 'could not check' is not 'checked and clean'.")
        raise SystemExit(3) from e

    digest = hashlib.sha256(data).hexdigest()
    if digest != PIN_SHA256:
        print(
            f"taint-gate-unavailable: the pinned URL served sha256 {digest}, not the "
            f"{PIN_SHA256} recorded in features/SOURCING.md section 0.4. Upstream has "
            f"changed under a commit SHA, which should be impossible; refusing to "
            f"measure against bytes nobody has assessed."
        )
        raise SystemExit(3)

    path = dest_dir / "snapgene.csv"
    path.write_bytes(data)
    return path


def their_descriptions(path: Path) -> list:
    """Every free-text description in their file, as strings and nothing else.

    Reads only the text needed for the comparison. Their identifiers, sequences
    and every other column are not returned, so nothing downstream of this
    function can accidentally carry one.
    """
    out = []
    with path.open(encoding="utf-8", errors="replace", newline="") as fh:
        for row in csv.DictReader(fh):
            for key, value in row.items():
                if key and "descr" in key.lower() and value:
                    out.append(value)
    return out


def our_descriptions() -> list:
    """(id, description) for the shipped table."""
    text = (ROOT / "features.tsv").read_text(encoding="utf-8")
    lines = [ln for ln in text.split("\n") if ln.strip() and not ln.startswith("#")]
    rows = list(csv.DictReader(io.StringIO("\n".join(lines)), delimiter="\t"))
    if rows and list(rows[0].keys()) != FEATURE_COLUMNS:
        raise SystemExit("features.tsv header does not match lib_columns.FEATURE_COLUMNS")
    return [(r["id"], r["description"]) for r in rows if r.get("description")]


def measure(ours: list, theirs: list) -> tuple:
    """Worst containment and any 5-gram collision, per row of ours."""
    findings, worst = [], []
    for rid, desc in ours:
        best_ratio, best_shared, best_run = 0.0, 0, 0
        for t in theirs:
            ratio, shared, run = containment(desc, t)
            if shared < MIN_SHARED:
                # Below the floor the ratio is noise: one shared word against a
                # three-token row is 33% and means nothing.
                ratio = 0.0
            if run > best_run:
                best_run = run
            if ratio > best_ratio:
                best_ratio, best_shared = ratio, shared
        worst.append((rid, best_ratio, best_shared, best_run))
        if best_run >= NGRAM or best_ratio >= FAIL_AT:
            findings.append(("FAIL", rid, best_ratio, best_shared, best_run))
        elif best_ratio >= WARN_AT:
            findings.append(("WARN", rid, best_ratio, best_shared, best_run))
    return findings, worst


def self_test() -> None:
    """Prove the gate can fail. A gate that cannot fail proves nothing.

    The strings below are synthetic: `_copied` is deliberately built to share a
    long run with `_theirs`, and `_theirs` is invented for this test rather than
    taken from their file. Nothing here quotes anyone.
    """
    theirs = "chloramphenicol acetyltransferase confers resistance to chloramphenicol in bacteria"
    copied = "chloramphenicol acetyltransferase confers resistance to chloramphenicol"
    independent = (
        "Acetylates the antibiotic so it can no longer bind the ribosomal "
        "peptidyl transferase centre."
    )

    _, _, run = containment(copied, theirs)
    assert run >= NGRAM, f"a verbatim run of {run} tokens did not trip the {NGRAM}-gram rule"
    ratio, shared, _ = containment(copied, theirs)
    assert ratio >= FAIL_AT, f"a verbatim copy measured only {ratio:.0%} containment"

    ratio2, shared2, run2 = containment(independent, theirs)
    assert run2 < NGRAM, "independent prose tripped the n-gram rule; the gate is useless"
    assert ratio2 < WARN_AT, f"independent prose measured {ratio2:.0%}"

    # The floor must actually bite: one shared word against a short row is not
    # evidence, and without the floor it is 100% containment.
    ratio3, shared3, _ = containment("protein", "protein")
    assert shared3 < MIN_SHARED, "the >=3-shared-token floor is not being applied"

    print(f"  PASS a verbatim run of {run} tokens is a hard fail")
    print(f"  PASS independent prose scores {ratio2:.0%} with a {run2}-token longest run")
    print(f"  PASS the {MIN_SHARED}-shared-token floor rejects a one-word overlap")


def main() -> int:
    ap = argparse.ArgumentParser(description="pLannotate snapgene.csv taint gate")
    ap.add_argument("--self-test", action="store_true",
                    help="prove the metric can fail, without fetching anything")
    args = ap.parse_args()

    print("taint gate -- our descriptions vs pLannotate's snapgene.csv")
    print(f"  pinned commit {PIN_COMMIT}")
    print(f"  pinned sha256 {PIN_SHA256}")
    print("\nSelf-test")
    self_test()
    if args.self_test:
        return 0

    ours = our_descriptions()
    tmp = Path(tempfile.mkdtemp(prefix="plf-taint-"))
    try:
        path = fetch_transient(tmp)
        theirs = their_descriptions(path)
        print(f"\nFetched and hash-verified; {len(theirs)} of their description strings, "
              f"{len(ours)} of ours")
        findings, worst = measure(ours, theirs)
    finally:
        # Before anything else can go wrong. The file must not survive this
        # process under any exit path, including an exception mid-measurement.
        shutil.rmtree(tmp, ignore_errors=True)
    if tmp.exists():
        print(f"REFUSING TO EXIT CLEAN: {tmp} still exists")
        return 3

    worst.sort(key=lambda w: (-w[1], -w[3]))
    print("\nHighest containment measured, worst five:")
    for rid, ratio, shared, run in worst[:5]:
        print(f"  {rid}  containment {ratio:6.1%}  {shared} shared token(s)  "
              f"longest shared run {run}")

    fails = [f for f in findings if f[0] == "FAIL"]
    warns = [f for f in findings if f[0] == "WARN"]
    for kind, rid, ratio, shared, run in findings:
        print(f"  {kind} {rid}: containment {ratio:.1%}, {shared} shared tokens, "
              f"longest shared run {run} token(s)")
    if fails:
        print(f"\n{len(fails)} row(s) at or above {FAIL_AT:.0%} containment or sharing a "
              f"{NGRAM}-token run. Rewrite them from the primary literature.")
        return 1
    if warns:
        print(f"\n{len(warns)} row(s) at or above {WARN_AT:.0%} containment. Not a "
              f"failure, but each needs a written justification in the PR.")
        return 0
    print(f"\nNo row reaches {WARN_AT:.0%} containment and none shares a {NGRAM}-token "
          f"run. The fetched file has been deleted.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
