"""Empirical probe of the SnapGene .dna container format.

Reads files as a stream of (type:uint8, length:uint32be, payload) blocks and
reports which block types actually occur in a real-world corpus, plus how much
of each file is accounted for. No SnapGene code or data is used -- this is
black-box observation of the container framing only.
"""
import struct
import sys
import glob
import os
from collections import Counter, defaultdict

# Names for block types that are publicly documented / inferable from payload.
BLOCK_NAMES = {
    0: "DNA sequence",
    1: "compressed DNA",
    2: "unknown-2",
    3: "unknown-3",
    5: "primers (XML)",
    6: "notes (XML)",
    7: "history tree (XML)",
    8: "additional seq properties (XML)",
    9: "file header",
    10: "features (XML)",
    11: "history node",
    13: "unknown-13",
    14: "unknown-14",
    16: "alignable sequences (XML)",
    17: "alignment trace data",
    18: "uracil positions",
    19: "custom DNA colors (XML)",
    20: "unknown-20",
    21: "unknown-21",
    28: "unknown-28",
    30: "unknown-30",
}


def parse_blocks(path, max_blocks=100000):
    """Yield (block_type, length, payload_head, offset) for each container block."""
    size = os.path.getsize(path)
    out = []
    with open(path, "rb") as fh:
        # Header block must come first.
        first = fh.read(5)
        if len(first) < 5:
            raise ValueError("file too short")
        btype, blen = struct.unpack(">BI", first)
        if btype != 9:
            raise ValueError(f"first block type is {btype}, expected 9")
        payload = fh.read(blen)
        if payload[:8] != b"SnapGene":
            raise ValueError("missing SnapGene magic")
        # payload: magic(8) + type(2) + exportVersion(2) + importVersion(2)
        ftype, exp_ver, imp_ver = struct.unpack(">HHH", payload[8:14])
        out.append((9, blen, payload, 0))

        while len(out) < max_blocks:
            offset = fh.tell()
            hdr = fh.read(5)
            if len(hdr) < 5:
                break
            btype, blen = struct.unpack(">BI", hdr)
            if blen > size:  # framing desync guard
                raise ValueError(f"implausible block length {blen} at offset {offset}")
            payload = fh.read(blen)
            if len(payload) < blen:
                raise ValueError(f"truncated block {btype} at offset {offset}")
            out.append((btype, blen, payload, offset))
    return out, ftype, exp_ver, imp_ver, size, fh.tell() if False else None


def describe(path):
    blocks, ftype, exp_ver, imp_ver, size, _ = parse_blocks(path)
    consumed = sum(5 + b[1] for b in blocks)
    seq_len = None
    topology = None
    flags = None
    for btype, blen, payload, _off in blocks:
        if btype == 0 and payload:
            flags = payload[0]
            seq_len = blen - 1
            topology = "circular" if (flags & 0x01) else "linear"
    n_features = None
    for btype, blen, payload, _off in blocks:
        if btype == 10:
            n_features = payload.count(b"<Feature ")
    n_primers = None
    for btype, blen, payload, _off in blocks:
        if btype == 5:
            n_primers = payload.count(b"<Primer ")
    return {
        "path": path,
        "size": size,
        "ftype": ftype,
        "exp_ver": exp_ver,
        "imp_ver": imp_ver,
        "blocks": blocks,
        "consumed": consumed,
        "seq_len": seq_len,
        "flags": flags,
        "topology": topology,
        "n_features": n_features,
        "n_primers": n_primers,
    }


def main(patterns):
    files = []
    for p in patterns:
        files.extend(glob.glob(p, recursive=True))
    files = sorted(set(files))
    print(f"corpus: {len(files)} files\n")

    type_counter = Counter()
    type_bytes = Counter()
    failures = []
    rows = []
    flag_counter = Counter()

    for f in files:
        try:
            d = describe(f)
        except Exception as e:
            failures.append((f, repr(e)))
            continue
        for btype, blen, _payload, _off in d["blocks"]:
            type_counter[btype] += 1
            type_bytes[btype] += blen
        if d["flags"] is not None:
            flag_counter[d["flags"]] += 1
        rows.append(d)

    print(f"parsed OK : {len(rows)}")
    print(f"failed    : {len(failures)}")
    for f, e in failures:
        print(f"   FAIL {os.path.basename(f)}: {e}")

    print("\n--- block types observed across corpus ---")
    print(f"{'type':>5} {'count':>7} {'total bytes':>14}  name")
    for btype, cnt in sorted(type_counter.items()):
        print(f"{btype:>5} {cnt:>7} {type_bytes[btype]:>14,}  {BLOCK_NAMES.get(btype, '** UNDOCUMENTED **')}")

    print("\n--- topology/flag byte values in block 0 ---")
    for fl, cnt in sorted(flag_counter.items()):
        bits = format(fl, "08b")
        print(f"  0x{fl:02x} ({bits})  x{cnt}   circular={bool(fl & 1)} ds={bool(fl & 2)}")

    print("\n--- framing completeness (bytes consumed vs file size) ---")
    bad = [r for r in rows if r["consumed"] != r["size"]]
    print(f"  files fully accounted for by block framing: {len(rows) - len(bad)}/{len(rows)}")
    for r in bad[:10]:
        print(f"    {os.path.basename(r['path'])}: consumed {r['consumed']:,} of {r['size']:,}")

    print("\n--- per-file summary ---")
    print(f"{'seq_len':>10} {'top':>8} {'feat':>5} {'prim':>5} {'exp':>4} {'imp':>4}  name")
    for r in sorted(rows, key=lambda r: -(r["seq_len"] or 0)):
        print(
            f"{(r['seq_len'] or -1):>10} {str(r['topology']):>8} "
            f"{str(r['n_features']):>5} {str(r['n_primers']):>5} "
            f"{r['exp_ver']:>4} {r['imp_ver']:>4}  {os.path.basename(r['path'])[:60]}"
        )


if __name__ == "__main__":
    main(sys.argv[1:])
