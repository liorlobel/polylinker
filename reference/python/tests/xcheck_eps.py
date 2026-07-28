r"""Check the EPS emitter against the PDF emitter, and against PostScript itself.

    python xcheck_eps.py target/release/pl.exe <plasmid file>...

**No PostScript interpreter is installed on this machine** — no Ghostscript, and
MuPDF refuses EPS — so this does not prove that a renderer draws the file. It is
worth saying plainly, because "the EPS oracle passes" would otherwise sound like
more than it is. What it does prove is two things that between them catch every
mistake this emitter can make on its own:

**1. The geometry agrees with the PDF, exactly.** EPS and PDF are separately
written emitters over the same `Scene`, and both turn a centre-form arc into
Béziers through the same routine, so their coordinate streams must be
point-for-point identical after the y-flip. The two formats have opposite
y-axes — PostScript's origin is bottom-left, PDF's is bottom-left too, while the
scene's is top-left — so an emitter that flips once too many or once too few
produces a figure that is upside-down and otherwise perfect. That survives a
glance at a thumbnail and is caught here.

**2. The PostScript is structurally sound.** Balanced `gsave`/`grestore`,
balanced and correctly escaped string literals, only Level-2 operators, and a
`%%BoundingBox` that actually contains every coordinate emitted. A bounding box
smaller than the artwork crops the figure silently — in the typesetter's hands
rather than on screen — which is the single most common way an EPS goes wrong.

That last claim used to be false as written, and the wording is what gave it
away. The box was compared against `eps_tokens`, and `eps_tokens` drops every
line containing ` show` — which is every label, because the emitter writes each
one as a single `... X Y moveto (text) show` line. So *no text coordinate was
ever tested against the box*. It is not a hypothetical gap: `pl_draw`'s label
gutter is capped at 30% of the canvas, so an ordinary GenBank file with feature
names over roughly 28 characters overflows it. Measured on the shipped binary,
a two-CDS file carrying `aph(3')-Ia aminoglycoside phosphotransferase gene`
puts a label from x=530 to x=806.42 under `%%BoundingBox: 0 0 720 720` — 86
points of gene name cropped off the plate, with this oracle printing
`problems : 0`. The left-hand column fails the same way in mirror image, with a
negative start x. That is a layout bug in `pl_draw` and is not fixed here; what
is fixed is that the oracle can now see it.

Labels are measured as ink boxes — origin, plus the advance width of the
decoded bytes, plus Helvetica's ascent and descent — because the origin alone
would have closed only half the hole. A right-column label is `Anchor::Start`,
so its `moveto` stays inside the box while the string runs out of it, which is
exactly the case above.

A `)` inside a feature name is the specific hazard for the string check: this
project's own feature database contains `aph(3')-Ia`, and an unescaped
parenthesis ends the string and turns the rest of the program into garbage.

Exits 1 on any disagreement and on comparing nothing.
"""
import os
import re
import subprocess
import sys

# Every operator this emitter is allowed to use. Anything else is either a typo
# or a Level-3 feature that an older RIP will reject.
ALLOWED_OPS = {
    "gsave", "grestore", "newpath", "moveto", "lineto", "curveto", "closepath",
    "fill", "stroke", "setlinewidth", "setrgbcolor", "setlinejoin", "setlinecap",
    "rectfill", "findfont", "scalefont", "setfont", "show", "showpage",
}

# Advance widths for Helvetica, U+0020..U+007E, in 1/1000 em. The same numbers
# as `pl_draw::pdf::HELVETICA`, transcribed because this file takes no
# dependencies -- and note what that does and does not buy: a label's *ink box*
# is computed here from the emitter's own idea of how wide Helvetica is, so this
# proves containment, not that the metrics are right. `text_width` is checked
# for agreement between the three emitters on the Rust side; here the question
# is only whether the box the emitter declares holds the box it drew.
HELVETICA = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333,  #  !"#$%&'()
    389, 584, 278, 333, 278, 278, 556, 556, 556, 556,  # *+,-./0123
    556, 556, 556, 556, 556, 556, 278, 278, 584, 584,  # 456789:;<=
    584, 556, 1015, 667, 667, 722, 722, 667, 611, 778,  # >?@ABCDEFG
    722, 278, 500, 667, 556, 833, 722, 778, 667, 778,  # HIJKLMNOPQ
    722, 667, 611, 722, 667, 944, 667, 667, 611, 278,  # RSTUVWXYZ[
    278, 278, 469, 556, 333, 556, 556, 500, 556, 556,  # \]^_`abcde
    278, 556, 556, 222, 222, 500, 222, 833, 556, 556,  # fghijklmno
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500,  # pqrstuvwxy
    500, 334, 260, 334, 584,                           # z{|}~
]

# Helvetica's ascender and descender, in 1/1000 em. A label's ink is not on its
# baseline: it reaches 0.718 em above and 0.207 em below, so a string whose
# `moveto` sits exactly on the top edge of the box is already cropped through
# the ascenders. Checking the origin alone was the mistake this whole section
# is here to avoid.
ASCENT = 0.718
DESCENT = 0.207

# The one shape the emitter writes a label as, in `pl_draw::eps`:
#   /Helvetica findfont 12 scalefont setfont 0 0 0 setrgbcolor X Y moveto (s) show
LABEL_RE = re.compile(
    r"findfont\s+(-?[\d.]+)\s+scalefont\s+setfont\b[^()]*?"
    r"(-?[\d.]+)\s+(-?[\d.]+)\s+moveto\s+(\((?:[^()\\]|\\.)*\))\s+show")


def ps_unescape(lit):
    """A PostScript string literal, back to the bytes it stands for.

    The emitter writes non-ASCII as octal (`\\326` for O-diaeresis), so decoding
    is not optional: measuring the literal's characters instead of its bytes
    would count `\\326` as four glyphs wide and quietly overstate every Latin-1
    label by three characters.
    """
    body = lit[1:-1]
    out = bytearray()
    i, n = 0, len(body)
    while i < n:
        c = body[i]
        if c != "\\":
            out.append(ord(c) & 0xFF)
            i += 1
            continue
        i += 1
        if i >= n:
            break
        d = body[i]
        if d.isdigit():
            oct_digits = ""
            while i < n and len(oct_digits) < 3 and body[i] in "01234567":
                oct_digits += body[i]
                i += 1
            out.append(int(oct_digits, 8) & 0xFF)
        else:
            out.append({"n": 10, "r": 13, "t": 9, "b": 8, "f": 12}.get(d, ord(d)) & 0xFF)
            i += 1
    return bytes(out)


def text_width(lit, pts):
    """How wide the label will actually be, in points."""
    total = 0
    for b in ps_unescape(lit):
        # Outside printable ASCII the emitter itself estimates 556; matching
        # that here keeps the two sides talking about the same box.
        total += HELVETICA[b - 0x20] if 0x20 <= b <= 0x7E else 556
    return total * pts / 1000.0


def eps_text_boxes(text):
    """Ink boxes for every label: (x0, y0, x1, y1), plus lines we could not read.

    `eps_tokens` drops every line containing ` show` -- correctly, because PDF
    positions a string with `Td` inside `BT`/`ET` rather than with `moveto`, so
    feeding label origins into the path comparison would report a difference
    that is only one of notation. The cost was that **no text coordinate was
    ever compared against the BoundingBox at all**, while the docstring, the
    summary banner and tools/ci.ps1 all said the box was proved to contain the
    artwork. It did not: `pl_draw`'s label gutter is capped at 30% of the canvas
    (`pl-draw/src/lib.rs`), so an ordinary GenBank file with feature names over
    roughly 28 characters overflows it. Measured on the shipped binary with a
    two-CDS file carrying `aph(3')-Ia aminoglycoside phosphotransferase gene`:
    a label runs from x=530 to x=806.42 under `%%BoundingBox: 0 0 720 720`, so
    86 points of gene name are cropped off the plate -- and this oracle printed
    `problems : 0`. The left-hand column fails the same way in mirror image,
    with a negative start x.
    """
    boxes = []
    unreadable = []
    for line in text.splitlines():
        if line.startswith("%") or " show" not in line:
            continue
        m = LABEL_RE.search(line)
        if not m:
            # An unparsed label is an unchecked label, and this check exists
            # because unchecked labels were the hole. Report it rather than
            # skipping it: if the emitter's output shape changes, this oracle
            # must go red, not quietly go back to measuring nothing.
            unreadable.append(line[:70])
            continue
        pts, x0, y0, lit = float(m.group(1)), float(m.group(2)), float(m.group(3)), m.group(4)
        w = text_width(lit, pts)
        boxes.append((x0, y0 - DESCENT * pts, x0 + w, y0 + ASCENT * pts))
    return boxes, unreadable


def run(exe, args):
    r = subprocess.run([exe] + args, capture_output=True)
    if r.returncode != 0:
        raise RuntimeError(f"pl {' '.join(args)}: {r.stderr.decode(errors='replace')}")
    return r.stdout


def eps_tokens(text):
    """Path-geometry operators, in order.

    Lines that place text are skipped. PostScript positions a string with the
    same `moveto` it uses for a path, while PDF uses `Td` inside `BT`/`ET`, so
    counting them here would compare a label position against a path point and
    report a difference that is only one of notation. Labels are compared
    separately, by content.
    """
    out = []
    for line in text.splitlines():
        if line.startswith("%") or " show" in line:
            continue
        # Strip string literals so their contents cannot look like operators.
        bare = re.sub(r"\((?:[^()\\]|\\.)*\)", " STR ", line)
        t = bare.split()
        for i, tok in enumerate(t):
            if tok in ("moveto", "lineto"):
                out.append((tok, tuple(float(x) for x in t[i - 2:i])))
            elif tok == "curveto":
                out.append((tok, tuple(float(x) for x in t[i - 6:i])))
            elif tok == "arc":
                out.append((tok, tuple(float(x) for x in t[i - 5:i])))
    return out


def pdf_tokens(data):
    """The same, from the PDF content stream, which is uncompressed by design."""
    m = re.search(rb"stream\r?\n(.*?)\r?\nendstream", data, re.S)
    if not m:
        raise RuntimeError("no content stream in the PDF")
    text = m.group(1).decode("latin-1")
    out = []
    for line in text.splitlines():
        bare = re.sub(r"\((?:[^()\\]|\\.)*\)", " STR ", line)
        t = bare.split()
        for i, tok in enumerate(t):
            if tok == "m":
                out.append(("moveto", tuple(float(x) for x in t[i - 2:i])))
            elif tok == "l":
                out.append(("lineto", tuple(float(x) for x in t[i - 2:i])))
            elif tok == "c":
                out.append(("curveto", tuple(float(x) for x in t[i - 6:i])))
    return out


def check_structure(text, path):
    """PostScript soundness, independent of what it draws."""
    problems = []

    depth = 0
    for line in text.splitlines():
        if line.startswith("%"):
            continue
        bare = re.sub(r"\((?:[^()\\]|\\.)*\)", " STR ", line)
        for tok in bare.split():
            if tok == "gsave":
                depth += 1
            elif tok == "grestore":
                depth -= 1
                if depth < 0:
                    problems.append("grestore without a matching gsave")
    if depth != 0:
        problems.append(f"{depth} unbalanced gsave(s)")

    # Every string literal must close, with escaping honoured. An unescaped ')'
    # in a name like aph(3')-Ia ends the string and eats the rest of the file.
    for line in text.splitlines():
        if line.startswith("%"):
            continue
        i, n = 0, len(line)
        while i < n:
            if line[i] == "\\":
                i += 2
                continue
            if line[i] == "(":
                j, d = i + 1, 1
                while j < n and d:
                    if line[j] == "\\":
                        j += 2
                        continue
                    d += (line[j] == "(") - (line[j] == ")")
                    j += 1
                if d:
                    problems.append(f"unterminated string: {line[:60]}")
                    break
                i = j
                continue
            i += 1

    # Only known operators.
    for line in text.splitlines():
        if line.startswith("%") or line.startswith("/"):
            continue
        bare = re.sub(r"\((?:[^()\\]|\\.)*\)", " ", line)
        for tok in bare.split():
            if tok.startswith("/") or tok == "STR":
                continue
            try:
                float(tok)
                continue
            except ValueError:
                pass
            if tok not in ALLOWED_OPS:
                problems.append(f"unknown operator {tok!r}")

    # The BoundingBox must contain the artwork.
    bb = re.search(r"%%BoundingBox: (\S+) (\S+) (\S+) (\S+)", text)
    if not bb:
        problems.append("no %%BoundingBox")
    else:
        x0, y0, x1, y1 = (float(v) for v in bb.groups())
        for op, vals in eps_tokens(text):
            pts = [(vals[i], vals[i + 1]) for i in range(0, 6 if op == "curveto" else 2, 2)]
            if op == "arc":
                cx, cy, r = vals[0], vals[1], vals[2]
                pts = [(cx - r, cy - r), (cx + r, cy + r)]
            for px, py in pts:
                if not (x0 - 0.5 <= px <= x1 + 0.5 and y0 - 0.5 <= py <= y1 + 0.5):
                    problems.append(
                        f"({px}, {py}) is outside the BoundingBox "
                        f"{x0} {y0} {x1} {y1} — the figure would be cropped")
                    break
            else:
                continue
            break

        # ... and the text too. Right-column labels are `Anchor::Start`, so
        # their `moveto` stays inside while the string runs off the right edge:
        # testing the origin alone would still pass a figure whose longest
        # feature name is half outside the box.
        boxes, unreadable = eps_text_boxes(text)
        for line in unreadable:
            problems.append(f"label line this check could not read: {line}")
        for tx0, ty0, tx1, ty1 in boxes:
            if not (x0 - 0.5 <= tx0 and tx1 <= x1 + 0.5
                    and y0 - 0.5 <= ty0 and ty1 <= y1 + 0.5):
                problems.append(
                    f"a label's ink box ({tx0}, {ty0})-({tx1}, {ty1}) is outside "
                    f"the BoundingBox {x0} {y0} {x1} {y1} — the label would be "
                    f"cropped")
                break
    return problems


def main(argv):
    exe = None
    if argv and os.path.isfile(argv[0]):
        exe, argv = os.path.abspath(argv[0]), argv[1:]
    if exe is None or not argv:
        print("usage: xcheck_eps.py <pl.exe> <plasmid file>...")
        return 1

    compared = 0
    ops = 0
    strings = 0
    label_boxes = 0
    bad = []
    for f in argv:
        if not os.path.isfile(f):
            continue
        # No --mm on either side, so scene units are points in both and the
        # only difference left to check is the coordinate convention.
        try:
            eps = run(exe, ["export", f, "--eps", "--stdout"]).decode("latin-1")
            pdf = run(exe, ["export", f, "--pdf", "--stdout"])
        except RuntimeError as e:
            # A multi-record file draws no single map by design; not this
            # check's business, and covered where that rule lives.
            if "would draw only the first" in str(e):
                continue
            raise
        compared += 1

        for p in check_structure(eps, f):
            bad.append((f, p))
        label_boxes += len(eps_text_boxes(eps)[0])

        e, p = eps_tokens(eps), pdf_tokens(pdf)
        # The EPS paints a white background rectangle the PDF does not, so drop
        # a leading run the PDF has no counterpart for.
        while len(e) > len(p) and e and e[0][0] == "moveto":
            if e[1:1 + len(p)] == p:
                e = e[1:]
                break
            e = e[1:]
        if len(e) != len(p):
            bad.append((f, f"{len(e)} EPS path ops against {len(p)} in the PDF"))
            continue
        for k, ((eo, ev), (po, pv)) in enumerate(zip(e, p)):
            ops += 1
            if eo != po:
                bad.append((f, f"op {k}: {eo} vs {po}"))
                break
            if any(abs(a - b) > 0.02 for a, b in zip(ev, pv)):
                bad.append((f, f"op {k} {eo}: {ev} vs {pv}"))
                break

        # Every label in one is a label in the other. Both sides are matched
        # with the *same* pattern against decoded text: writing it twice, once
        # for `str` and once for `bytes`, is how the first version of this check
        # ended up over-escaping the bytes copy and reporting a difference
        # between the emitters that did not exist.
        pdf_stream = re.search(r"stream\r?\n(.*?)\r?\nendstream",
                               pdf.decode("latin-1"), re.S)
        label = r"\((?:[^()\\]|\\.)*\)\s*"
        es = re.findall(label + "show", eps)
        ps = re.findall(label + "Tj", pdf_stream.group(1) if pdf_stream else "")
        strings += len(es)
        if len(es) != len(ps):
            bad.append((f, f"{len(es)} EPS labels against {len(ps)} in the PDF"))

    print("=" * 74)
    print(f"figures compared     : {compared}")
    print(f"path operators agreed: {ops:,}")
    print(f"labels               : {strings:,}")
    print(f"label ink boxes in box: {label_boxes:,}")
    print(f"problems             : {len(bad)}")
    print()
    print("No PostScript interpreter is installed here, so this does NOT prove a")
    print("renderer draws the file. It proves the geometry is point-for-point the")
    print("PDF's after the y-flip, that gsave/grestore and every string literal")
    print("balance, that only Level 2 operators appear, and that the BoundingBox")
    print("contains the artwork -- paths AND label ink boxes, origin plus advance")
    print("width plus ascent and descent. A box smaller than the drawing crops it")
    print("silently, in the typesetter's hands rather than on screen.")

    for f, why in bad[:8]:
        print(f"\n  {os.path.basename(f)}: {why}")

    if compared == 0 or ops == 0:
        print("\nFAIL: compared nothing")
        return 1
    # Labels present but none measured means the bbox arm of this check has
    # gone back to testing paths only, which is precisely the state it shipped
    # in. Say so rather than reporting a clean run over half the figure.
    if strings and not label_boxes:
        print(f"\nFAIL: {strings} label(s) found and 0 bounds-checked")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} problem(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
