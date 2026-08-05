"""Check that the checks can fail.

    python xcheck_oracles.py

Every other file in this directory is an oracle: it looks at something and says
whether it is right. This one looks at the oracles. It exists because this
project has now shipped several checks that were green by construction, and
every one of them was found by reading, never by a red gate:

  * the bench step reported `ok` for a score of zero;
  * `validate_digest.py` exited 0 when it compared zero files, and exited 0 when
    it found mismatches;
  * `drive_wasm.mjs` was wired into the gate under the name "wasm module vs
    native binary" with no corpus, so it compared nothing;
  * `test_roundtrip.py` counted every problem it found, printed the number, and
    exited 0 regardless -- README.md and CONTRIBUTING.md both named it as a
    check to run yourself;
  * `xcheck_eps.py` said it proved "a `%%BoundingBox` that actually contains
    every coordinate emitted" while its token scanner dropped every line
    containing ` show`, which is every label the emitter writes.

A check that cannot fail is worse than no check, because it is counted. So the
two properties below are pinned by *injecting the broken behaviour* and
demanding the oracle notice, and each is paired with a control that the
neighbouring correct case still passes -- a case that goes red for everything
proves as little as one that goes green for everything.

Standard library only, like everything else here, and no fixtures: the inputs
are synthesised, so this runs on a bare checkout with no corpus and no build.
"""
import glob
import io
import os
import sys
import contextlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import test_roundtrip                            # noqa: E402
import xcheck_eps                                # noqa: E402
import xcheck_icon                                # noqa: E402

CHECKS = []


def case(name):
    def deco(fn):
        CHECKS.append((name, fn))
        return fn
    return deco


def quiet(fn, *a, **kw):
    """Run something noisy and keep its output out of the summary."""
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        return fn(*a, **kw)


# --------------------------------------------------------------------------
# test_roundtrip.py
# --------------------------------------------------------------------------

def _one_real_path():
    """Any path a glob can match. The corpus is optional; this file is not."""
    return os.path.abspath(__file__)


@contextlib.contextmanager
def _check_returning(problems):
    """Replace test_roundtrip.check with one that reports `problems`."""
    class Doc:
        length = 2686
        is_circular = True
        features = []
        primers = []
        history_xml = None

    original = test_roundtrip.check
    test_roundtrip.check = lambda path: (Doc(), list(problems), 0.5)
    try:
        yield
    finally:
        test_roundtrip.check = original


@case("test_roundtrip exits non-zero when a file round-trips wrong")
def roundtrip_reports_problems():
    # The injection is the real failure mode: break `snapdna.dumps` and every
    # row prints `!! round-trip NOT byte-exact`. Before the fix the summary
    # said `problems found : 344` and the process still exited 0, so a corpus
    # in which nothing round-tripped was indistinguishable from a clean one.
    #
    # Note the assertion: `rc == 1`, not `rc != 0`. The broken `main` fell off
    # the end and returned `None`, and `None != 0` is True in Python -- so the
    # first version of this very case passed against the code it was written to
    # catch. That is the sixth green-by-construction check in this project and
    # it lasted about four minutes, which is roughly how long the others would
    # have lasted had anyone injected the fault.
    with _check_returning(["round-trip NOT byte-exact"]):
        rc = quiet(test_roundtrip.main, [_one_real_path()])
    return rc == 1, f"exit status was {rc!r}, expected 1"


@case("test_roundtrip exits non-zero when the glob matches no files")
def roundtrip_reports_empty_run():
    # `files clean : 0/0` used to read as a pass. Quoting the glob wrongly so
    # the shell eats it is the one typo that turns a 344-file run into this.
    rc = quiet(test_roundtrip.main, ["no/such/directory/**/*.dna"])
    return rc == 1, f"exit status was {rc!r}, expected 1"


@case("test_roundtrip still exits zero on a clean corpus")
def roundtrip_control_clean():
    # The control. An oracle that fails for everything is no better than one
    # that passes for everything.
    with _check_returning([]):
        rc = quiet(test_roundtrip.main, [_one_real_path()])
    return rc == 0, f"exit status was {rc}, expected 0 on a clean run"


# --------------------------------------------------------------------------
# xcheck_eps.py -- the BoundingBox must contain the TEXT, not only the paths
# --------------------------------------------------------------------------

EPS_HEAD = (
    "%!PS-Adobe-3.0 EPSF-3.0\n"
    "%%BoundingBox: 0 0 720 720\n"
    "%%HiResBoundingBox: 0 0 720 720\n"
    "%%EndComments\n"
    "gsave 1 1 1 setrgbcolor 0 0 720 720 rectfill grestore\n"
    "1 setlinejoin 1 setlinecap\n"
)
EPS_TAIL = "showpage\n%%EOF\n"


def eps_with(label_lines, path_lines=()):
    """A minimal but structurally valid EPS, in the emitter's exact shape."""
    return EPS_HEAD + "".join(path_lines) + "".join(label_lines) + EPS_TAIL


def label(x, y, text, pts=12):
    return (f"/Helvetica findfont {pts} scalefont setfont 0 0 0 setrgbcolor "
            f"{x} {y} moveto ({text}) show\n")


@case("a label starting left of the BoundingBox is reported")
def eps_label_negative_x():
    # Not hypothetical. `pl_draw`'s label gutter is capped at 30% of the
    # canvas, so a feature name over roughly 28 characters at 12 pt puts a
    # left-column label at negative x: the shipped binary emits
    # `-86.42 243.96 moveto (aph\(3'\)-Ia ...) show` under
    # `%%BoundingBox: 0 0 720 720`, and this oracle reported `problems : 0`.
    eps = eps_with([label(-86.42, 243.96,
                          r"aph\(3'\)-Ia aminoglycoside phosphotransferase")])
    p = xcheck_eps.check_structure(eps, "synthetic")
    return any("BoundingBox" in x for x in p), f"reported {p}"


@case("a label whose text runs off the right edge is reported")
def eps_label_overflows_right():
    # The half of the hole that checking the origin alone would have left
    # open: right-column labels are `Anchor::Start`, so the `moveto` sits
    # inside the box while the string runs out of it. 40 characters of
    # Helvetica at 12 pt is about 260 pt wide, from x=700 that ends near 960.
    eps = eps_with([label(700, 360, "A" * 40)])
    p = xcheck_eps.check_structure(eps, "synthetic")
    return any("BoundingBox" in x for x in p), f"reported {p}"


@case("a label whose ascenders clear the top edge is reported")
def eps_label_overflows_top():
    # Ink is not on the baseline: Helvetica reaches 0.718 em above it, so a
    # 12 pt label with its baseline at y=716 has ascenders at y=724.6, four
    # points outside a 720 pt box and cropped through the capitals.
    eps = eps_with([label(10, 716, "AmpR")])
    p = xcheck_eps.check_structure(eps, "synthetic")
    return any("BoundingBox" in x for x in p), f"reported {p}"


@case("a label wholly inside the BoundingBox is not reported")
def eps_label_control_inside():
    # The control for all three above. Over-correcting here would make the
    # gate red for every figure and get the check deleted, which is how a
    # project ends up with no check at all.
    eps = eps_with([label(100, 360, "AmpR"), label(200, 100, r"aph\(3'\)-Ia")])
    p = xcheck_eps.check_structure(eps, "synthetic")
    return not p, f"reported {p}"


@case("a path point outside the BoundingBox is still reported")
def eps_path_control_still_checked():
    # The neighbouring behaviour the text check must not have displaced.
    eps = eps_with([], ["newpath\n", "10 10 moveto\n", "900 10 lineto\n"])
    p = xcheck_eps.check_structure(eps, "synthetic")
    return any("BoundingBox" in x for x in p), f"reported {p}"


@case("a label line the emitter's pattern cannot read is reported, not skipped")
def eps_unreadable_label_reported():
    # Nothing may fail silently. If the emitter's label shape ever changes,
    # this oracle must go red rather than quietly return to measuring paths
    # only -- which is exactly the state it shipped in.
    eps = eps_with(["(AmpR) show\n"])
    p = xcheck_eps.check_structure(eps, "synthetic")
    return any("could not read" in x for x in p), f"reported {p}"


@case("eps_tokens still drops label lines, so the PDF comparison is unpolluted")
def eps_tokens_control():
    # The skip in `eps_tokens` is correct and must stay: PDF positions a string
    # with `Td` inside `BT`/`ET`, so feeding a label's `moveto` into the path
    # comparison would report a difference that is only one of notation. The
    # bug was never the skip, it was that nothing else looked at the text.
    eps = eps_with([label(100, 360, "AmpR")],
                   ["newpath\n", "10 10 moveto\n", "20 20 lineto\n"])
    toks = xcheck_eps.eps_tokens(eps)
    got = [t for t in toks if t[1] == (100.0, 360.0)]
    return not got, f"the label's moveto reached the path stream: {toks}"


@case("an octal-escaped label is measured in bytes, not in characters")
def eps_octal_width():
    # The emitter writes non-ASCII as octal, so `Olschlager` with an
    # O-diaeresis goes out as `\326lschl\344ger`. Measuring the literal's
    # characters would count `\326` as four glyphs and overstate the label by
    # three characters every time, which would make this check fire on figures
    # that are perfectly fine.
    lit = r"(\326lschl\344ger)"
    b = xcheck_eps.ps_unescape(lit)
    wide = xcheck_eps.text_width(lit, 12.0)
    # Ten glyphs: the two non-ASCII ones are estimated at 556, the same
    # middling estimate `pl_draw::pdf::width_of` uses, so both sides are
    # talking about the same box.
    expect = (556 + 222 + 500 + 500 + 556 + 222 + 556 + 556 + 556 + 333) * 12 / 1000
    naive = sum(xcheck_eps.HELVETICA[ord(c) - 0x20] for c in lit[1:-1]) * 12 / 1000
    return (b == b"\xd6lschl\xe4ger" and abs(wide - expect) < 0.01 and wide < naive,
            f"bytes={b!r} width={wide} expected {expect} (undecoded would be {naive})")


# --------------------------------------------------------------------------
# xcheck_icon.py -- the window icon must be the .ico's own frame
# --------------------------------------------------------------------------
#
# `xcheck_icon.compare` is the only thing that stands between the taskbar button
# of the running window and the executable's own icon becoming two different
# drawings, so it is pinned here for the reason everything above is. It imports
# at module scope safely: xcheck_icon keeps PIL and resvg_py inside `main`, so
# this file still runs on a bare checkout with nothing but the standard library.

BLUE = bytes((0x00, 0x72, 0xB2, 0xFF))


@case("a single differing byte between the window icon and the .ico is reported")
def icon_one_byte():
    # The real failure mode at its smallest. A length check or a digest would
    # also catch a swapped drawing; only a byte comparison catches an
    # antialiased edge that moved by one level, which is what a re-render from a
    # slightly edited master actually looks like.
    b = bytearray(BLUE * 4)
    b[6] = 0xB1
    p = xcheck_icon.compare([("frame vs blob", BLUE * 4, bytes(b))])
    return len(p) == 1 and "byte 6" in p[0], f"reported {p}"


@case("a truncated window icon is reported, not compared as far as it goes")
def icon_short():
    # A `zip` over two buffers of different length compares the shorter one and
    # passes. A blob half-written by an interrupted run of build-icon.py is
    # exactly that shape.
    p = xcheck_icon.compare([("frame vs blob", BLUE * 4, BLUE * 2)])
    return len(p) == 1 and "8 bytes against 16" in p[0], f"reported {p}"


@case("comparing nothing at all is reported, not counted as a pass")
def icon_empty_run():
    # The failure this project has shipped more often than any other. If the
    # .ico's directory ever fails to yield frames, the pair list is empty and
    # every byte comparison in it trivially succeeds.
    p = xcheck_icon.compare([])
    return len(p) == 1, f"reported {p}"


@case("identical icon bytes are not reported")
def icon_control_identical():
    # The control.
    p = xcheck_icon.compare([("frame vs blob", BLUE * 4, BLUE * 4)])
    return not p, f"reported {p}"


def main():
    passed = 0
    bad = []
    for name, fn in CHECKS:
        ok, why = fn()
        if ok:
            passed += 1
            print(f"  ok    {name}")
        else:
            bad.append((name, why))
            print(f"  FAIL  {name}")
            print(f"        {why}")

    print("\n" + "=" * 74)
    print(f"oracle properties pinned: {passed}/{len(CHECKS)}")
    print()
    print("These are meta-checks: they inject the broken behaviour and demand the")
    print("oracle notice. Each is paired with a control, because a check that goes")
    print("red for everything proves as little as one that goes green for it.")

    if not CHECKS:
        print("\nFAIL: pinned nothing")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} oracle(s) did not notice")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
