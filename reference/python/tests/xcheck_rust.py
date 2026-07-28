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
import tempfile

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

    # Block 6, element by element, attributes included.
    #
    # This comparison did not exist, and the two implementations had disagreed
    # about that block for as long as both existed: Python's dict comprehension
    # kept `<Empty/>` (text None -> "") where Rust waited for a text event and
    # dropped it, Python collapsed a repeated tag to the last where Rust kept
    # every one, and neither carried the `UTC` attribute that is half of
    # `<Created UTC="22:0:0">2022.12.13</Created>`.  Nothing compared them, so
    # nothing said so -- which is the failure mode this whole script exists to
    # prevent.
    py_notes = [(n.key, n.value, sorted(n.attrs)) for n in doc.notes]
    rs_notes = [
        (n["name"], n["value"], sorted((a["name"], a["value"]) for a in n["attrs"]))
        for n in rust.get("notes", [])
    ]
    if py_notes != rs_notes:
        problems.append(f"notes {rs_notes} vs {py_notes}")
    # Compared exactly, which is why the two readers have to agree on *what*
    # goes in here and not merely that something does: Rust reported every
    # descendant with a full path while Python reported the note's direct
    # children only, so `<A><B><C/></B></A>` was a hard DIFFER in the one field
    # added to prove this fix against an independent implementation.  Both now
    # carry three spellings -- `Notes/A/B`, `Notes/A/text()`, `Notes@version` --
    # and the synthetic fixtures below exercise all three, since no real file
    # does.
    if sorted(rust.get("unrepresentable_notes", [])) != sorted(doc.unrepresentable_notes):
        problems.append(
            f"unrepresentable note parts {rust.get('unrepresentable_notes')} "
            f"vs {doc.unrepresentable_notes}")

    return problems


# Block 6 shapes that no real file on any machine here has, and on which the two
# implementations were found disagreeing the moment anything compared them.
#
# The corpus cannot reach these -- all 32 real payloads are flat or one level
# deep, none has mixed content, none has an attribute on <Notes> -- so a
# cross-check driven only by real files reports "agree 33 / disagree 0" and
# proves nothing about any of them.  Both readers are written to a contract, and
# a contract is exactly what a corpus cannot pin.  Generated rather than checked
# in as binaries so the bytes stay readable in the diff.
SYNTHETIC = {
    # Text on both sides of a nested child.  Rust concatenated every run at note
    # depth and answered "beforeafter"; ElementTree's .text answered "before".
    "mixed_content": "<Notes><A>before<B/>after</A></Notes>",
    # Three levels.  Python walked one and hard-coded a three-segment path.
    "deep_nesting": '<Notes><A><B><C x="1"/></B></A></Notes>',
    # An attribute on the root, which both used to drop in silence.
    "root_attribute": '<Notes version="3"><Type>Synthetic</Type></Notes>',
    # The shapes that already agreed, kept as controls: a regression that
    # "fixes" the three above by breaking these would otherwise pass.
    "attribute_and_empty": '<Notes><Created UTC="22:0:0">2022.12.13</Created><Empty/></Notes>',
    "repeated_tag": "<Notes><Comments>one</Comments><Comments>two</Comments></Notes>",
    "citation": '<Notes><References>\n<Reference pubMedID="9335267"/>\n</References></Notes>',
}


def _write_synthetic(dirname):
    """Write one .dna per SYNTHETIC entry and return their paths."""
    os.makedirs(dirname, exist_ok=True)
    header = b"SnapGene" + (1).to_bytes(2, "big") + (15).to_bytes(2, "big") \
        + (19).to_bytes(2, "big")
    paths = []
    for name, notes in SYNTHETIC.items():
        raw = bytearray()
        for kind, payload in ((9, header), (0, b"\x01ACGT"), (6, notes.encode())):
            raw.append(kind)
            raw += len(payload).to_bytes(4, "big")
            raw += payload
        p = os.path.join(dirname, f"synthetic_{name}.dna")
        with open(p, "wb") as fh:
            fh.write(raw)
        paths.append(p)
    return paths


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    # Absolute: see the note in xcheck_seguid.py.
    binary, pattern = os.path.abspath(sys.argv[1]), sys.argv[2]

    files = sorted(f for f in glob.glob(pattern, recursive=True) if os.path.isfile(f))
    if not files:
        sys.exit("no files matched")
    files += _write_synthetic(os.path.join(tempfile.gettempdir(), "pl-xcheck-synthetic"))

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
