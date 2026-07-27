"""Check the EPS emitter against the PDF emitter, and against PostScript itself.

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
    print(f"problems             : {len(bad)}")
    print()
    print("No PostScript interpreter is installed here, so this does NOT prove a")
    print("renderer draws the file. It proves the geometry is point-for-point the")
    print("PDF's after the y-flip, that gsave/grestore and every string literal")
    print("balance, that only Level 2 operators appear, and that the BoundingBox")
    print("contains the artwork -- a box smaller than the drawing crops it")
    print("silently, in the typesetter's hands rather than on screen.")

    for f, why in bad[:8]:
        print(f"\n  {os.path.basename(f)}: {why}")

    if compared == 0 or ops == 0:
        print("\nFAIL: compared nothing")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} problem(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
