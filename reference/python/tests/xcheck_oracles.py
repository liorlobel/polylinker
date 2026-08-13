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
    native binary" with no corpus, so it compared nothing -- and the fix went
    into the wiring only, preconditioning the step on the corpus path EXISTING,
    which left the script itself with no floor: a directory holding zero `.dna`
    files still printed `identical: 0/0` and `ALL WASM CHECKS PASSED` and
    exited 0, until 2026-08-13;
  * `test_roundtrip.py` counted every problem it found, printed the number, and
    exited 0 regardless -- README.md and CONTRIBUTING.md both named it as a
    check to run yourself;
  * `xcheck_eps.py` said it proved "a `%%BoundingBox` that actually contains
    every coordinate emitted" while its token scanner dropped every line
    containing ` show`, which is every label the emitter writes.

A check that cannot fail is worse than no check, because it is counted. So every
property below is pinned by *injecting the broken behaviour* and demanding the
oracle notice, and each is paired with a control that the neighbouring correct
case still passes -- a case that goes red for everything proves as little as one
that goes green for everything.

Standard library only, like everything else here, and no fixtures: the inputs
are synthesised, so this runs on a bare checkout with no corpus and no build.
One dependency is declared rather than hidden: `drive_wasm.mjs` is a Node
script, nothing but Node can execute one, and the four cases that pin it
therefore shell out to `node`. If `node` is missing those four REPORT A FAILURE
and name the reason, rather than skipping -- a silent skip in this file would be
the exact shape of the defect the file exists to punish, and `node` is already
required by the workflow's `wasm` job and by four steps of `tools/ci.ps1`.
"""
import glob
import io
import os
import shutil
import subprocess
import sys
import tempfile
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


# --------------------------------------------------------------------------
# drive_wasm.mjs -- the corpus comparison must not pass having compared nothing
# --------------------------------------------------------------------------
#
# The wasm build has exactly one oracle: `drive_wasm.mjs` pushes every `.dna` in
# a corpus through the real `.wasm` and through `pl.exe` and demands the two
# agree. Until 2026-08-13 the loop that does it had no floor. `if (disagree)
# failures++` and `if (differ) failures++` are its only two ways of going red
# and neither can fire over an empty file list, so a corpus directory holding no
# `.dna` at all -- a GenBank-only folder, a corpus that moved, a OneDrive tree
# whose files are still placeholders -- printed `files compared : 0`,
# `identical: 0/0`, `ALL WASM CHECKS PASSED`, and exited 0. The gate believed it
# had closed this, having preconditioned the step on `Test-Path $Corpus` -- which
# asks whether a DIRECTORY EXISTS and not whether anything in it was compared, so
# the hole outlived the fix written to close it.
#
# Pinning it costs more machinery than the three oracles above, and the reason
# is worth stating: it is a Node script, and Python cannot execute one. So these
# cases lift the block under test STRAIGHT OUT OF THE SHIPPED FILE -- everything
# from the `corpus: wasm vs native` banner to the last line -- paste it under a
# preamble supplying the handful of names it reads from the rest of the script,
# and run that under `node`. Nothing is transcribed and nothing is re-stated:
# change the guard in `crates/pl-wasm/tests/drive_wasm.mjs` and these four cases
# change with it, which is the only arrangement worth having in a file whose
# whole subject is checks that drifted away from what they claimed to check.
#
# The preamble's molecule agrees with itself, so the comparisons inside the
# block are real but are never the thing being asserted. What is asserted is
# what the block does with zero files, with one file, with one file the two
# builds disagree about, and with no corpus argument at all -- that last one
# because the guard must NOT fire in the mode two shipped invocations use
# deliberately, and a fix that reddened the corpus-less legs of CI would have
# been worse than the defect it closed.

_DRIVE_WASM = os.path.normpath(os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..", "..", "..", "crates", "pl-wasm", "tests", "drive_wasm.mjs"))

# Sliced at the banner rather than at a line number: an edit anywhere above it
# then cannot silently change which region these cases run, and if the banner
# ever goes away `_drive_wasm` says so instead of quietly testing the wrong one.
_CORPUS_BANNER = "/* ---------- corpus: wasm vs native ---------- */"

# `__NATIVE_BP__` is substituted per case; every other brace here is JavaScript.
_STUBS = r"""/* Written by reference/python/tests/xcheck_oracles.py. Not part of the tree.

   Everything drive_wasm.mjs binds ABOVE the block spliced in below, in a form
   that needs no .wasm module and no native binary: with an empty file list the
   block touches none of it, and with a file in the list one molecule that
   agrees with itself drives every comparison the block makes. */
import { readdirSync } from "node:fs";
import { join, extname, basename } from "node:path";

const [corpus, plPath] = process.argv.slice(2);

const GENBANK =
  "LOCUS       synthetic                 12 bp    DNA     circular SYN 26-JUN-2026\n" +
  "ORIGIN\n" +
  "        1 aaaaaaaaaaaa\n" +
  "//\n";
const FROM_WASM = { bp: 12, circular: true, lowercase: 0, features: [], primers: [] };
const FROM_NATIVE = {
  bp: __NATIVE_BP__, circular: true, lowercase: 0,
  n_features: 0, n_primers: 0, n_binding_sites: 0, features: [],
};

const enc = new TextEncoder();
const dec = new TextDecoder();
let outBuf = enc.encode(JSON.stringify(FROM_WASM));
let failures = 0;

const readFileSync = () => new Uint8Array([0]);
const open = () => { outBuf = enc.encode(JSON.stringify(FROM_WASM)); return 0; };
const out = () => outBuf;
const outText = () => dec.decode(outBuf);
const outJson = () => JSON.parse(dec.decode(outBuf));
const withStr = (s, fn) => fn(0, enc.encode(s).length);
const w = {
  pl_sequence() { outBuf = enc.encode("a".repeat(FROM_WASM.bp)); },
  pl_to_genbank() { outBuf = enc.encode(GENBANK); return 0; },
};
const execFileSync = (_bin, args) =>
  args[0] === "info" ? JSON.stringify([FROM_NATIVE]) : GENBANK;

"""


def _drive_wasm(corpus_files=None, native_bp=12):
    """Run drive_wasm.mjs's own corpus block over a synthesised corpus.

    `corpus_files` is None for the two-argument invocation the workflow and the
    `wasm module self-checks` step both make, where no corpus is passed at all;
    otherwise it is a list of names created beside two decoys and an empty
    subdirectory. That shape is not invented -- it is the one the 2026-08-13
    audit reproduced the defect on, the in-repo `tests/library-fixture`: real
    `.gb` and `.fa` files, one subdirectory, zero `.dna`. `native_bp` is what
    the stub native binary reports, so a caller can make the builds disagree.

    Returns `(returncode, output)`, or `(None, why)` when the case could not be
    run at all. The callers report that second form as a FAILURE. It is the one
    place in this file where something could be skipped, and skipping is what
    every entry in the header did wrong.
    """
    node = shutil.which("node")
    if node is None:
        return None, ("`node` is not on PATH, so the only oracle that exists "
                      "for the browser build cannot be driven; this is a "
                      "failure rather than a skip on purpose")
    try:
        with open(_DRIVE_WASM, encoding="utf-8") as f:
            source = f.read()
    except OSError as e:
        return None, f"{e}"
    cut = source.find(_CORPUS_BANNER)
    if cut < 0:
        return None, f"{_DRIVE_WASM} no longer contains {_CORPUS_BANNER!r}"

    program = _STUBS.replace("__NATIVE_BP__", str(native_bp)) + source[cut:]
    with tempfile.TemporaryDirectory() as tmp:
        harness = os.path.join(tmp, "corpus_block.mjs")
        with open(harness, "w", encoding="utf-8") as f:
            f.write(program)
        argv = [node, harness]
        if corpus_files is not None:
            corpus = os.path.join(tmp, "corpus")
            os.makedirs(os.path.join(corpus, "subdir"))
            for name in ("notes.gb", "primers.fa", *corpus_files):
                with open(os.path.join(corpus, name), "w", encoding="utf-8") as f:
                    f.write(">decoy\nacgtacgtacgt\n")
            argv += [corpus, os.path.join(tmp, "pl")]
        done = subprocess.run(argv, capture_output=True, encoding="utf-8",
                              errors="replace")
    return done.returncode, (done.stderr.strip() or done.stdout.strip())


@case("drive_wasm refuses a corpus that turned out to hold no .dna files")
def drive_wasm_empty_corpus():
    # The injection is the absence of the guard, and it is not hypothetical: the
    # audit ran the shipped script at f0e4a6f against `tests/library-fixture`
    # and got `files compared : 0`, `identical: 0/0`, `ALL WASM CHECKS PASSED`,
    # exit 0.
    #
    # PROVEN TO FAIL at f0e4a6f: the corpus block ran its two loops over an
    # empty list and exited 0 having compared no molecules.
    # Mutation that re-breaks it: delete the four lines
    # `if (!files.length) { console.error(...); process.exit(2); }` that follow
    # `files.sort();` in crates/pl-wasm/tests/drive_wasm.mjs.
    #
    # `rc == 2` and not `rc != 0`, for the reason recorded in
    # `roundtrip_reports_problems` above: a check written against the loose form
    # once passed against the very code it was written to catch. 2 also carries
    # the distinction the script draws -- 1 is the two builds disagreeing, a
    # finding about the code; 2 is the harness pointed somewhere useless, a
    # finding about the invocation.
    rc, said = _drive_wasm(corpus_files=[])
    if rc is None:
        return False, said
    return (rc == 2 and "held no .dna files" in said,
            f"exit status was {rc!r}, expected 2; it said {said[-200:]!r}")


@case("drive_wasm still compares a corpus that has a .dna in it")
def drive_wasm_control_one_file():
    # The control, and the one that stops the guard being written as an
    # unconditional refusal. One `.dna` whose two builds agree must still be a
    # pass: exit 0, having compared exactly one molecule.
    rc, said = _drive_wasm(corpus_files=["plasmid.dna"])
    if rc is None:
        return False, said
    return (rc == 0 and "files compared : 1" in said,
            f"exit status was {rc!r}, expected 0; it said {said[-300:]!r}")


@case("drive_wasm still reports a wasm/native disagreement")
def drive_wasm_disagreement_still_seen():
    # The neighbouring behaviour the floor must not have displaced. This is the
    # thing the step is named after -- one build saying 12 bp and the other 11 --
    # and it has to keep going red, and go red as 1 rather than as 2, or the
    # floor has quietly replaced the comparison instead of guarding it.
    rc, said = _drive_wasm(corpus_files=["plasmid.dna"], native_bp=11)
    if rc is None:
        return False, said
    return (rc == 1 and "bp 12 vs 11" in said,
            f"exit status was {rc!r}, expected 1; it said {said[-300:]!r}")


@case("drive_wasm without a corpus argument still passes, as two CI legs need")
def drive_wasm_no_corpus_still_skips():
    # The other control, and the one that costs something if it is got wrong.
    # `.github/workflows/ci.yml`'s "Drive the real module" and `tools/ci.ps1`'s
    # "wasm module self-checks" both call this script with TWO arguments on
    # purpose: they have no corpus to give and they exist to exercise the ABI on
    # the hand-made inputs. The floor lives inside the `else` of `if (!corpus)`
    # so that those legs still take the announced skip and still exit 0. A guard
    # hoisted above that branch would redden both of them, which would have been
    # a worse defect than the one being fixed here.
    #
    # Mutation that re-breaks it: move the `if (!files.length)` guard out of the
    # `else` branch, e.g. up beside the `if (!corpus)` test.
    rc, said = _drive_wasm(corpus_files=None)
    if rc is None:
        return False, said
    return (rc == 0 and "no corpus given" in said,
            f"exit status was {rc!r}, expected 0; it said {said[-300:]!r}")


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
