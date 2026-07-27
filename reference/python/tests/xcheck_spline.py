"""Cross-validate the monotone spline against SciPy's PchipInterpolator.

    python xcheck_spline.py target/release/examples/dump_spline.exe

SciPy's `PchipInterpolator` *is* Fritsch–Carlson monotone cubic Hermite — the
same algorithm, written by different people from the same 1980 paper — so this
is a real independent implementation and not a transcription of ours.

Why the spline has an oracle at all: a gel calibration curve must be monotone,
because a longer fragment cannot run further than a shorter one. An ordinary
cubic spline through measured ladder points overshoots between knots, and real
ladders have uneven gaps (3, 4, 6, 10 kb) where the overshoot is large enough
to swap two bands. That is not a rounding error; it is the wrong answer about
which band is which. The tangent-clamping rules that prevent it are fiddly and
easy to get subtly wrong — precisely the thing to check against someone else's
implementation rather than against my own reasoning.

Ladder-shaped knot sets are included on purpose, alongside random ones: the
end intervals are where the one-sided tangent rule applies, and a ladder's
largest and smallest bands are exactly the ends.

Note that our `at()` **clamps** outside the knots where SciPy extrapolates.
That is a deliberate difference — extrapolating a calibration past the ladder
that produced it is where a gel prediction stops meaning anything — so queries
are drawn from inside the domain, and the clamping is covered by a unit test.

Exits 1 on any disagreement beyond 1e-9 relative, and on comparing nothing.
"""
import os
import random
import subprocess
import sys

import numpy as np
from scipy.interpolate import PchipInterpolator

TOL = 1e-9
rng = random.Random(20260729)


def ours(exe, knots, queries):
    # Full precision, and never repr(): numpy 2 renders a float64 as
    # "np.float64(2.69...)", which is not a number to anything downstream.
    stdin = "\n".join(f"{float(x):.17g} {float(y):.17g}" for x, y in knots) + "\n\n"
    stdin += "\n".join(f"{float(q):.17g}" for q in queries) + "\n"
    r = subprocess.run([exe], input=stdin, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"dump_spline: {r.stderr.strip()}")
    return [float(v) for v in r.stdout.split()]


def theirs(knots, queries):
    x = np.array([k[0] for k in knots], dtype=float)
    y = np.array([k[1] for k in knots], dtype=float)
    return [float(v) for v in PchipInterpolator(x, y)(np.array(queries, dtype=float))]


def ladder_knots():
    """Knot sets shaped like a real gel calibration: log10(bp) vs distance."""
    out = []
    for sizes in (
        [500, 1000, 1500, 2000, 3000, 4000, 6000, 10000],
        [100, 200, 300, 400, 500, 600, 800, 1000, 1500],
        [250, 500, 750, 1000, 2000, 2500, 3000, 3500, 4000, 5000, 6000, 8000, 10000],
        [1000, 10000],  # two knots: a straight line
    ):
        xs = [np.log10(s) for s in sizes]
        # A plausible decreasing migration curve, deliberately not linear.
        ys = [70.0 / (1.0 + 0.55 * (x - 2.0) ** 1.6) for x in xs]
        out.append(list(zip(xs, ys)))
    return out


def random_knots():
    out = []
    for _ in range(40):
        n = rng.randint(2, 12)
        xs = sorted(rng.uniform(-50, 50) for _ in range(n))
        # Reject near-duplicate x: both implementations are entitled to object.
        if any(b - a < 1e-6 for a, b in zip(xs, xs[1:])):
            continue
        shape = rng.choice(["monotone_up", "monotone_down", "wiggly", "flat_steps"])
        if shape == "monotone_up":
            ys = sorted(rng.uniform(-20, 20) for _ in range(n))
        elif shape == "monotone_down":
            ys = sorted((rng.uniform(-20, 20) for _ in range(n)), reverse=True)
        elif shape == "flat_steps":
            ys = [round(rng.uniform(-5, 5), 1) for _ in range(n)]
            for i in range(1, n, 2):  # deliberate ties, which pin a tangent to 0
                ys[i] = ys[i - 1]
        else:
            ys = [rng.uniform(-20, 20) for _ in range(n)]
        out.append(list(zip(xs, ys)))
    return out


def main(argv):
    exe = None
    if argv and os.path.isfile(argv[0]):
        exe = os.path.abspath(argv[0])
    if exe is None:
        print("usage: xcheck_spline.py <path to dump_spline.exe>")
        return 1

    sets = ladder_knots() + random_knots()
    compared = 0
    worst = 0.0
    bad = []
    for knots in sets:
        lo, hi = knots[0][0], knots[-1][0]
        # Inside the domain only: see the module docstring on clamping.
        queries = [lo, hi] + [lo + (hi - lo) * i / 97.0 for i in range(1, 97)]
        got = ours(exe, knots, queries)
        want = theirs(knots, queries)
        assert len(got) == len(want) == len(queries)
        for q, g, w in zip(queries, got, want):
            compared += 1
            scale = max(1.0, abs(w))
            err = abs(g - w) / scale
            worst = max(worst, err)
            if err > TOL:
                bad.append((knots, q, g, w, err))

    # Monotonicity is the property the whole thing exists for, so it is asserted
    # directly rather than inferred from agreeing with SciPy.
    reversals = 0
    for knots in ladder_knots():
        lo, hi = knots[0][0], knots[-1][0]
        qs = [lo + (hi - lo) * i / 4000.0 for i in range(4001)]
        vals = ours(exe, knots, qs)
        reversals += sum(1 for a, b in zip(vals, vals[1:]) if b > a + 1e-12)

    print("=" * 74)
    print(f"knot sets compared : {len(sets)}")
    print(f"points compared    : {compared:,}")
    print(f"worst relative diff: {worst:.3e}  (tolerance {TOL:.0e})")
    print(f"monotonicity reversals on decreasing ladder curves: {reversals}")
    print()
    print("SciPy's PchipInterpolator is Fritsch-Carlson monotone cubic Hermite:")
    print("the same 1980 algorithm, implemented by other people. Ladder-shaped")
    print("knot sets are included because the one-sided end-tangent rule applies")
    print("exactly at a ladder's largest and smallest bands.")

    for knots, q, g, w, err in bad[:5]:
        print(f"\n  at x={float(q):.17g} with {len(knots)} knots: rel err {err:.3e}")
        print(f"    ours  = {g:.17g}")
        print(f"    scipy = {w:.17g}")

    if compared == 0:
        print("\nFAIL: compared nothing")
        return 1
    if reversals:
        print(f"\nFAIL: the curve turned back {reversals} time(s)")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} disagreement(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
