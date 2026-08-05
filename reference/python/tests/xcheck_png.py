"""Our PNGs, opened by PIL.

`crates/pl-draw/src/png/tests.rs` parses the file with the same understanding
that wrote it: it knows where IHDR is because it put IHDR there. That catches a
mistake in the code and not a mistake in the reading.

PIL is a different decoder, written by other people, and stands in for every
program that will open these figures. Four things are checked:

  1. the file opens at all;
  2. the mode and size are what IHDR claimed;
  3. **every pixel equals the raw RGB the Rust wrote beside it** -- compared
     against the buffer, never against a re-encode, so no second encoder of ours
     is in the loop;
  4. `info["dpi"]` is the physical resolution that was asked for, because "at a
     specified physical width and dpi" is the roadmap row this implements, and a
     figure whose dpi does not survive is a figure that arrives in a manuscript
     at a size nobody chose.

Run from the repository root, after:

    cargo test -p pl-draw --test pngfile
"""

import os
import sys

try:
    from PIL import Image
except ImportError:
    print("PIL is not installed", file=sys.stderr)
    sys.exit(2)


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    d = sys.argv[2] if len(sys.argv) > 2 else os.path.join(root, "target", "tmp", "png")
    manifest = os.path.join(d, "MANIFEST")
    if not os.path.exists(manifest):
        print(f"no PNGs at {d}", file=sys.stderr)
        print("run: cargo test -p pl-draw --test pngfile", file=sys.stderr)
        return 2

    rows = [r for r in open(manifest, encoding="utf8").read().split("\n") if r]
    if not rows:
        print("the manifest is empty, so this would check nothing", file=sys.stderr)
        return 2

    bad = 0
    for row in rows:
        name, w, h, dpi = row.split("\t")
        w, h = int(w), int(h)
        path = os.path.join(d, name + ".png")
        try:
            im = Image.open(path)
            im.load()
        except Exception as e:  # noqa: BLE001 - any failure is the finding
            print(f"  FAIL {name}: PIL would not open it: {e}")
            bad += 1
            continue

        if im.size != (w, h):
            print(f"  FAIL {name}: PIL sees {im.size}, IHDR said ({w}, {h})")
            bad += 1
            continue
        if im.mode != "RGB":
            print(f"  FAIL {name}: mode {im.mode}, expected RGB")
            bad += 1
            continue

        want = open(os.path.join(d, name + ".rgb"), "rb").read()
        got = im.convert("RGB").tobytes()
        if got != want:
            n = sum(1 for a, b in zip(got, want) if a != b)
            first = next((i for i, (a, b) in enumerate(zip(got, want)) if a != b), None)
            px = first // 3 if first is not None else -1
            print(
                f"  FAIL {name}: {n} of {len(want)} bytes differ; first at byte "
                f"{first}, pixel ({px % w}, {px // w})"
            )
            bad += 1
            continue

        if dpi == "-":
            if "dpi" in im.info:
                print(f"  FAIL {name}: no dpi was asked for, file claims {im.info['dpi']}")
                bad += 1
                continue
            note = "no dpi, as asked"
        else:
            want_dpi = float(dpi)
            got_dpi = im.info.get("dpi")
            if got_dpi is None:
                print(f"  FAIL {name}: {want_dpi} dpi was asked for, file carries none")
                bad += 1
                continue
            # pHYs stores whole pixels per metre, so half of one is the floor on
            # what any encoder can do: 0.5 * 0.0254 = 0.0127 dpi.
            if max(abs(float(v) - want_dpi) for v in got_dpi) > 0.0127:
                print(f"  FAIL {name}: asked {want_dpi} dpi, PIL reads {got_dpi}")
                bad += 1
                continue
            note = f"{got_dpi[0]:.4f} dpi"

        size = os.path.getsize(path)
        raw = len(want)
        print(f"  ok   {name}: {w}x{h} RGB, {note}, {size} bytes ({raw / size:.1f}x)")

    print(f"{len(rows) - bad} of {len(rows)} PNGs accepted by PIL")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
