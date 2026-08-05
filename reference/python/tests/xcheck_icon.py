"""The window icon and the shell icon are the same drawing.

    python reference/python/tests/xcheck_icon.py .

`bins/pl-gui/icon/` holds one master (`polylinker.svg`, four rectangles) and two
generated artefacts:

  * `polylinker.ico`, nine resvg-rasterised frames, linked into polylinker.exe
    as a Windows resource and read by Explorer, the Start Menu shortcut and
    Add/Remove Programs;
  * `polylinker-64.rgba`, 64x64 raw RGBA8, `include_bytes!`d by
    `bins/pl-gui/src/main.rs` and handed to `ViewportBuilder::with_icon`, which
    is what the TASKBAR BUTTON OF THE RUNNING WINDOW, Alt-Tab and the title bar
    show. winit does not read the executable's resources, so this second file is
    not a duplicate of the first -- it is the only way the window gets an icon at
    all.

Two artefacts from one master is exactly the shape that goes stale. `main.rs`
holds a sha256 of each, which proves they came out of ONE run of `build-icon.py`
but cannot compare a single pixel: the `.ico`'s frames are PNG, and this project
has no PNG *decoder* -- `pl-draw` writes them and nothing reads them. That is
this file's job, and it needs PIL to do it.

Three comparisons, all byte-for-byte:

  1. **the `.ico`'s 64x64 frame, decoded, against `polylinker-64.rgba`.** This is
     the one that catches drift. Regenerate one artefact and not the other, or
     hand-edit either, and this goes red.
  2. `polylinker-64.rgba` against a fresh resvg render of `polylinker.svg` at 64.
     Catches BOTH artefacts being stale together, which comparison 1 cannot see.
  3. every other frame of the `.ico` against a fresh resvg render at its own
     size. This closes the gap `tools/ci.ps1` documents on the step `the built
     binaries carry their icon and version resource`: that step proves the .exe
     carries the `.ico`'s bytes and is blind to the `.ico` being a stale
     rasterisation of an edited `polylinker.svg`.

Nothing here is compared against a re-encode: the reference for a frame is
resvg's own RGBA output, never a PNG this project wrote.

`compare` is deliberately dependency-free and importable, so
`xcheck_oracles.py` can inject a broken pair and demand it notice -- including
the empty-list case, which is the failure this project has shipped more often
than any other: a checker that compared nothing and exited 0.

Exit status: 0 all comparisons clean, 1 a real disagreement, 2 the checker could
not run (missing file, missing PIL or resvg_py).
"""

import io
import os
import struct
import sys

ICON_DIR = os.path.join("bins", "pl-gui", "icon")
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def compare(pairs):
    """Byte-for-byte, over `(what, want, got)`; returns a list of problems.

    An EMPTY `pairs` is itself a problem, and that is not defensive
    programming. Every silent oracle this project has found reported success
    for a run in which it compared nothing -- a glob that matched no files, a
    corpus that was not there, a directory left over from a previous run. Here
    the equivalent is an `.ico` whose directory this file failed to parse, which
    would otherwise leave a green step measuring zero frames.
    """
    if not pairs:
        return ["nothing was compared, so nothing was proved"]
    problems = []
    for what, want, got in pairs:
        if len(want) != len(got):
            problems.append(f"{what}: {len(got)} bytes against {len(want)}")
            continue
        bad = [i for i in range(len(want)) if want[i] != got[i]]
        if bad:
            problems.append(
                f"{what}: {len(bad)} of {len(want)} bytes differ, "
                f"first at byte {bad[0]} (pixel {bad[0] // 4}, "
                f"channel {'RGBA'[bad[0] % 4]}): "
                f"{want[bad[0]]} became {got[bad[0]]}"
            )
    return problems


def ico_frames(data):
    """`{(w, h): png_bytes}` from an ICONDIR, without decoding anything.

    The directory and the frame offsets are uncompressed, so this is plain
    struct work. Width and height are single bytes with 0 meaning 256, which is
    why an `.ico` cannot name a frame larger than that.
    """
    reserved, kind, count = struct.unpack_from("<HHH", data, 0)
    if (reserved, kind) != (0, 1):
        raise ValueError(f"not an ICONDIR of type 1: reserved={reserved} type={kind}")
    out = {}
    for i in range(count):
        w, h, _colours, _res, _planes, _bits, size, off = struct.unpack_from(
            "<BBBBHHII", data, 6 + i * 16
        )
        out[(w or 256, h or 256)] = data[off : off + size]
    if len(out) != count:
        raise ValueError(f"the directory declares {count} frames and names {len(out)} sizes")
    return out


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    try:
        import resvg_py
        from PIL import Image
    except ImportError as e:
        print(f"{e}; this checker needs PIL and resvg_py", file=sys.stderr)
        return 2

    here = os.path.join(root, ICON_DIR)
    try:
        with open(os.path.join(here, "polylinker.svg"), encoding="utf-8") as f:
            svg = f.read()
        with open(os.path.join(here, "polylinker.ico"), "rb") as f:
            ico = f.read()
    except OSError as e:
        print(f"{e}", file=sys.stderr)
        return 2

    # The window icon's size is READ OFF THE BLOB, not written here. A second
    # statement of 64 in this file would be one more place to go stale, which is
    # the defect the whole step is about. `main.rs` asserts the same square
    # relation against its own `ICON_PX`, so the number is joined at both ends.
    blobs = [n for n in os.listdir(here) if n.endswith(".rgba")]
    if len(blobs) != 1:
        print(f"expected exactly one .rgba blob in {here}, found {blobs}", file=sys.stderr)
        return 2
    with open(os.path.join(here, blobs[0]), "rb") as f:
        blob = f.read()
    px = round((len(blob) / 4) ** 0.5)
    if px * px * 4 != len(blob):
        print(f"{blobs[0]} is {len(blob)} bytes, which is no square RGBA8 image", file=sys.stderr)
        return 2

    def render(size):
        """resvg's own RGBA for the master at `size`, via its PNG output."""
        raw = bytes(resvg_py.svg_to_bytes(svg_string=svg, width=size, height=size))
        im = Image.open(io.BytesIO(raw)).convert("RGBA")
        if im.size != (size, size):
            raise ValueError(f"resvg returned {im.size} for {size}")
        return im.tobytes()

    try:
        frames = ico_frames(ico)
    except (ValueError, struct.error) as e:
        print(f"polylinker.ico: {e}", file=sys.stderr)
        return 2

    def decode(size):
        """One frame of the `.ico`, decoded by PIL, as straight RGBA8."""
        png = frames[(size, size)]
        if not png.startswith(PNG_SIGNATURE):
            raise ValueError(f"the {size} px frame is not PNG-compressed")
        return Image.open(io.BytesIO(png)).convert("RGBA").tobytes()

    pairs = []
    try:
        if (px, px) not in frames:
            print(
                f"polylinker.ico has no {px}x{px} frame, so the window icon is not "
                f"one of its frames; sizes present: {sorted(frames)}",
                file=sys.stderr,
            )
            return 1
        # 1. THE DRIFT CHECK.
        pairs.append((f"the .ico's {px}x{px} frame vs {blobs[0]}", decode(px), blob))
        # 2. and 3. Both artefacts against the master they claim to come from.
        pairs.append((f"{blobs[0]} vs resvg at {px}", render(px), blob))
        for w, h in sorted(frames):
            pairs.append((f"the .ico's {w}x{h} frame vs resvg at {w}", render(w), decode(w)))
    except (ValueError, OSError) as e:
        print(f"{e}", file=sys.stderr)
        return 2

    problems = compare(pairs)
    for p in problems:
        print(f"  !! {p}")
    opaque = sorted({tuple(blob[i : i + 3]) for i in range(0, len(blob), 4) if blob[i + 3] == 255})
    print(
        f"  {len(pairs)} byte-for-byte comparisons over {len(frames)} .ico frames "
        f"and one {px}x{px} window icon; "
        + ", ".join("#%02X%02X%02X" % c for c in opaque)
        + f"; {len(problems)} disagreement(s)"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
