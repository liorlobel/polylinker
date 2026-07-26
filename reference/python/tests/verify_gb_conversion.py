"""Verify that .dna -> GenBank conversion preserves what matters.

Reads each .dna with snapdna, reads the converted .gb back with Biopython, and
compares sequence, topology, feature count, per-feature coordinates, strand and
colour. Reports exactly what survives and what does not, because a converter
whose losses are undocumented is worse than one that loses nothing quietly.
"""
import glob
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import snapdna          # noqa: E402
from dna2gb import locus_name   # noqa: E402

from Bio import SeqIO   # noqa: E402


def load_manifest(gb_dir):
    """source .dna -> output .gb, as recorded by the converter."""
    path = os.path.join(gb_dir, "manifest.tsv")
    mapping = {}
    if os.path.exists(path):
        with open(path, encoding="utf-8") as fh:
            next(fh, None)
            for line in fh:
                parts = line.rstrip("\n").split("\t")
                if len(parts) == 2:
                    mapping[os.path.normcase(parts[0])] = parts[1]
    return mapping


def check(dna_path, gb_dir, manifest):
    doc = snapdna.load(dna_path)
    gb = manifest.get(os.path.normcase(os.path.abspath(dna_path)))
    if not gb:
        gb = os.path.join(gb_dir, locus_name(dna_path) + ".gb")
    if not os.path.exists(gb):
        return None, [f"no converted file at {gb}"]

    rec = SeqIO.read(gb, "genbank")
    problems = []

    if str(rec.seq).upper() != doc.sequence.upper():
        problems.append(f"sequence differs ({len(rec.seq)} vs {doc.length} bp)")

    topo = rec.annotations.get("topology", "linear")
    if (topo == "circular") != doc.is_circular:
        problems.append(f"topology lost: .dna={'circular' if doc.is_circular else 'linear'}, gb={topo}")

    # The converter emits every source feature first, in order, then appends one
    # primer_bind per detected binding site. Split by position, not by type --
    # a source feature may itself be typed primer_bind.
    n_src = len(doc.features)
    gb_feats = rec.features[:n_src]
    gb_primer_sites = rec.features[n_src:]
    if len(gb_feats) != n_src:
        problems.append(f"feature count {len(gb_feats)} != {n_src}")

    # coordinates and colour, feature by feature
    coord_bad = color_bad = strand_bad = 0
    for src, out in zip(doc.features, gb_feats):
        want_start = min(s.start for s in src.segments) - 1
        want_end = max(s.end for s in src.segments)
        if int(out.location.start) != want_start or int(out.location.end) != want_end:
            coord_bad += 1
        want_strand = -1 if src.directionality == 2 else 1
        if out.location.strand != want_strand:
            strand_bad += 1
        want_color = next((s.color for s in src.segments if s.color), None)
        got = out.qualifiers.get("ApEinfo_fwdcolor", [None])[0]
        if want_color and got != want_color:
            color_bad += 1
    if coord_bad:
        problems.append(f"{coord_bad} feature(s) with wrong coordinates")
    if strand_bad:
        problems.append(f"{strand_bad} feature(s) with wrong strand")
    if color_bad:
        problems.append(f"{color_bad} feature(s) lost colour")

    n_seg = sum(1 for f in doc.features if len(f.segments) > 1)
    n_join = sum(1 for f in gb_feats if len(f.location.parts) > 1)
    if n_seg != n_join:
        problems.append(f"multi-segment features: {n_seg} in .dna, {n_join} join() in gb")

    # Sites outside the sequence are intentionally skipped by the converter.
    src_sites = sum(1 for p in doc.primers for (s, e, _st, _tm) in p.binding_sites
                    if 1 <= s and e <= doc.length and e >= s)
    gb_sites = len(gb_primer_sites)
    if src_sites != gb_sites:
        problems.append(f"primer sites {gb_sites} != {src_sites}")

    return {
        "bp": doc.length,
        "feats": len(doc.features),
        "primers": len(doc.primers),
        "sites": src_sites,
    }, problems


def main(pattern, gb_dir):
    files = sorted(set(glob.glob(pattern, recursive=True)))
    manifest = load_manifest(gb_dir)
    clean = 0
    total_problems = 0
    lost_history = 0

    print(f"{'bp':>10} {'feat':>5} {'prim':>5} {'sites':>6}  status")
    for f in files:
        info, problems = check(f, gb_dir, manifest)
        if info is None:
            print(f"{'--':>10} {'--':>5} {'--':>5} {'--':>6}  {os.path.basename(f)[:38]}: {problems[0]}")
            total_problems += 1
            continue
        doc = snapdna.load(f)
        if doc.history_xml:
            lost_history += 1
        if problems:
            total_problems += len(problems)
            print(f"{info['bp']:>10,} {info['feats']:>5} {info['primers']:>5} {info['sites']:>6}  "
                  f"{os.path.basename(f)[:34]}\n            !! {'; '.join(problems)}")
        else:
            clean += 1
            print(f"{info['bp']:>10,} {info['feats']:>5} {info['primers']:>5} {info['sites']:>6}  "
                  f"ok  {os.path.basename(f)[:34]}")

    print("\n" + "=" * 72)
    print(f"faithful conversions : {clean}/{len(files)}")
    print(f"problems             : {total_problems}")
    print(f"\nKnown, accepted losses (not counted as problems):")
    print(f"  cloning history trees : {lost_history} file(s) had one; GenBank has no equivalent")
    print(f"  derived enzyme caches : dropped by design, regenerable")
    print(f"  methylation flags     : Dam/Dcm/EcoKI state has no GenBank field")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
