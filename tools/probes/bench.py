"""A rough timing probe over real `.dna` files.

Takes the files to time as arguments, or from `PL_CORPUS` if none are given:

    python tools/probes/bench.py a.dna b.dna
    PL_CORPUS="/path/to/plasmids" python tools/probes/bench.py

The paths used to be hard-coded to one machine's lab drive, which made the
script useless to anybody else and published the directory layout of an
unpublished project into a public repository. Neither is a good trade for three
saved keystrokes.
"""
import glob
import os
import sys
import time

_ref = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..",
                    "reference", "python")
sys.path.insert(0, _ref)
sys.path.insert(0, os.path.join(_ref, "tests"))
import snapdna
from validate_digest import our_digest

targets = [a for a in sys.argv[1:] if os.path.exists(a)]
if not targets:
    root = os.environ.get("PL_CORPUS")
    if not root:
        print("give .dna files as arguments, or set PL_CORPUS to a directory")
        raise SystemExit(1)
    targets = sorted(glob.glob(os.path.join(root, "**", "*.dna"), recursive=True))[:3]
if not targets:
    print("no .dna files found")
    raise SystemExit(1)

print(f"{'file size':>12} {'bp':>10} {'parse ms':>9} {'digest ms':>10} {'sites':>8}  name")
for t in targets:
    if not os.path.exists(t): continue
    size = os.path.getsize(t)
    t0 = time.perf_counter(); doc = snapdna.load(t); t1 = time.perf_counter()
    t2 = time.perf_counter(); d = our_digest(doc.sequence, doc.is_circular); t3 = time.perf_counter()
    n = sum(len(v) for v in d.values())
    print(f"{size:>12,} {doc.length:>10,} {(t1-t0)*1000:>9.0f} {(t3-t2)*1000:>10.0f} {n:>8,}  {os.path.basename(t)[:40]}")
print("\n(digest = 50 enzymes, pure-Python str.find; a Rust/WASM or Aho-Corasick")
print(" implementation over the full ~470-enzyme set is the real target)")
