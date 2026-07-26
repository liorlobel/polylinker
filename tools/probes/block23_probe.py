"""Characterise the undocumented .dna blocks 2 and 3.

Question: are they regenerable caches (a writer can synthesise or omit them) or
do they carry irreplaceable user data (a writer must round-trip them)?

Strategy: relate their size to the sequence length, look at byte-value entropy
and structure, and check for compression magic / XML / repeating record widths.
"""
import struct
import sys
import glob
import os
import zlib
import math
from collections import Counter


def blocks(path):
    out = []
    size = os.path.getsize(path)
    with open(path, "rb") as fh:
        while True:
            hdr = fh.read(5)
            if len(hdr) < 5:
                break
            btype, blen = struct.unpack(">BI", hdr)
            payload = fh.read(blen)
            out.append((btype, blen, payload))
    return out, size


def entropy(b):
    if not b:
        return 0.0
    c = Counter(b)
    n = len(b)
    return -sum((v / n) * math.log2(v / n) for v in c.values())


def probe(path):
    bl, size = blocks(path)
    seq_len = None
    for t, l, p in bl:
        if t == 0:
            seq_len = l - 1
    res = {"name": os.path.basename(path)[:44], "seq_len": seq_len, "size": size}
    for target in (2, 3):
        for t, l, p in bl:
            if t == target:
                res[f"b{target}_len"] = l
                res[f"b{target}_ratio"] = (l / seq_len) if seq_len else None
                res[f"b{target}_head"] = p[:24]
                res[f"b{target}_ent"] = entropy(p[:200000])
                # how well does generic compression squeeze it?
                res[f"b{target}_zlib"] = len(zlib.compress(p[:400000], 6)) / min(len(p), 400000)
                res[f"b{target}_uniq"] = len(set(p[:200000]))
                break
    return res


def main(patterns):
    files = []
    for p in patterns:
        files.extend(glob.glob(p, recursive=True))
    files = sorted(set(files), key=os.path.getsize)

    print(f"{'seq_len':>9} {'b2_len':>10} {'b2/seq':>7} {'b2_ent':>6} {'b2_uq':>5} "
          f"{'b3_len':>10} {'b3/seq':>7} {'b3_ent':>6} {'b3_uq':>5}  name")
    rows = []
    for f in files:
        try:
            r = probe(f)
        except Exception as e:
            print(f"  FAIL {os.path.basename(f)}: {e}")
            continue
        rows.append(r)
        print(
            f"{(r.get('seq_len') or -1):>9} "
            f"{r.get('b2_len', -1):>10} {(r.get('b2_ratio') or 0):>7.3f} {(r.get('b2_ent') or 0):>6.2f} {r.get('b2_uniq', -1):>5} "
            f"{r.get('b3_len', -1):>10} {(r.get('b3_ratio') or 0):>7.3f} {(r.get('b3_ent') or 0):>6.2f} {r.get('b3_uniq', -1):>5}  "
            f"{r['name']}"
        )

    print("\n--- first 24 bytes of block 2 / block 3 (largest file) ---")
    big = max(rows, key=lambda r: r.get("seq_len") or 0)
    for target in (2, 3):
        h = big.get(f"b{target}_head", b"")
        print(f"  block {target}: {' '.join(f'{x:02x}' for x in h)}")
        print(f"           ascii: {''.join(chr(x) if 32 <= x < 127 else '.' for x in h)}")
        print(f"           zlib compressibility: {big.get(f'b{target}_zlib', 0):.3f} "
              f"(1.0 = incompressible)")

    print("\n--- interpretation hints ---")
    print("  ratio ~0.5  -> 4 bits/base (2 bases packed per byte)")
    print("  ratio ~1.0  -> 1 byte per base")
    print("  ratio ~2.0  -> 2 bytes per base")
    print("  high entropy + incompressible -> packed/encoded data, not text")


if __name__ == "__main__":
    main(sys.argv[1:])
