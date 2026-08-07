"""Our raster of a figure, against resvg's raster of the same figure.

Every other check on `pl-draw`'s rasterizer is a property -- this pixel is dark,
that area is right, this colour parses. None of them can say the *picture* is
right.

resvg is an independent SVG renderer, in a different codebase. It is handed the
SVG this crate already emits, and forced onto the same font binary this crate
fills outlines from (`skip_system_fonts` plus explicit `font_files`), so a
disagreement about text is a disagreement about placement and rasterization
rather than about which typeface got picked. One comparison then covers arc
flattening, winding, stroke construction, antialiasing, glyph decoding, glyph
placement, the baseline constant, the anchor arithmetic and colour parsing.

IT CANNOT CATCH A WRONG SCENE, and must not be described as if it could: both
images come from the same SVG, so a misplaced feature is misplaced identically
in each. Moving an arrowhead's tip by 2 units leaves all four comparisons clean.
The figures' own geometry is asserted in `crates/pl-draw/src/tests.rs` and
`crates/pl-draw/src/linear.rs`, against the scene. Two figures are compared here
because they are two different rasterizer WORKLOADS -- arcs, thick strokes and
sparse text on one side; long thin boxes, concave pentagons, hairlines and dense
small text on the other.

WHY THE THRESHOLDS ARE WHAT THEY ARE. Two correct antialiasing implementations
do not agree bit for bit: this one samples 16 sub-scanlines per row and computes
exact horizontal coverage, resvg accumulates analytic area. Edge pixels differ
by small amounts and interior pixels do not. So the assertions are on the shape
of the disagreement, not on a single average:

  - **away from any edge, every pixel identical.** This is the real test.
    Antialiasing can only disagree where there is a gradient to disagree about;
    a wrong winding, a misplaced glyph or a hole in a stroke all put differing
    pixels in FLAT regions, where two correct renderers must agree exactly -- so
    the bar there is zero, not a percentage. Measured over both figures at both
    scales: 0 differing pixels of 71,009 to 8,151,997 flat ones. Adding half a
    unit to `raster`'s baseline constant fails it on both figures, and the old
    95%-of-all-pixels bar would not have noticed it on the ring at 4x.
  - and every grossly differing pixel must still lie on an edge, on the
    undilated mask. Measured on the ring at 4x: 490 gross pixels, 100% of them
    on an edge (median |grad| 114 against 0 for the canvas at large), scattered
    over 167 separate 8x8 tiles at about 3 px each.

A FLAT PIXEL IS ONE WITH NO EDGE IN ITS EIGHT NEIGHBOURS, not one whose own
gradient is small. The three-point gradient at the pixel *beside* a glyph stem
reads near zero while the pixel is still half covered by the stem in one
renderer and not the other, so the undilated mask calls it flat and it can
legitimately differ: 0.87% of the linear figure's "flat" pixels at 1x. Dilating
by one pixel takes that to zero on every figure, which is what lets the bar be
exact.

A GLOBAL "95% OF ALL PIXELS IDENTICAL" WAS THE FIRST BAR AND IT MEASURED THE
WRONG THING -- how much of the canvas is blank. The ring is 720x720 and mostly
white, and passed at 98.3%; the linear figure is 720x123 and almost entirely
ink, and failed at 92.2% with ZERO gross differences and 100% of its residue on
an edge. A threshold a correct renderer fails for being densely drawn is a
threshold that will be widened until it means nothing.

A count-based trend was tried here first and thrown away: at 1x the figure has
ONE grossly differing pixel, so any ratio against it is noise. Sharpness of the
discriminator matters more than the size of the sample.

EVERY FIGURE IN THE MANIFEST, not one. `pl-draw` draws two: the ring, and the
horizontal track a linear molecule gets. They share the `Scene` and nothing
below it -- arcs and a label ring on one side, pentagons, site ticks and stacked
label rows on the other -- so judging `map.svg` alone said nothing at all about
the linear renderer. The Rust side writes `FIGURES`; adding a third figure is
one line there and this file needs no edit.

Run from the repository root, after:

    cargo test -p pl-draw --test render
"""

import io
import os
import sys

try:
    import numpy as np
    import resvg_py
    from PIL import Image
except ImportError as e:
    print(f"missing an oracle: {e}", file=sys.stderr)
    sys.exit(2)

FONTS = [
    "crates/pl-draw/fonts/LiberationSans-Regular.ttf",
    "crates/pl-draw/fonts/LiberationSans-Bold.ttf",
]


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    d = os.path.join(root, "target", "tmp", "render")
    manifest = os.path.join(d, "FIGURES")
    if not os.path.exists(manifest):
        print(f"no figure manifest at {d}", file=sys.stderr)
        print("run: cargo test -p pl-draw --test render", file=sys.stderr)
        return 2
    stems = open(manifest, encoding="utf8").read().split()
    if not stems:
        print(f"{manifest} names no figures", file=sys.stderr)
        return 2

    fonts = [os.path.join(root, f) for f in FONTS]
    for f in fonts:
        if not os.path.exists(f):
            print(f"missing font {f}", file=sys.stderr)
            return 2

    scales = [int(s) for s in open(os.path.join(d, "SCALES")).read().split()]
    bad = 0
    fractions = []
    for stem, scale in [(s, k) for s in stems for k in scales]:
        svg_path = os.path.join(d, f"{stem}.svg")
        if not os.path.exists(svg_path):
            print(f"{manifest} names {stem}, but {svg_path} is not there", file=sys.stderr)
            return 2
        svg = open(svg_path, encoding="utf8").read()
        ours = np.asarray(
            Image.open(os.path.join(d, f"{stem}@{scale}x.png")).convert("RGB")
        ).astype(int)
        ref_png = resvg_py.svg_to_bytes(
            svg_string=svg,
            background="#ffffff",
            skip_system_fonts=True,
            font_files=fonts,
            sans_serif_family="Liberation Sans",
            zoom=float(scale),
        )
        ref = np.asarray(Image.open(io.BytesIO(bytes(ref_png))).convert("RGB")).astype(int)

        if ours.shape != ref.shape:
            print(f"  FAIL {stem} {scale}x: we drew {ours.shape}, resvg drew {ref.shape}")
            bad += 1
            continue

        delta = np.abs(ours - ref).max(axis=2)
        n = delta.size
        same = (delta == 0).sum() / n
        near = (delta <= 2).sum() / n
        ys, xs = np.nonzero(delta > 64)
        gross = len(ys)
        fractions.append(gross / n)

        # Is each gross difference on an edge? Measured on the REFERENCE, so
        # our own output cannot define away its own mistakes.
        gy, gx = np.gradient(ref.mean(axis=2))
        grad = np.hypot(gy, gx)
        on_edge = float((grad[ys, xs] > 10).mean()) if gross else 1.0

        # Flat means "no edge in the eight neighbours" -- see the header for the
        # half-covered pixel beside a glyph stem that the undilated mask calls
        # flat and that two correct renderers may legitimately disagree about.
        edge = grad > 10
        near_edge = np.zeros_like(edge)
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                near_edge |= np.roll(np.roll(edge, dy, 0), dx, 1)
        flat = ~near_edge
        flat_bad = int((delta[flat] != 0).sum())

        print(
            f"  {stem} {scale}x {ours.shape[1]}x{ours.shape[0]}: {same:7.3%} identical, "
            f"{near:7.3%} within 2/255, {gross} px off by >64 ({gross / n:.4%}), "
            f"{on_edge:.2%} of those on an edge, {flat_bad} of {int(flat.sum())} flat px "
            f"differ, mean {np.abs(ours - ref).mean():.4f}/255"
        )
        if flat_bad:
            fy, fx = np.nonzero(flat & (delta != 0))
            worst = [(int(x), int(y), int(delta[y, x])) for x, y in zip(fx, fy)][:6]
            print(
                f"  FAIL {stem} {scale}x: {flat_bad} pixel(s) differ where there is no "
                f"edge to disagree about -- that is geometry, not antialiasing. "
                f"First few (x, y, delta): {worst}"
            )
            bad += 1
        if gross / n > 0.005:
            print(f"  FAIL {stem} {scale}x: {gross / n:.3%} of pixels differ grossly")
            bad += 1
        if on_edge < 0.98:
            flat = [
                (int(x), int(y)) for x, y in zip(xs, ys) if grad[y, x] <= 10
            ][:6]
            print(
                f"  FAIL {stem} {scale}x: {1 - on_edge:.2%} of the gross differences are in "
                f"FLAT regions, where two correct renderers must agree exactly -- "
                f"that is geometry, not antialiasing. First few: {flat}"
            )
            bad += 1

    print("agrees with resvg" if bad == 0 else f"{bad} check(s) failed")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
