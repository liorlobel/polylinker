"""Our zlib streams, judged by Python's zlib.

`crates/pl-draw/src/deflate/tests.rs` round-trips the encoder against a decoder
written from RFC 1951 *in the same repository*. Two implementations written by
one author from one reading of one spec can be wrong together: read `HDIST` as
the count rather than the count minus one in both, and both agree while every
real decoder rejects the stream.

Python's `zlib` is CPython's binding to the reference implementation. It is not
ours, it did not read our reading, and it is what the world will use to open the
PNGs this crate writes. That makes it the oracle.

Two things are checked, and the second is the one that costs something:

  1. `zlib.decompress(stream) == input`, for every case the Rust wrote. This is
     the correctness claim.
  2. The stream is also fed to a **streaming** decompressor one byte at a time.
     A stream can decode correctly in one shot and still be malformed for a
     reader that consumes it incrementally -- a wrong final-block bit is the
     classic case, because `decompress` stops when it has the whole output and
     never notices there was supposed to be more. Browsers and image libraries
     stream.

Run from the repository root, after:

    cargo test -p pl-draw --test zstream
"""

import os
import re
import sys
import zlib


def target_tmpdir(root):
    """Where Cargo puts CARGO_TARGET_TMPDIR for this test binary."""
    return os.path.join(root, "target", "tmp", "zstream")


def module_prose(root):
    """`deflate.rs`'s comment text, unwrapped, so a claim reads as one line."""
    path = os.path.join(root, "crates", "pl-draw", "src", "deflate.rs")
    out = []
    for line in open(path, encoding="utf8"):
        t = line.strip()
        for marker in ("//!", "///"):
            if t.startswith(marker):
                out.append(t[len(marker) :].strip())
                break
    return re.sub(r"\s+", " ", " ".join(out))


def check_size_claim(root, sizes):
    """The `zlib -9` sentence in `deflate.rs`, against `zlib -9`.

    That sentence -- "the totals are within 1%, and this is smaller on N of the
    M cases" -- was the one measurement in the module with no oracle anywhere
    in the repository. It said two until 2026-08-04, when a recount against
    CPython's `zlib.compress(raw, 9)` over the same corpus made it three:
    `one-symbol` (28 vs 29), `window-edge` (453 vs 479) and `map-scanlines`
    (1896 vs 1897). Nothing went red, because nothing was looking.

    This is where it belongs. The comparison needs a reference DEFLATE encoder
    and there is none under `crates/` by design, so the Rust suite cannot make
    it; the streams are already here and so is `zlib`.
    """
    prose = module_prose(root)
    ours = sum(o for o, _ in sizes.values())
    ref = sum(r for _, r in sizes.values())
    wins = sorted(n for n, (o, r) in sizes.items() if o < r)
    drift = (ours - ref) / ref * 100.0

    bad = 0

    def claim(pattern, what):
        nonlocal bad
        m = re.search(pattern, prose)
        if m is None:
            print(f"  FAIL deflate.rs no longer states {what}: /{pattern}/ does not match")
            bad += 1
            return None
        return m

    m = claim(r"smaller on (\d+) of the (\d+) cases", "how many cases it beats zlib -9 on")
    if m:
        if int(m.group(2)) != len(sizes):
            print(f"  FAIL deflate.rs says {m.group(2)} cases; the corpus has {len(sizes)}")
            bad += 1
        if int(m.group(1)) != len(wins):
            print(
                f"  FAIL deflate.rs says smaller on {m.group(1)} cases; "
                f"measured {len(wins)}: {', '.join(wins)}"
            )
            bad += 1

    m = claim(r"totals are within (\d+)%", "the total-size bound")
    if m and abs(drift) > float(m.group(1)):
        print(f"  FAIL deflate.rs claims within {m.group(1)}%; measured {drift:+.3f}%")
        bad += 1

    m = claim(r"([\d,]+) bytes against ([\d,]+), ([+-][\d.]+)%", "the totals it was measured at")
    if m:
        want = (ours, ref, round(drift, 3))
        got = (
            int(m.group(1).replace(",", "")),
            int(m.group(2).replace(",", "")),
            float(m.group(3)),
        )
        if got[:2] != want[:2] or abs(got[2] - want[2]) > 0.001:
            print(
                f"  FAIL deflate.rs quotes {got[0]} against {got[1]}, {got[2]:+}%; "
                f"measured {want[0]} against {want[1]}, {want[2]:+}%"
            )
            bad += 1

    # The cases it wins, by name and by the pair of byte counts. Named rather
    # than counted only, because "three" with the wrong three in mind is the
    # same defect one step along -- the map-scanlines win is already claimed by
    # `MAX_CHAIN`'s table, which is how it came to be left out of this count.
    for name in wins:
        ours_n, ref_n = sizes[name]
        if f"`{name}` ({ours_n} against {ref_n})" not in prose:
            print(
                f"  FAIL deflate.rs does not record `{name}` ({ours_n} against {ref_n}), "
                f"a case this encoder wins"
            )
            bad += 1
    for name, (ours_n, ref_n) in sizes.items():
        if ours_n >= ref_n and f"`{name}` ({ours_n} against" in prose:
            print(f"  FAIL deflate.rs lists `{name}` as a win; it is {ours_n} against {ref_n}")
            bad += 1

    print(
        f"  {'ok  ' if bad == 0 else 'FAIL'} vs zlib -9: {ours} bytes against {ref} "
        f"({drift:+.3f}%), smaller on {len(wins)} of {len(sizes)}: {', '.join(wins)}"
    )
    return bad


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    d = sys.argv[2] if len(sys.argv) > 2 else target_tmpdir(root)
    manifest = os.path.join(d, "MANIFEST")
    if not os.path.exists(manifest):
        print(f"no streams at {d}", file=sys.stderr)
        print("run: cargo test -p pl-draw --test zstream", file=sys.stderr)
        return 2

    names = [n for n in open(manifest, encoding="utf8").read().split("\n") if n]
    if not names:
        print("the manifest is empty, so this would check nothing", file=sys.stderr)
        return 2

    bad = 0
    total_raw = total_z = 0
    sizes = {}
    for name in names:
        raw = open(os.path.join(d, name + ".raw"), "rb").read()
        z = open(os.path.join(d, name + ".z"), "rb").read()
        total_raw += len(raw)
        total_z += len(z)
        sizes[name] = (len(z), len(zlib.compress(raw, 9)))

        try:
            got = zlib.decompress(z)
        except Exception as e:  # noqa: BLE001 - any failure is the finding
            print(f"  FAIL {name}: zlib refused the stream: {e}")
            bad += 1
            continue
        if got != raw:
            where = next(
                (i for i, (a, b) in enumerate(zip(got, raw)) if a != b), min(len(got), len(raw))
            )
            print(
                f"  FAIL {name}: decoded {len(got)} bytes, expected {len(raw)}, "
                f"first difference at {where}"
            )
            bad += 1
            continue

        # Byte at a time, so a stream that is only valid when read whole fails.
        do = zlib.decompressobj()
        out = bytearray()
        try:
            for i in range(len(z)):
                out += do.decompress(z[i : i + 1])
            out += do.flush()
        except Exception as e:  # noqa: BLE001
            print(f"  FAIL {name}: fine in one shot, broken when streamed: {e}")
            bad += 1
            continue
        if bytes(out) != raw:
            print(f"  FAIL {name}: streamed decode differs from the one-shot decode")
            bad += 1
            continue
        if not do.eof:
            print(f"  FAIL {name}: the stream never declared a final block")
            bad += 1
            continue
        if do.unused_data:
            print(f"  FAIL {name}: {len(do.unused_data)} bytes of trailing rubbish")
            bad += 1
            continue

        ratio = (len(raw) / len(z)) if len(z) else 0.0
        print(f"  ok   {name}: {len(raw)} -> {len(z)} bytes ({ratio:.1f}x)")

    # Counted apart from `bad`, so a wrong number in a comment never makes the
    # summary line understate how many streams zlib actually took.
    prose_bad = check_size_claim(root, sizes)

    print(
        f"{len(names) - bad} of {len(names)} streams accepted by zlib; "
        f"{total_raw} bytes in, {total_z} out"
    )
    return 1 if (bad or prose_bad) else 0


if __name__ == "__main__":
    sys.exit(main())
