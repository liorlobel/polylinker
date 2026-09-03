"""Build polylinker.icns from polylinker.svg, with nothing but the standard
library, and prove what is in it.

This is the macOS sibling of `build-icon.py`, and it is a separate script for
a reason worth stating rather than a failure to merge: `build-icon.py` needs
`resvg_py` and `PIL`, neither of which the machine that first needed a `.icns`
had, and the SVG this project draws is four axis-aligned rectangles. A
general SVG rasteriser is the right tool for the `.ico`, whose sizes were
judged on a contact sheet frame by frame; it is not the right dependency to
add for a picture whose exact pixel coverage can be computed in closed form.
So this script computes it: for every pixel, the fraction of its area under
each rectangle, alpha = that fraction, colour = the coverage-weighted mix
where two rectangles of different colour meet inside one pixel (which happens
at 16 px, where one pixel spans the 2-unit gap between the two rows). That is
exact box-filter antialiasing, and it is deterministic on any platform with a
Python.

Two consequences, both deliberate:

  * THE PIXELS ARE NOT THE .ICO's PIXELS. resvg antialiases with its own
    filter, so the 64 px frame here is not byte-equal to polylinker-64.rgba.
    They are one drawing rasterised twice, not one rasterisation copied, and
    `the_macos_icon_is_the_icns_on_disk` in bins/pl-gui/src/main.rs says so
    rather than claiming a kinship it cannot check.
  * THE CONTAINER IS WRITTEN HERE, NOT BY `iconutil`. An ICNS is a flat
    sequence of (type, length, payload) entries and Apple accepts PNG
    payloads for every size since 10.7, so there is nothing `iconutil` adds
    except a table of contents and, across macOS versions, bytes that differ.
    Writing the container directly makes the output a function of this file
    and the SVG, reproducible off a Mac, and pinnable by sha256 the way the
    .ico is. `iconutil --convert iconset polylinker.icns` is run afterwards
    where it exists, as the read-back that proves macOS parses the result --
    a writer that only agrees with itself proves nothing.

The eleven entries are the ones macOS asks for: 16, 32, 64, 128, 256, 512 and
1024 pixels at 1x (icp4 icp5 icp6 ic07 ic08 ic09 ic10) and the four @2x
aliases of 16, 32, 128 and 256 (ic11 ic12 ic13 ic14), which are the same
pixels as 32, 64, 256 and 512 and are written as the same bytes. Transparent
background, for the reason build-icon.py gives: the Dock and Finder paint
their own ground behind it.

Run from anywhere: `python3 bins/pl-gui/icon/build-icns.py` from the
repository root does the same thing as running it from this directory. It
reads the SVG only to REFUSE if the drawing has changed: the rectangles below
are the SVG's four strokes with their width applied, and a redrawn master
must be re-transcribed here and the digest re-pinned, which is what the
assertion on the SVG's sha256 forces.
"""

import hashlib
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = HERE / "polylinker.svg"
OUT = HERE / "polylinker.icns"

# polylinker.svg, transcribed: viewBox 0 0 256 256, four butt-capped strokes of
# width 46 -- a horizontal stroke from (x1, y) to (x2, y) is the rectangle
# x1..x2 by y-23..y+23. The two rows are y=104 and y=152, so the rows are
# 81..127 and 129..175 with a 2-unit gap between them.
SVG_SHA256 = "c7e33841fbfd852083092a042b513a88496298c41b7f8d4796ffe8f7368b7642"
BLUE = (0x00, 0x72, 0xB2)
ORANGE = (0xE6, 0x9F, 0x00)
RECTS = [
    # (x0, y0, x1, y1, rgb) in SVG units, half-open on the right/bottom.
    (18.0, 81.0, 112.0, 127.0, BLUE),
    (18.0, 129.0, 58.0, 175.0, BLUE),
    (144.0, 81.0, 238.0, 127.0, ORANGE),
    (90.0, 129.0, 238.0, 175.0, ORANGE),
]
VIEWBOX = 256.0

# (ICNS type, pixels). Order is the order written; macOS does not care, but a
# stable order is what makes the file reproducible.
ENTRIES = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
    (b"ic11", 32),  # 16x16@2x
    (b"ic12", 64),  # 32x32@2x
    (b"ic13", 256),  # 128x128@2x
    (b"ic14", 512),  # 256x256@2x
]


def overlap(a0, a1, b0, b1):
    """Length of the overlap of [a0, a1) and [b0, b1)."""
    lo = max(a0, b0)
    hi = min(a1, b1)
    return hi - lo if hi > lo else 0.0


def raster(px):
    """RGBA8 bytes, row-major, top row first, non-premultiplied -- IconData's
    layout and PNG's."""
    unit = VIEWBOX / px  # SVG units per pixel
    # Per-rectangle 1-D coverage along each axis, so the 2-D coverage is a
    # product and the inner loop is a multiply rather than a clip.
    cov = []
    for x0, y0, x1, y1, rgb in RECTS:
        cx = [overlap(i * unit, (i + 1) * unit, x0, x1) / unit for i in range(px)]
        cy = [overlap(j * unit, (j + 1) * unit, y0, y1) / unit for j in range(px)]
        cov.append((cx, cy, rgb))
    out = bytearray(px * px * 4)
    for j in range(px):
        for i in range(px):
            a = 0.0
            r = g = b = 0.0
            for cx, cy, (cr, cg, cb) in cov:
                c = cx[i] * cy[j]
                if c:
                    a += c
                    r += cr * c
                    g += cg * c
                    b += cb * c
            if a:
                # Non-overlapping rectangles, so a <= 1 up to float noise.
                a = min(a, 1.0)
                k = (j * px + i) * 4
                out[k] = round(r / a)
                out[k + 1] = round(g / a)
                out[k + 2] = round(b / a)
                out[k + 3] = round(a * 255.0)
    return bytes(out)


def png_chunk(kind, data):
    return (
        struct.pack(">I", len(data))
        + kind
        + data
        + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    )


def png(px, rgba):
    """A minimal RGBA PNG: colour type 6, bit depth 8, filter 0 on every row.
    Filter None rather than adaptive for the reason crates/pl-draw/src/png.rs
    gives -- on flat line art it is smaller, and here it is also simpler."""
    rows = bytearray()
    stride = px * 4
    for j in range(px):
        rows.append(0)
        rows += rgba[j * stride : (j + 1) * stride]
    ihdr = struct.pack(">IIBBBBB", px, px, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", ihdr)
        + png_chunk(b"IDAT", zlib.compress(bytes(rows), 9))
        + png_chunk(b"IEND", b"")
    )


def main():
    svg = SRC.read_bytes()
    got = hashlib.sha256(svg).hexdigest()
    if got != SVG_SHA256:
        # Not a failure of this script; a refusal to draw a picture it has not
        # been told about. The rectangles above are a transcription of the
        # SVG, and a transcription of a file that has changed is a lie with a
        # digest on it.
        sys.exit(
            f"{SRC.name} has changed (sha256 {got}, this script transcribes "
            f"{SVG_SHA256}). Re-transcribe RECTS from the new drawing, update "
            f"SVG_SHA256, and re-pin the .icns digest in bins/pl-gui/src/main.rs."
        )

    frames = {}
    for _, px in ENTRIES:
        if px not in frames:
            frames[px] = png(px, raster(px))

    body = b"".join(
        kind + struct.pack(">I", 8 + len(frames[px])) + frames[px]
        for kind, px in ENTRIES
    )
    icns = b"icns" + struct.pack(">I", 8 + len(body)) + body
    OUT.write_bytes(icns)

    print(f"{OUT.name}: {len(icns)} bytes, {len(ENTRIES)} entries, {len(frames)} distinct frames")
    for kind, px in ENTRIES:
        print(f"  {kind.decode()}  {px:5d} px  {len(frames[px]):7d} bytes")

    # Transparency and colour, read back from the smallest frame's raw pixels
    # rather than asserted: the corner must be clear and only the two inks
    # may appear at full alpha.
    small = raster(16)
    corner = tuple(small[0:4])
    opaque = {tuple(small[i : i + 3]) for i in range(0, len(small), 4) if small[i + 3] == 255}
    print(f"  16px corner pixel: {corner}  (alpha 0 = transparent, as intended)")
    print("  opaque colours at 16px:", ", ".join("#%02X%02X%02X" % c for c in sorted(opaque)))

    # The read-back: does macOS parse it? Only where iconutil exists; the
    # container is the same bytes everywhere, this is the one check that needs
    # Apple's reader. `--convert iconset` writes one PNG per entry it
    # understood, so the count is the assertion.
    iconutil = shutil.which("iconutil")
    if iconutil:
        with tempfile.TemporaryDirectory() as tmp:
            dest = Path(tmp) / "readback.iconset"
            subprocess.run([iconutil, "--convert", "iconset", "--output", str(dest), str(OUT)], check=True)
            back = sorted(p.name for p in dest.iterdir())
            print(f"  iconutil read back {len(back)} image(s): {', '.join(back)}")
            if len(back) != len(ENTRIES):
                sys.exit(f"iconutil read {len(back)} entries out of {len(ENTRIES)} written; the container is wrong")
    else:
        print("  iconutil not on PATH; the container was not read back by macOS here")

    # The digest `the_macos_icon_is_the_icns_on_disk` in bins/pl-gui/src/main.rs
    # holds. Printed rather than kept anywhere here, for build-icon.py's reason.
    print("\nthe sha256 that bins/pl-gui/src/main.rs holds:")
    print(f'    {OUT.name:24} "{hashlib.sha256(icns).hexdigest()}"  ({len(icns)} bytes)')


if __name__ == "__main__":
    main()
