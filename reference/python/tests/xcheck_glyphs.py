"""Our glyph outlines, against fontTools.

`crates/pl-draw/src/font.rs` walks the `glyf` table by hand, because everything
under `crates/` takes no dependencies. Nothing in the repository can check that
walk: a test that reads `glyf` the way `font.rs` reads `glyf` agrees with itself
by construction.

fontTools is a different implementation, in another language, by other people.
It is the oracle for the one thing that most needs one.

THE IMPLIED ON-CURVE POINT IS NOT RE-DERIVED HERE, and that is the whole design
of this check. TrueType contours are quadratic B-splines in which two
consecutive off-curve points imply an on-curve point at their midpoint. If this
script expanded that rule itself, it would be testing the Rust against a second
statement of the same reasoning by the same author.

Instead it subclasses `fontTools.pens.basePen.BasePen`, whose own `qCurveTo`
performs the expansion and calls `_qCurveToOne(ctrl, end)` per segment. The
expansion is therefore fontTools', not ours. This matters: an earlier design's
oracle for this stage was a glyph bounding box, which is blind to ignoring the
on-curve flag entirely -- a defect worth about half a pixel at 9 pt and 300 dpi
that no reviewer would see either.

THE SAME ARGUMENT IS WHY THE RANGE IS NOT JUST ASCII. Measured by walking
`glyf` directly (2026-08-04): no ASCII codepoint in either committed face is a
composite glyph, while 59 Latin-1 codepoints are in Regular and 58 in Bold --
every accented letter, plus U+00A0, U+00AD and the fractions. An ASCII-only
range therefore judged `Face::composite` not at all, and `pdf::encode` passes
Latin-1 through because accented characters turn up in feature names. The
composite decoder places one glyph inside another at a signed offset, and an
offset read as unsigned, or a second component skipped, moves an accent by a
few font units -- invisible in the rendered glyph, which is the point.

`fontTools`' glyph set expands components itself (`_TTGlyphGlyf.draw` walks
them), so this side of the comparison is again its statement of the rule and
not a second copy of ours.

Run from the repository root, after:

    cargo test -p pl-draw --test glyphs
"""

import os
import re
import sys

try:
    from fontTools.pens.basePen import BasePen
    from fontTools.ttLib import TTFont
except ImportError:
    print("fontTools is not installed", file=sys.stderr)
    sys.exit(2)

TOL = 5e-4  # the Rust prints 4 decimals

# Printable ASCII, then printable Latin-1; the gap is the C1 controls. Must
# match the loop in `crates/pl-draw/tests/glyphs.rs`, and a mismatch shows up
# as a missing-codepoint failure rather than as a silent shrinking of the
# check -- see `compare`, which iterates over what fontTools was asked for.
CODEPOINTS = list(range(0x20, 0x7F)) + list(range(0xA0, 0x100))


class Steps(BasePen):
    """Collect the same commands `font.rs` emits, expanded by fontTools."""

    def __init__(self, glyphSet):
        super().__init__(glyphSet)
        self.out = []

    def _moveTo(self, pt):
        self.out.append(("M", pt[0], pt[1]))

    def _lineTo(self, pt):
        self.out.append(("L", pt[0], pt[1]))

    def _curveToOne(self, a, b, c):  # cubic; TrueType glyphs have none
        self.out.append(("C", c[0], c[1]))

    def _qCurveToOne(self, ctrl, end):
        self.out.append(("Q", ctrl[0], ctrl[1], end[0], end[1]))

    def _closePath(self):
        self.out.append(("Z",))


def ours(path):
    """Parse the Rust dump into {codepoint: [command, ...]}."""
    by_cp, cur = {}, None
    for line in open(path, encoding="utf8"):
        line = line.strip()
        if not line:
            continue
        if line.startswith("#"):
            cp = int(line.split()[1])
            cur = by_cp.setdefault(cp, [])
            continue
        f = line.split()
        cur.append(tuple([f[0]] + [float(v) for v in f[1:]]))
    return by_cp


def theirs(ttf):
    font = TTFont(ttf)
    gs = font.getGlyphSet()
    cmap = font.getBestCmap()
    glyf = font["glyf"]
    out, composite = {}, 0
    for cp in CODEPOINTS:
        name = cmap.get(cp)
        pen = Steps(gs)
        if name is not None:
            gs[name].draw(pen)
            if glyf[name].isComposite():
                composite += 1
        out[cp] = pen.out
    return out, composite


def compare(name, a, b):
    bad = 0
    checked = 0
    for cp in sorted(b):
        x, y = a.get(cp, []), b[cp]
        # fontTools closes a contour it opened; so do we. But it emits no
        # trailing `lineTo` back to the start when the contour already ends
        # there, and neither do we -- if that ever diverges it shows up as a
        # length mismatch, which is the point.
        if len(x) != len(y):
            print(f"  FAIL {name} U+{cp:04X} {chr(cp)!r}: {len(x)} commands, fontTools has {len(y)}")
            bad += 1
            continue
        for i, (p, q) in enumerate(zip(x, y)):
            if p[0] != q[0]:
                print(f"  FAIL {name} U+{cp:04X} {chr(cp)!r} step {i}: {p[0]} vs {q[0]}")
                bad += 1
                break
            if len(p) != len(q):
                print(f"  FAIL {name} U+{cp:04X} {chr(cp)!r} step {i}: arity {len(p)} vs {len(q)}")
                bad += 1
                break
            if any(abs(u - v) > TOL for u, v in zip(p[1:], q[1:])):
                print(
                    f"  FAIL {name} U+{cp:04X} {chr(cp)!r} step {i} {p[0]}: "
                    f"{tuple(round(v, 4) for v in p[1:])} vs "
                    f"{tuple(round(v, 4) for v in q[1:])}"
                )
                bad += 1
                break
        checked += len(y)
    return bad, checked


def check_gate_comment(root, pairs, comps, total, bad):
    """The coverage figure quoted above the gate step, against this run.

    That comment is the one line in the repository that says how much of the
    outline decoder its only independent oracle sees, and it decayed the moment
    the range widened: it read "3,504 outline commands" -- 95 ASCII codepoints
    across 2 faces, with 0 composites among them -- for a check that had since
    been extended to Latin-1 and was really comparing 8,657 across 117
    composites. A reader auditing coverage would have concluded the composite
    branch was unjudged, which is what the number described and not what the
    check did.

    So the figure is re-derived here, where it is produced, rather than
    remembered there. `Step` judges on the exit code, so a stale comment now
    reddens the gate.
    """
    path = os.path.join(root, "tools", "ci.ps1")
    if not os.path.exists(path):
        print(f"FAIL: no {path}, so the gate comment's coverage figure is unpinned")
        return 1
    # Unwrapped, so a claim that spans three comment lines reads as one
    # sentence -- the same join `deflate/tests.rs` makes over `//!`.
    text = " ".join(
        line.strip().lstrip("#").strip()
        for line in open(path, encoding="utf8")
        if line.strip().startswith("#")
    )
    text = re.sub(r"\s+", " ", text)
    m = re.search(
        r"It now compares ([\d,]+) outline commands over (\d+) glyph-face pairs, "
        r"(\d+) of them composites, with (\d+) disagreements\.",
        text,
    )
    if m is None:
        print("FAIL: tools/ci.ps1 no longer states the fontTools coverage figure it was given")
        return 1
    want = (total, pairs, comps, bad)
    got = tuple(int(g.replace(",", "")) for g in m.groups())
    if got != want:
        print(
            f"FAIL: tools/ci.ps1 claims {got[0]} commands over {got[1]} pairs "
            f"({got[2]} composite, {got[3]} disagreeing); this run measured "
            f"{want[0]}, {want[1]}, {want[2]}, {want[3]}"
        )
        return 1
    return 0


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    d = os.path.join(root, "target", "tmp", "glyphs")
    if not os.path.exists(os.path.join(d, "regular.outlines")):
        print(f"no outlines at {d}", file=sys.stderr)
        print("run: cargo test -p pl-draw --test glyphs", file=sys.stderr)
        return 2

    bad = total = comps = pairs = 0
    for name, ttf in [
        ("regular", "crates/pl-draw/fonts/LiberationSans-Regular.ttf"),
        ("bold", "crates/pl-draw/fonts/LiberationSans-Bold.ttf"),
    ]:
        a = ours(os.path.join(d, name + ".outlines"))
        b, composite = theirs(os.path.join(root, ttf))
        n, c = compare(name, a, b)
        bad += n
        comps += composite
        total += c
        pairs += len(CODEPOINTS)
        print(
            f"  {'ok  ' if n == 0 else 'FAIL'} {name}: {len(CODEPOINTS)} glyphs "
            f"({composite} composite), {c} commands, {n} disagreeing"
        )
    # A composite count of zero would mean the range had quietly stopped
    # covering `Face::composite` -- the state this check was in before the
    # Latin-1 range was added, when it compared 95 codepoints of which none
    # was a composite glyph in either face.
    if comps == 0:
        print("FAIL: not one compared glyph is a composite, so the composite decoder is unjudged")
        return 1
    stale = check_gate_comment(root, pairs, comps, total, bad)
    print(
        f"{total} outline commands compared against fontTools "
        f"({comps} composite glyphs among them); {bad} glyphs disagree"
    )
    return 1 if (bad or stale) else 0


if __name__ == "__main__":
    sys.exit(main())
