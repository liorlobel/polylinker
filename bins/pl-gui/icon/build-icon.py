"""Build polylinker.ico AND the window icon from polylinker.svg, and prove what
is in them.

`bins/pl-gui/build.rs` links the .ico straight into polylinker.exe and names
this script in its refusal message, so the path has to work from anywhere:
`python bins/pl-gui/icon/build-icon.py` from the repository root does the same
thing as running it from this directory. SRC and both outputs are resolved
against this file rather than the working directory, which is why -- the earlier
version read a bare "icon_B3.svg" relative to the cwd, and that name had not
existed since the master was renamed to polylinker.svg.

Sizes are the ones Windows actually asks for: 16 (Explorer detail, taskbar),
20 and 24 (125%/150% DPI of 16), 32 (Explorer medium, Alt-Tab), 40 (200% of
20), 48 (Explorer large), 64, 128 and 256 (extra-large, and the Add/Remove
Programs and installer art).

Transparent background, not white: the icon sits on a taskbar and a Start Menu
tile whose colour the user chose, and a white plate around it would be visible
on every one of them.

# The second output, and why this script grew one

The .ico is for the SHELL. Explorer, the Start Menu shortcut and Add/Remove
Programs read it out of the linked `.res`; the RUNNING WINDOW does not. winit
does not look at the executable's resources at all, and the taskbar button of a
live window, Alt-Tab and the title bar are all fed from whatever
`egui::ViewportBuilder::with_icon` was handed. That argument is an
`egui::IconData` -- raw RGBA8 plus a width and a height -- and there is no path
from a `.ico` to one without a PNG decoder, which this project does not have and
does not want (`pl-draw` writes PNG; nothing in the repository reads one).

So the window icon is a raw RGBA blob, written HERE, from the same `Image`
object that becomes the .ico's 64 px frame. Not a second render, not a second
drawing, not a resample: `frames[SIZES.index(WINDOW_PX)]` is used twice. That is
what makes the drift impossible to introduce by editing this file -- and
`xcheck_icon.py` is what makes it impossible to introduce by editing one output
and not the other.

WHY 64 AND NOT 16, 128 OR 256. `IconData` holds ONE image and eframe rescales it
-- Lanczos3, to `GetSystemMetrics(SM_CXICON)` for the taskbar and `SM_CXSMICON`
for the title bar -- so the size is a single compromise rather than the .ico's
nine. 64 is `SM_CXICON` exactly at 200% DPI, the taskbar and Alt-Tab size on the
machine this is developed on, and 32 (100% DPI taskbar) and 16 (title bar, 100%
DPI) are integer halvings of it, so the four hard-edged rectangles come out of
the downscale without fractional coverage on their edges. Upscaling is the case
worth avoiding and 64 never meets it.
It costs 64 * 64 * 4 = 16,384 bytes in the binary, and that is measured: the
release polylinker.exe went from 12,897,280 bytes to 12,913,664 on 2026-08-05,
a difference of exactly 16,384, or 0.127%. 128 px would cover 400% DPI as well
and cost 65,536; no panel this ships to runs at 400%, and an icon four times the
size of anything asked for is four times the bytes for nothing.
"""

import hashlib
import io
from pathlib import Path

import resvg_py
from PIL import Image

HERE = Path(__file__).resolve().parent
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
# The one frame the running window gets. Must be in SIZES: the whole argument
# above is that the blob and an .ico frame are the same rasterisation.
WINDOW_PX = 64
SRC = HERE / "polylinker.svg"
OUT = HERE / "polylinker.ico"
RGBA = HERE / f"polylinker-{WINDOW_PX}.rgba"

svg = SRC.read_text(encoding="utf-8")
frames = []
for px in SIZES:
    b = bytes(resvg_py.svg_to_bytes(svg_string=svg, width=px, height=px))
    im = Image.open(io.BytesIO(b)).convert("RGBA")
    assert im.size == (px, px), f"{px}: resvg returned {im.size}"
    frames.append(im)

# PIL writes every requested size from the largest frame unless each is given
# explicitly, and a 16 px icon downsampled from 256 by a general resampler is
# not the same picture as one resvg rasterised at 16 -- the second is what the
# contact sheet judged. So the largest frame is saved with the rest appended.
frames[-1].save(OUT, format="ICO", sizes=[(p, p) for p in SIZES], append_images=frames[:-1])

# The window icon: the SAME frame object, straight to bytes.
#
# `Image.tobytes()` on an RGBA image is row-major, top row first, four
# non-premultiplied bytes per pixel in R,G,B,A order -- which is `IconData`'s
# layout exactly, so `main.rs` can `include_bytes!` this and hand the slice over
# with no reordering step to get wrong.
window = frames[SIZES.index(WINDOW_PX)]
blob = window.tobytes()
assert len(blob) == WINDOW_PX * WINDOW_PX * 4, f"{len(blob)} bytes is not RGBA8"
RGBA.write_bytes(blob)

# Read them both back and report what is actually in the files, rather than what
# was asked for.
ico = Image.open(OUT)
got = sorted(ico.info["sizes"])
print(f"{OUT.name}: {OUT.stat().st_size} bytes")
print("  sizes in file:", ", ".join(f"{w}x{h}" for w, h in got))
missing = [s for s in SIZES if (s, s) not in got]
print("  missing:", missing or "none")

# Transparency survived?
ico.size = got[0]
smallest = ico.convert("RGBA")
corner = smallest.getpixel((0, 0))
print(f"  16px corner pixel: {corner}  (alpha 0 = transparent, as intended)")

# The two digests `the_window_icon_and_the_ico_are_one_generation` in
# bins/pl-gui/src/main.rs asserts. Printed rather than kept anywhere here: a
# record in this file would be a record written by the same run that wrote the
# bytes, which can only ever agree with itself. The test is the second party.
print(f"{RGBA.name}: {RGBA.stat().st_size} bytes, {WINDOW_PX}x{WINDOW_PX} RGBA8")
opaque = {tuple(blob[i : i + 3]) for i in range(0, len(blob), 4) if blob[i + 3] == 255}
print("  opaque colours:", ", ".join("#%02X%02X%02X" % c for c in sorted(opaque)))
print("\nthe two sha256 that bins/pl-gui/src/main.rs holds:")
print(f'    {OUT.name:24} "{hashlib.sha256(OUT.read_bytes()).hexdigest()}"')
print(f'    {RGBA.name:24} "{hashlib.sha256(blob).hexdigest()}"')
