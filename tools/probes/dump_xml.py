"""Dump the XML-bearing blocks of a .dna file to define the document data model."""
import struct
import sys
import os

XML_BLOCKS = {5: "primers", 6: "notes", 7: "history-tree", 8: "additional-properties",
              10: "features", 16: "alignable-sequences", 19: "custom-colors"}


def blocks(path):
    with open(path, "rb") as fh:
        while True:
            hdr = fh.read(5)
            if len(hdr) < 5:
                return
            btype, blen = struct.unpack(">BI", hdr)
            yield btype, fh.read(blen)


def main(path, limit=3000):
    print(f"### {os.path.basename(path)}\n")
    for btype, payload in blocks(path):
        if btype in XML_BLOCKS:
            txt = payload.decode("utf-8", "replace")
            print(f"--- block {btype} ({XML_BLOCKS[btype]}) : {len(payload):,} bytes ---")
            print(txt[:limit])
            if len(txt) > limit:
                print(f"... [{len(txt) - limit:,} more chars]")
            print()


if __name__ == "__main__":
    main(sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000)
