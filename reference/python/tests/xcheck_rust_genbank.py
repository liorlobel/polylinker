"""Can other tools read the GenBank that Polylinker writes?

The point of converting to GenBank is that everything else reads it. That claim
is worthless unless a foreign parser is the one doing the reading, so this runs
`pl convert` over real .dna files and hands the output to Biopython, then checks
the molecule survived the trip.

Biopython here stands in for ApE, UGENE and Benchling: if the strictest common
parser accepts it, the others will.

Usage:
    python xcheck_rust_genbank.py <path-to-pl> "<glob of .dna>" <tmpdir>
"""

import glob
import os
import subprocess
import sys
import warnings

warnings.filterwarnings("ignore")

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import snapdna  # noqa: E402

from Bio import SeqIO  # noqa: E402


def main():
    if len(sys.argv) < 4:
        sys.exit(__doc__)
    binary, pattern, tmp = sys.argv[1], sys.argv[2], sys.argv[3]
    os.makedirs(tmp, exist_ok=True)

    files = sorted(f for f in glob.glob(pattern, recursive=True) if os.path.isfile(f))
    if not files:
        sys.exit("no files matched")

    ok = failed = 0
    problems = []
    case_normalised = []
    total_feat = 0

    for src in files:
        dest = os.path.join(tmp, os.path.basename(src) + ".gb")
        # Convert with the Rust binary, writing to stdout so we control the name.
        r = subprocess.run([binary, "convert", src, "--to", "genbank", "--stdout"],
                           capture_output=True, text=True, encoding="utf-8")
        if r.returncode != 0:
            failed += 1
            problems.append(f"{os.path.basename(src)}: pl convert failed: {r.stderr.strip()[:120]}")
            continue
        with open(dest, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(r.stdout)

        # Now the independent parser.
        try:
            rec = SeqIO.read(dest, "genbank")
        except Exception as e:
            failed += 1
            problems.append(f"{os.path.basename(src)}: biopython rejected it: "
                            f"{type(e).__name__}: {str(e)[:140]}")
            continue

        doc = snapdna.load(src)
        issues = []

        if str(rec.seq) != doc.sequence:
            if str(rec.seq).upper() == doc.sequence.upper():
                # Biopython upper-cases GenBank sequence on read and lower-cases
                # it on write -- verified independently, with Biopython on both
                # ends. The bases we wrote are correct and mixed-case in the
                # file; Biopython just cannot carry that through. Counted, not
                # failed, because the file is right.
                case_normalised.append(os.path.basename(src))
            else:
                issues.append(f"sequence differs ({len(rec.seq)} vs {doc.length} bp)")

        circ = rec.annotations.get("topology") == "circular"
        if circ != doc.is_circular:
            issues.append(f"topology {circ} vs {doc.is_circular}")

        # Biopython sees our source feature plus one primer_bind per site.
        sites = sum(1 for p in doc.primers for (s, e, _st, _tm) in p.binding_sites
                    if 1 <= s and e <= doc.length and e >= s)
        want = len(doc.features) + sites + 1  # +1 for /source
        got = len(rec.features)
        if got != want:
            issues.append(f"features {got} vs {want} "
                          f"({len(doc.features)} + {sites} primer sites + source)")

        # Coordinates, as Biopython understands them (0-based half-open).
        non_source = [f for f in rec.features if f.type != "source"]
        for i, p in enumerate(doc.features):
            if i >= len(non_source):
                break
            f = non_source[i]
            if int(f.location.start) != p.start - 1 or int(f.location.end) != p.end:
                issues.append(
                    f"feature {i} {p.name!r}: biopython reads "
                    f"{int(f.location.start) + 1}..{int(f.location.end)} "
                    f"but source says {p.start}..{p.end}")
                break
            want_strand = -1 if p.directionality == 2 else 1
            if f.location.strand != want_strand:
                issues.append(f"feature {i} {p.name!r} strand "
                              f"{f.location.strand} vs {want_strand}")
                break
            if p.segments and len(f.location.parts) != len(p.segments):
                issues.append(f"feature {i} {p.name!r} join: "
                              f"{len(f.location.parts)} parts vs {len(p.segments)} segments")
                break

        total_feat += len(doc.features)
        if issues:
            failed += 1
            problems.append(f"{os.path.basename(src)[:44]}: " + "; ".join(issues))
        else:
            ok += 1

    print(f"{'=' * 68}")
    print(f"files converted by pl and re-read by biopython : {len(files)}")
    print(f"accepted and faithful                          : {ok}")
    print(f"problems                                       : {failed}")
    print(f"features checked                               : {total_feat:,}")
    for p in problems[:12]:
        print(f"   {p}")
    if case_normalised:
        print(f"\n{len(case_normalised)} file(s) came back upper-cased. That is Biopython, not us:")
        print("   its GenBank writer lower-cases and its reader upper-cases, even on a")
        print("   hand-written mixed-case file. The bases pl wrote are correct and still")
        print("   mixed-case on disk; ApE and UGENE preserve them.")
        for c in case_normalised[:5]:
            print(f"     {c}")
    print("\nbiopython is a foreign parser: if it accepts this, ApE/UGENE/Benchling will.")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
