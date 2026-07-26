"""snapdna -- a clean-room reader/writer for the SnapGene .dna container format.

Derived entirely from black-box observation of the byte layout of .dna files
(container framing, block boundaries, and the plainly-readable XML payloads).
No vendor code was disassembled and no vendor database is reproduced here.

Container layout
----------------
A .dna file is a flat, ordered stream of blocks:

    block := type:uint8  length:uint32be  payload:length bytes

The first block must be type 9 (header) whose payload is:

    magic "SnapGene" (8 bytes)
    fileType   uint16be   1 = DNA, 2 = protein, 3 = ...
    exportVer  uint16be
    importVer  uint16be

Block types observed in the wild
--------------------------------
   0  DNA sequence      1 flags byte + ASCII bases
   2  cut-site cache    DERIVED: precomputed restriction cut positions
   3  enzyme table      DERIVED: uint32be ASCII-length + comma-separated
                        IUPAC recognition sites, then packed match data
   5  primers           XML  <Primers><Primer><BindingSite>
   6  notes             XML  <Notes> UUID / dates / TransformedInto
   7  history tree      XZ-COMPRESSED XML <HistoryTree>
   8  extra properties  XML  <AdditionalSequenceProperties> end stickiness
  10  features          XML  <Features><Feature><Segment range="a-b">
  11  history node      per-node history payload
  13/14/17/28           small ancillary blocks, passed through verbatim

Flags byte of block 0 (bit 0 = LSB):
    bit0 circular   bit1 double-stranded
    bit2 Dam methylated   bit3 Dcm methylated   bit4 EcoKI methylated

Blocks 2 and 3 are caches fully derivable from (sequence x enzyme set), so a
writer may regenerate or omit them rather than preserving them.
"""

from __future__ import annotations

import struct
import lzma
from dataclasses import dataclass, field
from typing import Iterator
import xml.etree.ElementTree as ET

MAGIC = b"SnapGene"

BLOCK_HEADER = 9
BLOCK_SEQUENCE = 0
BLOCK_CUTSITE_CACHE = 2
BLOCK_ENZYME_TABLE = 3
BLOCK_PRIMERS = 5
BLOCK_NOTES = 6
BLOCK_HISTORY_TREE = 7
BLOCK_EXTRA_PROPS = 8
BLOCK_FEATURES = 10
BLOCK_HISTORY_NODE = 11

# Blocks that are pure caches of (sequence x enzyme set) and can be regenerated.
DERIVED_BLOCKS = frozenset({BLOCK_CUTSITE_CACHE, BLOCK_ENZYME_TABLE})

FILE_TYPE_DNA = 1

FLAG_CIRCULAR = 0x01
FLAG_DOUBLE_STRANDED = 0x02
FLAG_DAM_METHYLATED = 0x04
FLAG_DCM_METHYLATED = 0x08
FLAG_ECOKI_METHYLATED = 0x10


class SnapDnaError(Exception):
    pass


@dataclass
class Block:
    """A raw container block, kept verbatim so unknown types survive a rewrite."""

    type: int
    payload: bytes

    @property
    def size_on_disk(self) -> int:
        return 5 + len(self.payload)


@dataclass
class Segment:
    start: int          # 1-based inclusive, as stored
    end: int            # 1-based inclusive
    color: str | None = None
    translated: bool = False
    kind: str = "standard"


@dataclass
class Feature:
    name: str
    type: str = "misc_feature"
    directionality: int | None = None   # 1 = forward, 2 = reverse, 3 = both
    segments: list[Segment] = field(default_factory=list)
    qualifiers: dict[str, str] = field(default_factory=dict)

    @property
    def start(self) -> int:
        return min(s.start for s in self.segments) if self.segments else 0

    @property
    def end(self) -> int:
        return max(s.end for s in self.segments) if self.segments else 0


@dataclass
class Primer:
    name: str
    sequence: str
    description: str = ""
    binding_sites: list[tuple[int, int, int, float | None]] = field(default_factory=list)
    # (start, end, strand, melting_temperature)


@dataclass
class SnapDnaDocument:
    sequence: str = ""
    flags: int = FLAG_DOUBLE_STRANDED
    file_type: int = FILE_TYPE_DNA
    export_version: int = 15
    import_version: int = 19
    features: list[Feature] = field(default_factory=list)
    primers: list[Primer] = field(default_factory=list)
    notes: dict[str, str] = field(default_factory=dict)
    history_xml: str | None = None
    blocks: list[Block] = field(default_factory=list)   # verbatim, for round-tripping

    # -- topology / methylation -------------------------------------------
    @property
    def is_circular(self) -> bool:
        return bool(self.flags & FLAG_CIRCULAR)

    @property
    def is_double_stranded(self) -> bool:
        return bool(self.flags & FLAG_DOUBLE_STRANDED)

    @property
    def length(self) -> int:
        return len(self.sequence)

    def __repr__(self) -> str:
        return (
            f"<SnapDnaDocument {self.length:,} bp "
            f"{'circular' if self.is_circular else 'linear'} "
            f"features={len(self.features)} primers={len(self.primers)}>"
        )


# --------------------------------------------------------------------------
# container level
# --------------------------------------------------------------------------

def iter_blocks(data: bytes) -> Iterator[Block]:
    """Walk the block stream, validating framing as we go."""
    n = len(data)
    pos = 0
    first = True
    while pos < n:
        if pos + 5 > n:
            raise SnapDnaError(f"truncated block header at offset {pos}")
        btype, blen = struct.unpack_from(">BI", data, pos)
        pos += 5
        if pos + blen > n:
            raise SnapDnaError(
                f"block type {btype} at {pos - 5} claims {blen} bytes, "
                f"only {n - pos} remain"
            )
        payload = data[pos:pos + blen]
        pos += blen
        if first:
            if btype != BLOCK_HEADER:
                raise SnapDnaError(f"first block is type {btype}, expected 9")
            if not payload.startswith(MAGIC):
                raise SnapDnaError("missing 'SnapGene' magic in header block")
            first = False
        yield Block(btype, payload)
    if first:
        raise SnapDnaError("empty file")


def pack_blocks(blocks: list[Block]) -> bytes:
    out = bytearray()
    for b in blocks:
        out += struct.pack(">BI", b.type, len(b.payload))
        out += b.payload
    return bytes(out)


# --------------------------------------------------------------------------
# payload parsing
# --------------------------------------------------------------------------

def _parse_features(payload: bytes) -> list[Feature]:
    root = ET.fromstring(payload.decode("utf-8", "replace"))
    feats = []
    for fel in root.findall("Feature"):
        segs = []
        for sel in fel.findall("Segment"):
            rng = sel.get("range", "")
            if "-" not in rng:
                continue
            a, _, b = rng.partition("-")
            try:
                segs.append(Segment(
                    start=int(a), end=int(b),
                    color=sel.get("color"),
                    translated=sel.get("translated") == "1",
                    kind=sel.get("type", "standard"),
                ))
            except ValueError:
                continue
        quals = {}
        for q in fel.findall("Q"):
            qname = q.get("name", "")
            v = q.find("V")
            if v is None:
                continue
            quals[qname] = v.get("text") or v.get("int") or v.get("predef") or ""
        d = fel.get("directionality")
        feats.append(Feature(
            name=fel.get("name", ""),
            type=fel.get("type", "misc_feature"),
            directionality=int(d) if d and d.isdigit() else None,
            segments=segs,
            qualifiers=quals,
        ))
    return feats


def _parse_primers(payload: bytes) -> list[Primer]:
    root = ET.fromstring(payload.decode("utf-8", "replace"))
    prims = []
    for pel in root.findall("Primer"):
        sites = []
        for bs in pel.findall("BindingSite"):
            if bs.get("simplified") == "1":
                continue  # duplicate of the detailed entry
            loc = bs.get("location", "")
            if "-" not in loc:
                continue
            a, _, b = loc.partition("-")
            tm = bs.get("meltingTemperature")
            try:
                sites.append((
                    int(a), int(b),
                    int(bs.get("boundStrand", "0")),
                    float(tm) if tm else None,
                ))
            except ValueError:
                continue
        prims.append(Primer(
            name=pel.get("name", ""),
            sequence=pel.get("sequence", ""),
            description=pel.get("description", ""),
            binding_sites=sites,
        ))
    return prims


def _parse_notes(payload: bytes) -> dict[str, str]:
    root = ET.fromstring(payload.decode("utf-8", "replace"))
    return {child.tag: (child.text or "") for child in root}


def _parse_history(payload: bytes) -> str | None:
    """Block 7 is xz-compressed XML in modern files, plain XML in older ones."""
    if payload[:6] == b"\xfd7zXZ\x00":
        try:
            return lzma.decompress(payload).decode("utf-8", "replace")
        except lzma.LZMAError:
            return None
    return payload.decode("utf-8", "replace")


# --------------------------------------------------------------------------
# public API
# --------------------------------------------------------------------------

def loads(data: bytes) -> SnapDnaDocument:
    doc = SnapDnaDocument()
    doc.blocks = list(iter_blocks(data))

    for b in doc.blocks:
        if b.type == BLOCK_HEADER:
            doc.file_type, doc.export_version, doc.import_version = struct.unpack(
                ">HHH", b.payload[8:14]
            )
        elif b.type == BLOCK_SEQUENCE:
            if b.payload:
                doc.flags = b.payload[0]
                doc.sequence = b.payload[1:].decode("ascii", "replace")
        elif b.type == BLOCK_FEATURES:
            doc.features = _parse_features(b.payload)
        elif b.type == BLOCK_PRIMERS:
            doc.primers = _parse_primers(b.payload)
        elif b.type == BLOCK_NOTES:
            doc.notes = _parse_notes(b.payload)
        elif b.type == BLOCK_HISTORY_TREE:
            doc.history_xml = _parse_history(b.payload)
    return doc


def load(path: str) -> SnapDnaDocument:
    with open(path, "rb") as fh:
        return loads(fh.read())


def dumps(doc: SnapDnaDocument, *, drop_derived: bool = False) -> bytes:
    """Serialise back to .dna bytes.

    With the original blocks retained this is byte-exact. `drop_derived` omits
    the regenerable enzyme caches (blocks 2 and 3), which is what a real
    writer that has not yet implemented cut-site computation should do.
    """
    if not doc.blocks:
        raise SnapDnaError("document has no blocks; synthesising from scratch "
                           "is not implemented yet")
    blocks = doc.blocks
    if drop_derived:
        blocks = [b for b in blocks if b.type not in DERIVED_BLOCKS]
    return pack_blocks(blocks)


def dump(doc: SnapDnaDocument, path: str, *, drop_derived: bool = False) -> None:
    with open(path, "wb") as fh:
        fh.write(dumps(doc, drop_derived=drop_derived))
