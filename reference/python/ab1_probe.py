"""Probe the ABIF (.ab1) Sanger chromatogram format on a real corpus.

Sanger-trace review is one of the highest-frequency SnapGene workflows, so
reading .ab1 is the second interop question after .dna. ABIF is an openly
documented Applied Biosystems format: a header plus a directory of tagged
entries, each with a type code, count and either an inline value or an offset.

This reads it directly -- no vendor library -- and cross-checks the basecalls
and quality scores against Biopython's reader.
"""
import struct
import sys
import glob
import os
from collections import Counter

# ABIF element type codes we care about
T_CHAR, T_WORD, T_SHORT, T_LONG, T_FLOAT, T_PSTRING, T_CSTRING = 2, 3, 4, 5, 7, 18, 19


def read_abif(path):
    with open(path, "rb") as fh:
        data = fh.read()
    if data[:4] != b"ABIF":
        raise ValueError("missing ABIF magic")
    version = struct.unpack_from(">H", data, 4)[0]

    # A directory entry is 28 bytes:
    #   name 4s | number i4 | elementtype i2 | elementsize i2
    #   numelements i4 | datasize i4 | dataoffset i4 | datahandle i4
    # When datasize <= 4 the value is stored inline *in* the dataoffset field.
    ENTRY = ">4sIHHIII"          # through dataoffset = 24 bytes
    OFF_DATAOFFSET = 20

    # The header's own directory entry sits at offset 6.
    _n, _num, _t, _s, ecount, _dsz, dir_offset = struct.unpack_from(ENTRY, data, 6)

    entries = {}
    for i in range(ecount):
        off = dir_offset + i * 28
        if off + 28 > len(data):
            break
        tag, tnum, ttype, tsize, tcount, tdatasize, tdataoff = \
            struct.unpack_from(ENTRY, data, off)
        if tdatasize <= 4:
            raw = data[off + OFF_DATAOFFSET: off + OFF_DATAOFFSET + tdatasize]
        else:
            raw = data[tdataoff: tdataoff + tdatasize]
        entries[(tag.decode("ascii", "replace"), tnum)] = (ttype, tcount, raw)
    return version, entries


def val(entries, tag, num=1):
    e = entries.get((tag, num))
    if not e:
        return None
    ttype, count, raw = e
    if ttype == T_CHAR:
        return raw.decode("ascii", "replace")
    if ttype == T_PSTRING:
        return raw[1:1 + raw[0]].decode("ascii", "replace") if raw else ""
    if ttype == T_CSTRING:
        return raw.rstrip(b"\x00").decode("ascii", "replace")
    if ttype == T_SHORT:
        return struct.unpack(f">{count}h", raw)
    if ttype == T_WORD:
        return struct.unpack(f">{count}H", raw)
    if ttype == T_LONG:
        return struct.unpack(f">{count}i", raw)
    return raw


def probe(path):
    version, entries = read_abif(path)
    seq = val(entries, "PBAS") or ""
    qual = val(entries, "PCON")
    if isinstance(qual, str):
        qual = tuple(ord(c) for c in qual)
    elif isinstance(qual, (bytes, bytearray)):
        qual = tuple(qual)
    traces = {c: val(entries, t) for c, t in zip("GATC", ["DATA"] * 4)}
    # the four processed trace channels are DATA 9..12
    chans = [val(entries, "DATA", n) for n in (9, 10, 11, 12)]
    order = val(entries, "FWO_") or ""
    return {
        "version": version,
        "n_entries": len(entries),
        "seq": seq,
        "qual": qual,
        "sample": val(entries, "SMPL") or "",
        "machine": val(entries, "MCHN") or "",
        "dye": val(entries, "DySN") or "",
        "run": val(entries, "RUND") or "",
        "order": order,
        "chan_len": [len(c) if c else 0 for c in chans],
        "peaks": val(entries, "PLOC"),
    }


def main(patterns, limit=10):
    files = []
    for p in patterns:
        files.extend(glob.glob(p, recursive=True))
    files = sorted(set(files))
    print(f"corpus: {len(files)} .ab1 files\n")

    okc = 0
    fails = []
    qsum = []
    lens = []
    machines = Counter()

    for f in files:
        try:
            d = probe(f)
        except Exception as e:
            fails.append((os.path.basename(f), repr(e)))
            continue
        okc += 1
        lens.append(len(d["seq"]))
        machines[d["machine"].strip()] += 1
        if d["qual"]:
            good = sum(1 for q in d["qual"] if q >= 20)
            qsum.append(good / max(len(d["qual"]), 1))

    print(f"parsed OK : {okc}/{len(files)}")
    print(f"failed    : {len(fails)}")
    for n, e in fails[:5]:
        print(f"   {n}: {e}")

    if lens:
        lens.sort()
        print(f"\nread length: min {lens[0]}, median {lens[len(lens)//2]}, max {lens[-1]}")
    if qsum:
        print(f"mean fraction of bases with Q>=20: {sum(qsum)/len(qsum):.1%}")
    print(f"instruments: {dict(machines.most_common(5))}")

    print("\n--- detail for a few files ---")
    for f in files[:limit//2]:
        try:
            d = probe(f)
        except Exception:
            continue
        print(f"  {os.path.basename(f)[:46]}")
        print(f"    ABIF v{d['version']}  entries={d['n_entries']}  channel order={d['order']}")
        print(f"    sample={d['sample']!r} machine={d['machine']!r} dye={d['dye']!r}")
        print(f"    bases={len(d['seq'])}  trace points/channel={d['chan_len']}  "
              f"peak locations={len(d['peaks']) if d['peaks'] else 0}")
        print(f"    5' 60 bases: {d['seq'][:60]}")

    # cross-check against Biopython
    print("\n--- cross-check vs Biopython SeqIO 'abi' ---")
    try:
        from Bio import SeqIO
    except ImportError:
        print("  biopython unavailable")
        return
    mism = 0
    checked = 0
    for f in files[:60]:
        try:
            rec = SeqIO.read(f, "abi")
            mine = probe(f)["seq"]
            if str(rec.seq) != mine:
                mism += 1
                if mism <= 3:
                    print(f"  MISMATCH {os.path.basename(f)[:40]}: "
                          f"len {len(rec.seq)} vs {len(mine)}")
            checked += 1
        except Exception as e:
            print(f"  ERROR {os.path.basename(f)[:40]}: {e!r}")
    print(f"  basecall strings identical for {checked - mism}/{checked} files")


if __name__ == "__main__":
    main(sys.argv[1:])
