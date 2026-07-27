"""Check that a rendered chromatogram shows what the file says, on real traces.

    python xcheck_trace_render.py target/release/pl.exe '<glob of .ab1>' [tmpdir]

There is no external tool that renders ABIF to compare against, so this asserts
the property the picture has to have, read back out of the SVG itself:

  **at each called base, the curve drawn in that base's colour is the tallest
  of the four.**

That statement exercises the whole path end to end on real data: a channel
mapped to the wrong base, a letter placed at the wrong x, or peak positions
read from the wrong tag all break it, and all three otherwise produce a picture
that still looks like a perfectly ordinary chromatogram. Measured: swapping two
channels drops the agreement rate to 49.08%.

**What this check cannot see, stated plainly.** All 374 ABIF files on the drive
it runs against carry `FWO_ = GATC`, so hard-coding that order instead of
reading `FWO_` is *indistinguishable* here — injecting exactly that bug changed
the result by nothing at all. A previous version of this docstring claimed the
check would catch it and put a number on it; the number was invented and the
claim was false. The guard for `FWO_` is the unit test
`every_base_is_drawn_in_the_colour_its_own_channel_says`, which builds
synthetic traces in three different channel orders because no file here has
anything but one.

The check runs over the *reliable* part of each read only. Sanger traces are
genuinely ambiguous at the ends — overlapping peaks, and a basecaller reporting
Q4 because it is guessing — so a disagreement there is the chemistry, not the
renderer. Biopython supplies the quality values used to decide that, and the
number of bases skipped is reported rather than quietly dropped.

Exits 1 if the agreement rate falls below THRESHOLD, and on comparing nothing.
"""
import glob
import os
import re
import subprocess
import sys
import warnings

from Bio import SeqIO, BiopythonParserWarning

warnings.simplefilter("ignore", BiopythonParserWarning)

# Well below what a working renderer scores (99.94% on this corpus) and far
# above what a broken one reaches: swapping two channels scores 49.08%.
THRESHOLD = 0.97
MIN_QUALITY = 30

COLORS = {"#10a010": "A", "#1030d0": "C", "#101010": "G", "#d01010": "T"}


def render(exe, path, out):
    r = subprocess.run([exe, "trace", path, "--svg", out],
                       capture_output=True, text=True)
    return r.returncode == 0 and os.path.exists(out)


def parse_svg(text):
    """(curves by base, letters) from the rendered SVG."""
    curves = {}
    for m in re.finditer(r'<path d="([^"]*)"[^>]*stroke="(#[0-9a-f]{6})"', text):
        d, color = m.group(1), m.group(2)
        if color not in COLORS:
            continue  # a quality bar, which is filled and not stroked
        pts = [(float(x), float(y)) for x, y in
               re.findall(r'[ML]\s*(-?[\d.]+)[ ,]+(-?[\d.]+)', d)]
        if pts:
            curves[COLORS[color]] = pts
    letters = []
    for m in re.finditer(
            r'<text x="(-?[\d.]+)" y="-?[\d.]+"[^>]*fill="(#[0-9a-f]{6})"[^>]*>'
            r'([ACGTN])</text>', text):
        letters.append((float(m.group(1)), m.group(3)))
    return curves, letters


def y_at(pts, x):
    """The curve's height at an x, by nearest sample."""
    best, by = None, None
    for px, py in pts:
        d = abs(px - x)
        if best is None or d < best:
            best, by = d, py
    return by


def main(argv):
    exe = None
    if argv and os.path.isfile(argv[0]):
        exe, argv = os.path.abspath(argv[0]), argv[1:]
    if exe is None:
        print("usage: xcheck_trace_render.py <pl.exe> '<glob>' [tmpdir]")
        return 1
    tmp = argv[-1] if argv and os.path.isdir(argv[-1]) else "."
    pats = argv[:-1] if tmp != "." else argv

    files = []
    for p in pats:
        files.extend(glob.glob(p, recursive=True))
    files = sorted(set(files))[:40]

    out = os.path.join(tmp, "_xcheck_trace.svg")
    checked = agree = skipped_low_q = 0
    traces = 0
    bad = []
    for f in files:
        try:
            rec = SeqIO.read(f, "abi")
        except Exception:
            continue  # an SCF or ZTR wearing an .ab1 name; xcheck_abif covers those
        qual = rec.letter_annotations.get("phred_quality") or []
        if not render(exe, f, out):
            bad.append((f, "did not render"))
            continue
        with open(out, encoding="utf8") as fh:
            curves, letters = parse_svg(fh.read())
        if len(curves) != 4 or not letters:
            bad.append((f, f"expected 4 curves and some letters, got "
                           f"{len(curves)} and {len(letters)}"))
            continue
        traces += 1
        for i, (x, base) in enumerate(letters):
            if base not in curves:
                continue
            if i >= len(qual) or qual[i] < MIN_QUALITY:
                skipped_low_q += 1
                continue
            checked += 1
            # Lower y is taller: the scene's origin is top-left.
            mine = y_at(curves[base], x)
            if all(mine <= y_at(curves[b], x) for b in "ACGT" if b != base):
                agree += 1
    if os.path.exists(out):
        os.remove(out)

    rate = agree / checked if checked else 0.0
    print("=" * 74)
    print(f"chromatograms rendered : {traces}")
    print(f"bases checked          : {checked:,}")
    print(f"tallest curve is the called base: {agree:,}  ({rate:.2%})")
    print(f"skipped below Q{MIN_QUALITY}         : {skipped_low_q:,}  "
          f"(ambiguous chemistry, not the renderer)")
    print()
    print("No external renderer exists to compare against, so the picture is")
    print("read back and asserted to agree with the file: the curve drawn in")
    print("each called base's colour must be the tallest one there. Swapping")
    print("two channels scores 49%. Note that every file here has FWO_=GATC,")
    print("so this cannot see a hard-coded channel order -- a unit test does.")

    for f, why in bad[:5]:
        print(f"\n  {os.path.basename(f)}: {why}")

    if traces == 0 or checked == 0:
        print("\nFAIL: checked nothing")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} file(s) did not render")
        return 1
    if rate < THRESHOLD:
        print(f"\nFAIL: {rate:.2%} is below the {THRESHOLD:.0%} floor")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
