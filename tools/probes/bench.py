import time, sys, os
sys.path.insert(0, r"<SCRATCH>")
import snapdna
from validate_digest import our_digest

targets = [
 r"<CORPUS>\pACYC184-Ppho-fab2-6his.dna",
 r"<CORPUS>\Fusobacterium_nucleatum_subsp_nucleatum_ATCC_23726.dna",
 r"<CORPUS>\NC_000913.3 Escherichia coli str. K-12 substr. MG1655, complete genome.dna",
]
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
