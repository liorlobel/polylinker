"""Decode the structure of .dna blocks 2 and 3.

Hypothesis: block 3 = restriction-enzyme recognition-site table (an enzyme-set
cache), block 2 = precomputed cut-site position index. If both are derivable
from (sequence + enzyme set), a writer can regenerate them instead of
round-tripping them, which is the crux of write support.
"""
import struct
import sys
import os
import re
from collections import Counter

IUPAC = set("ACGTRYSWKMBDHVN")


def get_block(path, want):
    with open(path, "rb") as fh:
        while True:
            hdr = fh.read(5)
            if len(hdr) < 5:
                return None
            btype, blen = struct.unpack(">BI", hdr)
            payload = fh.read(blen)
            if btype == want:
                return payload


def seq_len(path):
    p = get_block(path, 0)
    return len(p) - 1 if p else None


def decode_b3(payload, label):
    print(f"\n=== BLOCK 3 :: {label} ===")
    print(f"  total bytes: {len(payload):,}")
    ver = payload[0]
    count = struct.unpack(">I", payload[1:5])[0]
    print(f"  byte0 = {ver}   next uint32be = {count:,}")

    body = payload[5:]
    # The head is ASCII, comma-separated. Find how far pure-ASCII runs.
    ascii_end = 0
    for i, b in enumerate(body):
        if not (32 <= b < 127):
            ascii_end = i
            break
    else:
        ascii_end = len(body)
    head = body[:ascii_end].decode("ascii", "replace")
    print(f"  leading ASCII run: {ascii_end:,} bytes ({ascii_end / max(len(body),1):.1%} of body)")

    tokens = head.split(",")
    print(f"  comma-separated tokens: {len(tokens):,}")
    print(f"  first 12 tokens: {tokens[:12]}")
    print(f"  last 3 tokens of ASCII run: {tokens[-3:]}")

    # How many tokens look like IUPAC recognition sequences?
    iupac_tok = [t for t in tokens if t and set(t) <= IUPAC]
    print(f"  tokens that are pure IUPAC: {len(iupac_tok):,} / {len(tokens):,} "
          f"({len(iupac_tok)/max(len(tokens),1):.1%})")
    lens = Counter(len(t) for t in iupac_tok)
    print(f"  recognition-site length distribution: {dict(sorted(lens.items()))}")
    print(f"  does token count match the uint32 header? {len(tokens)} vs {count} -> "
          f"{'MATCH' if len(tokens) == count else 'no'}")
    return len(tokens), count


def decode_b2(payload, slen, label):
    print(f"\n=== BLOCK 2 :: {label} ===")
    print(f"  total bytes: {len(payload):,}   seq_len: {slen:,}")
    ver = payload[0]
    print(f"  byte0 = {ver}")
    body = payload[1:]
    print(f"  body bytes: {len(body):,}  ratio to seq: {len(body)/slen:.4f}")

    # Try reading as uint32be stream
    n32 = len(body) // 4
    vals = struct.unpack(f">{min(n32, 40)}I", body[: min(n32, 40) * 4])
    print(f"  first 24 uint32be: {list(vals[:24])}")
    allv = struct.unpack(f">{n32}I", body[: n32 * 4])
    inrange = sum(1 for v in allv if v <= slen)
    print(f"  uint32 values within [0, seq_len]: {inrange:,}/{n32:,} ({inrange/max(n32,1):.1%})")
    mono = all(allv[i] <= allv[i + 1] for i in range(min(len(allv), 2000) - 1))
    print(f"  first 2000 monotonically non-decreasing? {mono}")

    # Try uint16be
    n16 = len(body) // 2
    v16 = struct.unpack(f">{min(n16,40)}H", body[: min(n16,40) * 2])
    print(f"  first 24 uint16be: {list(v16[:24])}")
    print(f"  hypothesis check: bytes/base = {len(body)/slen:.4f} "
          f"(2.0 would be one uint16 per base; ~2.13 suggests uint16/base + extra table)")


def main(paths):
    for p in paths:
        if not os.path.exists(p):
            print(f"missing: {p}")
            continue
        label = os.path.basename(p)[:40]
        slen = seq_len(p)
        b3 = get_block(p, 3)
        if b3:
            decode_b3(b3, label)
        b2 = get_block(p, 2)
        if b2 and slen:
            decode_b2(b2, slen, label)


if __name__ == "__main__":
    main(sys.argv[1:])
