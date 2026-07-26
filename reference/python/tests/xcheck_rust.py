"""Cross-check the Rust implementation against the Python reference.

Two independently written parsers of the same undocumented format, compared
field by field on real files. Agreement is meaningful evidence; a single
implementation agreeing with itself is not.

Usage:
    python xcheck_rust.py <path-to-pl-binary> "<glob of .dna files>"
"""

import glob
import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import snapdna  # noqa: E402


def rust_info(binary, paths):
    """Run `pl info --json` over a batch and return {path: record}."""
    out = subprocess.run(
        [binary, "info", "--json", *paths],
        capture_output=True, text=True, encoding="utf-8",
    )
    if out.returncode != 0:
        sys.exit(f"pl info failed:\n{out.stderr[:2000]}")
    return {rec["file"]: rec for rec in json.loads(out.stdout)}


def compare(path, rust, doc):
    problems = []

    if rust.get("error"):
        return [f"rust refused the file: {rust['error']}"]

    if rust["bp"] != doc.length:
        problems.append(f"length {rust['bp']} vs {doc.length}")
    if rust["circular"] != doc.is_circular:
        problems.append(f"topology {rust['circular']} vs {doc.is_circular}")
    if rust["n_features"] != len(doc.features):
        problems.append(f"features {rust['n_features']} vs {len(doc.features)}")
    if rust["n_primers"] != len(doc.primers):
        problems.append(f"primers {rust['n_primers']} vs {len(doc.primers)}")

    py_sites = sum(len(p.binding_sites) for p in doc.primers)
    if rust["n_binding_sites"] != py_sites:
        problems.append(f"binding sites {rust['n_binding_sites']} vs {py_sites}")

    # Feature-by-feature, in document order.
    strand_of = {1: "+", 2: "-", 3: "both"}
    for i, (r, p) in enumerate(zip(rust["features"], doc.features)):
        if r["name"] != p.name:
            problems.append(f"feature {i} name {r['name']!r} vs {p.name!r}")
            break
        if r["kind"] != p.type:
            problems.append(f"feature {i} {p.name!r} kind {r['kind']!r} vs {p.type!r}")
            break
        if (r["start"], r["end"]) != (p.start, p.end):
            problems.append(
                f"feature {i} {p.name!r} span {r['start']}..{r['end']} "
                f"vs {p.start}..{p.end}")
            break
        if r["segments"] != len(p.segments):
            problems.append(
                f"feature {i} {p.name!r} segments {r['segments']} vs {len(p.segments)}")
            break
        want = strand_of.get(p.directionality, "none")
        if r["strand"] != want:
            problems.append(f"feature {i} {p.name!r} strand {r['strand']} vs {want}")
            break

    lower = sum(1 for c in doc.sequence if c.islower())
    if rust["lowercase"] != lower:
        problems.append(f"lowercase bases {rust['lowercase']} vs {lower}")

    return problems


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    # Absolute: see the note in xcheck_seguid.py.
    binary, pattern = os.path.abspath(sys.argv[1]), sys.argv[2]

    files = sorted(f for f in glob.glob(pattern, recursive=True) if os.path.isfile(f))
    if not files:
        sys.exit("no files matched")

    # Batch to keep the command line short on Windows.
    rust = {}
    for i in range(0, len(files), 25):
        rust.update(rust_info(binary, files[i:i + 25]))

    agree = disagree = 0
    total_feat = total_bp = 0
    for f in files:
        rec = rust.get(f) or rust.get(f.replace("\\", "/"))
        if rec is None:
            print(f"  MISSING from rust output: {f}")
            disagree += 1
            continue
        try:
            doc = snapdna.load(f)
        except Exception as e:
            print(f"  python refused {os.path.basename(f)}: {e}")
            disagree += 1
            continue
        problems = compare(f, rec, doc)
        total_feat += len(doc.features)
        total_bp += doc.length
        if problems:
            disagree += 1
            print(f"  DIFFER  {os.path.basename(f)[:44]}")
            for p in problems:
                print(f"            {p}")
        else:
            agree += 1

    print(f"\n{'=' * 66}")
    print(f"files compared     : {len(files)}")
    print(f"agree              : {agree}")
    print(f"disagree           : {disagree}")
    print(f"features compared  : {total_feat:,}")
    print(f"bases compared     : {total_bp:,}")
    print("\ntwo independent implementations of an undocumented binary format")
    return 0 if disagree == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
