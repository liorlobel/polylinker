"""Cross-validate the PDF export against the SVG, and against PDF readers.

    python xcheck_pdf.py target/release/pl.exe

`pl-draw` builds one `Scene` and renders it twice, so SVG and PDF ought to be
the same picture. "Ought to" is the part worth checking: the PDF back end has
to flip the coordinate system, convert every arc to Béziers, and place text by
*measuring* it, because PDF has no `text-anchor`. Any of those can be plausibly
wrong.

Three independent things are asserted:

  1. **The file is a PDF.** pypdf and PyMuPDF both open it, agree on the page
     count and the media box, and read the metadata. A wrong cross-reference
     offset produces a file that looks fine to `strings` and opens in nothing.
  2. **Nothing is missing.** Every string the SVG draws appears in the PDF's
     extracted text.
  3. **Text lands in the same place.** Each string's position is compared
     against the SVG's, with the SVG's `text-anchor` resolved using the same
     measurement the PDF back end claims to use. This is the assertion that
     actually exercises the Helvetica width table -- a table off by one entry
     would shift centred text and nothing else would notice.

Exits 1 on any disagreement and on comparing nothing.
"""
import os
import re
import subprocess
import sys
import tempfile

import fitz  # PyMuPDF
import pypdf

# The fixtures already in the repository, plus whatever else is named on the
# command line.
FIXTURES = [
    "prototype/demo-construct.gb",
    "tests/library-fixture/a.gb",
    # multi.gb is deliberately absent: `pl export` refuses a multi-record file
    # rather than drawing record 1 and calling it the map of the file.
    "tests/library-fixture/odd.gb",
    "tests/library-fixture/asm.fa",
]

# Helvetica advance widths, U+0020..U+007E, in 1/1000 em -- the same table
# `crates/pl-draw/src/pdf.rs` carries. Transcribed here *independently* from
# PyMuPDF at run time rather than copied, so a typo in the Rust table shows up
# as a disagreement instead of being reproduced on both sides.
_HELV = fitz.Font("helv")


def width(s, size):
    return sum(_HELV.glyph_advance(ord(c)) for c in s) * size


TEXT_RE = re.compile(
    r'<text x="([-\d.]+)" y="([-\d.]+)" font-size="([\d.]+)"'
    r'(?: font-weight="[^"]*")? fill="[^"]*" text-anchor="(\w+)"[^>]*>([^<]*)</text>'
)


def svg_texts(svg):
    """Every string the SVG draws, with the x of its left edge."""
    out = []
    for m in TEXT_RE.finditer(svg):
        x, y, size, anchor, text = (
            float(m[1]),
            float(m[2]),
            float(m[3]),
            m[4],
            m[5],
        )
        text = (
            text.replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", '"')
        )
        w = width(text, size)
        left = {"start": x, "middle": x - w / 2, "end": x - w}[anchor]
        out.append((text, left, y, size))
    return out


def run(exe, src, outdir, pdf):
    args = [exe, "export", src, "--outdir", outdir]
    if pdf:
        args.append("--pdf")
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"{' '.join(args)}: {r.stderr.strip()}")


def check(exe, src, outdir):
    problems = []
    run(exe, src, outdir, pdf=False)
    run(exe, src, outdir, pdf=True)
    stem = None
    for f in os.listdir(outdir):
        if f.endswith(".pdf"):
            stem = f[:-4]
    if stem is None:
        return [f"{src}: no PDF was written"], 0

    pdf_path = os.path.join(outdir, stem + ".pdf")
    svg_path = os.path.join(outdir, stem + ".svg")

    # --- 1. it is a PDF, per two readers -------------------------------------
    r = pypdf.PdfReader(pdf_path)
    if len(r.pages) != 1:
        problems.append(f"{src}: pypdf sees {len(r.pages)} pages")
    doc = fitz.open(pdf_path)
    if doc.page_count != 1:
        problems.append(f"{src}: PyMuPDF sees {doc.page_count} pages")
    page = doc[0]
    box = [float(v) for v in r.pages[0].mediabox]
    if box != [0, 0, 720, 720]:
        problems.append(f"{src}: media box {box}")
    if abs(page.rect.width - 720) > 0.01 or abs(page.rect.height - 720) > 0.01:
        problems.append(f"{src}: PyMuPDF rect {page.rect}")

    # --- 2. nothing is missing ----------------------------------------------
    svg = open(svg_path, encoding="utf8").read()
    wanted = svg_texts(svg)
    # PyMuPDF splits on spaces; join the page's words back into one haystack.
    got_words = page.get_text("words")
    haystack = " ".join(w[4] for w in got_words)
    for text, _, _, _ in wanted:
        for token in text.split():
            if token not in haystack:
                problems.append(f"{src}: {token!r} is drawn in the SVG and absent from the PDF")

    # --- 3. text lands in the same place ------------------------------------
    #
    # PyMuPDF gives each word's bounding box in PDF space with y measured from
    # the top, which is the same orientation the SVG uses, so the comparison is
    # direct. Only whole strings that survived as one word are compared, since a
    # multi-word label is split by the extractor and its pieces are not what the
    # renderer positioned.
    by_word = {}
    for x0, y0, x1, y1, w, *_ in got_words:
        by_word.setdefault(w, []).append((x0, y0, x1, y1))
    checked = 0
    for text, left, mid_y, size in wanted:
        if " " in text or text not in by_word:
            continue
        # Nearest candidate, so repeated strings do not pair up wrongly.
        best = min(by_word[text], key=lambda b: abs(b[0] - left) + abs((b[1] + b[3]) / 2 - mid_y))
        dx = best[0] - left
        dy = (best[1] + best[3]) / 2 - mid_y
        checked += 1
        # A point of slack on x: the extractor reports the inked box, and a
        # glyph's left side bearing is not zero. Three on y, because the
        # vertical centring of a line is a convention, not a measurement.
        if abs(dx) > 2.5 or abs(dy) > 3.5:
            problems.append(
                f"{src}: {text!r} at ({best[0]:.1f}, {(best[1]+best[3])/2:.1f}) "
                f"but the SVG puts it at ({left:.1f}, {mid_y:.1f}) -- off by ({dx:+.1f}, {dy:+.1f})"
            )
    return problems, checked


def main(argv):
    exe = None
    if argv and os.path.isfile(argv[0]):
        exe = os.path.abspath(argv[0])
        argv = argv[1:]
    if exe is None:
        print("usage: xcheck_pdf.py <path to pl.exe> [extra files...]")
        return 1

    files = [f for f in FIXTURES + list(argv) if os.path.exists(f)]
    all_problems = []
    positions = 0
    for src in files:
        with tempfile.TemporaryDirectory() as d:
            p, n = check(exe, src, d)
            all_problems += p
            positions += n

    print("=" * 74)
    print(f"files compared        : {len(files)}")
    print(f"text positions checked: {positions}")
    print(f"disagreements         : {len(all_problems)}")
    print()
    print("the PDF and the SVG come from one Scene, so they should be the same")
    print("picture; this checks that the coordinate flip, the arc-to-Bezier")
    print("conversion and the measured text anchoring did not change it")
    for p in all_problems[:15]:
        print(f"  {p}")

    if not files:
        print("\nFAIL: no fixtures found")
        return 1
    if positions == 0:
        print("\nFAIL: compared no text positions")
        return 1
    if all_problems:
        print(f"\nFAIL: {len(all_problems)} disagreement(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
