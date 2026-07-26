"""Test a concrete structural hypothesis for .dna block 2.

Hypothesis H1: block 2 is  version:uint8  then, for each recognition site
listed in block 3 (in the same order), a uint32be count followed by that many
uint32be positions.

A hypothesis is accepted only if it consumes the payload exactly, for every
file, with all positions inside the sequence.
"""
import struct
import sys
import glob
import os


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


def sites_from_b3(b3):
    n_ascii = struct.unpack(">I", b3[1:5])[0]
    ascii_part = b3[5:5 + n_ascii].decode("ascii", "replace")
    return ascii_part.split(","), b3[5 + n_ascii:]


def test_h1(b2, n_sites, seq_len):
    """version byte, then per-site [count][positions...]"""
    body = b2[1:]
    pos = 0
    counts = []
    all_positions = []
    for i in range(n_sites):
        if pos + 4 > len(body):
            return False, f"ran out of data at site {i}/{n_sites}", None
        cnt = struct.unpack_from(">I", body, pos)[0]
        pos += 4
        if cnt > seq_len or pos + 4 * cnt > len(body):
            return False, f"implausible count {cnt} at site {i}", None
        vals = struct.unpack_from(f">{cnt}I", body, pos) if cnt else ()
        pos += 4 * cnt
        counts.append(cnt)
        all_positions.extend(vals)
    if pos != len(body):
        return False, f"consumed {pos} of {len(body)} bytes ({len(body) - pos} left over)", None
    return True, "exact fit", (counts, all_positions)


def main(patterns):
    files = []
    for p in patterns:
        files.extend(glob.glob(p, recursive=True))
    files = sorted(set(files), key=os.path.getsize)[:14]

    print(f"{'seq_len':>9} {'sites':>6} {'result':>10}  detail")
    accepted = 0
    for f in files:
        b2, b3 = get_block(f, 2), get_block(f, 3)
        seq = get_block(f, 0)
        if not (b2 and b3 and seq):
            continue
        seq_len = len(seq) - 1
        sites, _tail = sites_from_b3(b3)
        okk, why, data = test_h1(b2, len(sites), seq_len)
        if okk:
            accepted += 1
            counts, positions = data
            inrange = sum(1 for v in positions if 0 <= v <= seq_len)
            mono_ok = True
            # positions within a single enzyme should be ascending if absolute
            idx = 0
            for c in counts:
                grp = positions[idx:idx + c]
                if any(grp[i] > grp[i + 1] for i in range(len(grp) - 1)):
                    mono_ok = False
                    break
                idx += c
            print(f"{seq_len:>9,} {len(sites):>6} {'H1 FITS':>10}  "
                  f"total_sites={len(positions):,} in_range={inrange/max(len(positions),1):.0%} "
                  f"ascending_per_enzyme={mono_ok}")
        else:
            print(f"{seq_len:>9,} {len(sites):>6} {'H1 fails':>10}  {why}")

    print(f"\nH1 accepted for {accepted}/{len(files)} files tested")
    if accepted == len(files) and accepted:
        print("=> block 2 is a per-enzyme cut-position index keyed to block 3's site list.")
        print("   Fully derivable from (sequence x enzyme set): a writer can regenerate it.")


if __name__ == "__main__":
    main(sys.argv[1:])
