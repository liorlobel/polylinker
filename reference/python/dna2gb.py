"""Convert SnapGene .dna files to GenBank -- in bulk, losslessly enough to matter.

This is the data-liberation step. GenBank is plain text, is read by every free
tool in existence (ApE, UGENE, Benchling, Biopython, Geneious), and is read
back by SnapGene itself, so nothing is burned by converting.

Feature colours are written in BOTH conventions so the output looks right
wherever it lands:
  /ApEinfo_fwdcolor + /ApEinfo_revcolor   -- ApE, UGENE, and SnapGene all read these
  /note="color: #rrggbb"                  -- Benchling and several web viewers

Usage
-----
    python dna2gb.py "C:/path/to/**/*.dna" -o converted/
    python dna2gb.py plasmid.dna                 # writes plasmid.gb alongside
    python dna2gb.py "*.dna" --stdout            # pipe it somewhere

Requires Biopython only for writing (pip install biopython). The .dna reading
is done by snapdna.py, which has no dependencies at all.

Known limitation: sequence case
-------------------------------
Biopython's GenBank writer lower-cases the sequence, and its reader upper-cases
it -- verified with Biopython on both ends of a round-trip, including a
hand-written mixed-case file. So this converter loses the distinction between
upper- and lowercase bases, which in practice marks soft-masked or low-coverage
assembly regions and non-annealing primer tails. Seven contigs in the reference
corpus carry such bases.

Everything else round-trips faithfully. If case matters to you, use the Rust
implementation instead, which writes the ORIGIN block itself and preserves it:

    pl convert plasmid.dna --to genbank
"""

from __future__ import annotations

import argparse
import glob
import os
import re
import sys
import datetime

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import snapdna  # noqa: E402

try:
    from Bio.Seq import Seq
    from Bio.SeqRecord import SeqRecord
    from Bio.SeqFeature import SeqFeature, SimpleLocation, CompoundLocation
    from Bio import SeqIO
except ImportError:
    sys.exit("This converter needs Biopython for GenBank output:\n"
             "    pip install biopython")


# GenBank LOCUS names cannot contain spaces and are conventionally short.
def locus_name(path: str) -> str:
    stem = os.path.splitext(os.path.basename(path))[0]
    stem = re.sub(r"[^A-Za-z0-9_.-]+", "_", stem).strip("_")
    return (stem or "sequence")[:20]


def to_record(doc: snapdna.SnapDnaDocument, name: str) -> SeqRecord:
    rec = SeqRecord(
        Seq(doc.sequence),
        id=name,
        name=name,
        description=doc.notes.get("Description", "").strip() or name,
    )
    rec.annotations["molecule_type"] = "DNA"
    rec.annotations["topology"] = "circular" if doc.is_circular else "linear"
    rec.annotations["data_file_division"] = "SYN"
    created = doc.notes.get("Created", "")
    rec.annotations["date"] = _gb_date(created)

    if doc.notes.get("UUID"):
        rec.annotations["comment"] = (
            f"Converted from SnapGene .dna by polylinker/dna2gb.\n"
            f"Original document UUID: {doc.notes['UUID']}"
        )

    n = len(doc.sequence)

    for f in doc.features:
        loc = _location(f, n)
        if loc is None:
            continue
        quals = {"label": [f.name]}
        # Carry through the qualifiers SnapGene stored.
        for k, v in (f.qualifiers or {}).items():
            if k == "label":
                continue
            quals.setdefault(k, []).append(str(v))
        color = next((s.color for s in f.segments if s.color), None)
        if color:
            quals["ApEinfo_fwdcolor"] = [color]
            quals["ApEinfo_revcolor"] = [color]
            quals.setdefault("note", []).append(f"color: {color}")
        rec.features.append(SeqFeature(loc, type=f.type or "misc_feature",
                                       qualifiers=quals))

    # Primers become primer_bind features at each detected binding site, which
    # is how GenBank represents them and how other tools will show them.
    for p in doc.primers:
        for (start, end, strand, tm) in p.binding_sites:
            if start < 1 or end > n or end < start:
                continue
            quals = {
                "label": [p.name],
                "note": [f"primer {p.sequence}"] + ([f"Tm: {tm} C"] if tm else []),
            }
            rec.features.append(SeqFeature(
                SimpleLocation(start - 1, end, strand=-1 if strand else 1),
                type="primer_bind", qualifiers=quals))

    return rec


def _gb_date(created: str) -> str:
    """SnapGene stores 'YYYY.MM.DD'; GenBank wants 'DD-MON-YYYY'."""
    m = re.match(r"(\d{4})\.(\d{1,2})\.(\d{1,2})", created or "")
    if m:
        try:
            d = datetime.date(int(m.group(1)), int(m.group(2)), int(m.group(3)))
            return d.strftime("%d-%b-%Y").upper()
        except ValueError:
            pass
    return datetime.date.today().strftime("%d-%b-%Y").upper()


def _location(f, n):
    """Map 1-based inclusive segments onto Biopython's 0-based half-open model."""
    strand = -1 if f.directionality == 2 else 1
    parts = []
    for s in f.segments:
        start, end = s.start - 1, s.end
        if start < 0 or end > n or end <= start:
            continue
        parts.append(SimpleLocation(start, end, strand=strand))
    if not parts:
        return None
    if len(parts) == 1:
        return parts[0]
    if strand == -1:
        parts.reverse()
    return CompoundLocation(parts)


def convert(path: str, outdir: str | None, to_stdout: bool, claimed: set | None = None):
    """Convert one file.

    `claimed` tracks output paths already written during this run. Different
    source files can share a basename -- collecting a lab's plasmids into one
    output directory makes that likely -- and silently overwriting one with
    another is data loss. Collisions get a numeric suffix instead.
    """
    doc = snapdna.load(path)
    name = locus_name(path)
    rec = to_record(doc, name)

    if to_stdout:
        SeqIO.write(rec, sys.stdout, "genbank")
        return None, doc, rec, False

    dest_dir = outdir or os.path.dirname(os.path.abspath(path))
    os.makedirs(dest_dir, exist_ok=True)

    dest = os.path.join(dest_dir, name + ".gb")
    renamed = False
    if claimed is not None and dest in claimed:
        n = 2
        while os.path.join(dest_dir, f"{name}-{n}.gb") in claimed:
            n += 1
        dest = os.path.join(dest_dir, f"{name}-{n}.gb")
        renamed = True
    if claimed is not None:
        claimed.add(dest)

    with open(dest, "w", encoding="utf-8") as fh:
        SeqIO.write(rec, fh, "genbank")
    return dest, doc, rec, renamed


def main():
    ap = argparse.ArgumentParser(description="Convert SnapGene .dna files to GenBank.")
    ap.add_argument("patterns", nargs="+", help="files or globs (use quotes; ** works)")
    ap.add_argument("-o", "--outdir", help="output directory (default: alongside each input)")
    ap.add_argument("--stdout", action="store_true", help="write to stdout instead of files")
    args = ap.parse_args()

    files = []
    for p in args.patterns:
        files.extend(glob.glob(p, recursive=True))
    files = sorted(set(f for f in files if os.path.isfile(f)))

    if not files:
        sys.exit("No files matched. Remember to quote globs on Windows.")

    if not args.stdout:
        print(f"{'bp':>10} {'top':>5} {'feat':>5} {'prim':>5}  output")

    ok = failed = renamed_count = 0
    claimed: set[str] = set()
    manifest: list[tuple[str, str]] = []
    for f in files:
        try:
            dest, doc, rec, renamed = convert(f, args.outdir, args.stdout, claimed)
        except Exception as e:
            failed += 1
            print(f"  FAILED  {os.path.basename(f)}: {e}", file=sys.stderr)
            continue
        ok += 1
        renamed_count += bool(renamed)
        if dest:
            manifest.append((os.path.abspath(f), dest))
        if not args.stdout:
            n_feat = len(doc.features)
            n_sites = len(rec.features) - n_feat
            print(f"{len(doc.sequence):>10,} "
                  f"{'circ' if doc.is_circular else 'lin':>5} "
                  f"{n_feat:>5} {n_sites:>5}  {dest}"
                  f"{'   [renamed: basename collision]' if renamed else ''}")

    if manifest and args.outdir:
        mpath = os.path.join(args.outdir, "manifest.tsv")
        with open(mpath, "w", encoding="utf-8") as fh:
            fh.write("source_dna\toutput_gb\n")
            for src, dst in manifest:
                fh.write(f"{src}\t{dst}\n")
        print(f"\nmanifest: {mpath}")

    if not args.stdout:
        print(f"\nconverted {ok} file(s), {failed} failed")
        if renamed_count:
            print(f"{renamed_count} output file(s) were given a numeric suffix because "
                  f"two inputs shared a basename -- nothing was overwritten.")
        if ok:
            print("GenBank is plain text and is read by ApE, UGENE, Benchling, "
                  "Biopython and SnapGene itself.")


if __name__ == "__main__":
    main()
