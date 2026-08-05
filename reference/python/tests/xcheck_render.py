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

WHY THE THRESHOLDS ARE WHAT THEY ARE. Two correct antialiasing implementations
do not agree bit for bit: this one samples 16 sub-scanlines per row and computes
exact horizontal coverage, resvg accumulates analytic area. Edge pixels differ
by small amounts and interior pixels do not. So the assertions are on the shape
of the disagreement, not on a single average:

  - the great majority of pixels identical, and nearly all within a hair;
  - **every grossly differing pixel must lie on an edge.** This is the real
    test. Antialiasing can only disagree where there is a gradient to disagree
    about; a wrong arc, a wrong winding, a misplaced glyph or a hole in a stroke
    all put differing pixels in FLAT regions, where two correct renderers must
    agree exactly. Measured on the first run at 4x: 490 gross pixels, 100% of
    them on an edge (median |grad| 114 against 0 for the canvas at large),
    scattered over 167 separate 8x8 tiles at about 3 px each.

A count-based trend was tried here first and thrown away: at 1x the figure has
ONE grossly differing pixel, so any ratio against it is noise. Sharpness of the
discriminator matters more than the size of the sample.

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
    svg_path = os.path.join(d, "map.svg")
    if not os.path.exists(svg_path):
        print(f"no figure at {d}", file=sys.stderr)
        print("run: cargo test -p pl-draw --test render", file=sys.stderr)
        return 2

    svg = open(svg_path, encoding="utf8").read()
    fonts = [os.path.join(root, f) for f in FONTS]
    for f in fonts:
        if not os.path.exists(f):
            print(f"missing font {f}", file=sys.stderr)
            return 2

    scales = [int(s) for s in open(os.path.join(d, "SCALES")).read().split()]
    bad = 0
    fractions = []
    for scale in scales:
        ours = np.asarray(
            Image.open(os.path.join(d, f"map@{scale}x.png")).convert("RGB")
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
            print(f"  FAIL {scale}x: we drew {ours.shape}, resvg drew {ref.shape}")
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

        print(
            f"  {scale}x {ours.shape[1]}x{ours.shape[0]}: {same:7.3%} identical, "
            f"{near:7.3%} within 2/255, {gross} px off by >64 ({gross / n:.4%}), "
            f"{on_edge:.2%} of those on an edge, mean {np.abs(ours - ref).mean():.4f}/255"
        )
        if same < 0.95:
            print(f"  FAIL {scale}x: only {same:.2%} of pixels are identical")
            bad += 1
        if gross / n > 0.005:
            print(f"  FAIL {scale}x: {gross / n:.3%} of pixels differ grossly")
            bad += 1
        if on_edge < 0.98:
            flat = [
                (int(x), int(y)) for x, y in zip(xs, ys) if grad[y, x] <= 10
            ][:6]
            print(
                f"  FAIL {scale}x: {1 - on_edge:.2%} of the gross differences are in "
                f"FLAT regions, where two correct renderers must agree exactly -- "
                f"that is geometry, not antialiasing. First few: {flat}"
            )
            bad += 1

    print("agrees with resvg" if bad == 0 else f"{bad} check(s) failed")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
