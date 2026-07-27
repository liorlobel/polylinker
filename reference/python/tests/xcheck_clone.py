"""Differential test: pl-clone against pydna.

docs/PLAN.md ADR-5 makes pydna the oracle for cloning and pl-clone the runtime.
That only works if the two agree, so this compares them fragment by fragment,
watson and crick and overhang, over sequences neither of us picked.

Sticky ends are where a port goes subtly wrong: a fragment can have the right
length and the wrong overhang, and it will look completely fine until someone
tries to ligate it.

Usage:
    python xcheck_clone.py <path-to-pl.exe>
"""

import json
import os
import random
import subprocess
import sys

from Bio import Restriction
from pydna.dseq import Dseq

# Enzymes covering all three end shapes: 5' overhang, 3' overhang, blunt.
ENZYMES = [
    "EcoRI", "BamHI", "HindIII", "XhoI", "SalI",      # 5' overhang
    "PstI", "KpnI", "SacI", "SphI", "NsiI",            # 3' overhang
    "EcoRV", "SmaI", "DraI", "PvuII", "ScaI",          # blunt
    "NotI", "AscI",                                     # 8-cutters
]

rng = random.Random(20260727)


def rand_seq(n):
    return "".join(rng.choice("ACGT") for _ in range(n))


def build_cases():
    """Sequences with a controlled number of sites for a given enzyme."""
    cases = []
    for name in ENZYMES:
        site = str(getattr(Restriction, name).site)
        for n_sites in (1, 2, 3):
            for circular in (True, False):
                parts = [rand_seq(rng.randint(20, 60))]
                for _ in range(n_sites):
                    parts.append(site)
                    parts.append(rand_seq(rng.randint(20, 60)))
                seq = "".join(parts)
                cases.append({
                    "id": f"{name}-{n_sites}site-{'circ' if circular else 'lin'}",
                    "enzyme": name,
                    "circular": circular,
                    "seq": seq,
                })
        # A site straddling the origin, which only exists when circular.
        for split in (1, len(site) // 2, len(site) - 1):
            seq = site[split:] + rand_seq(80) + site[:split]
            cases.append({
                "id": f"{name}-origin{split}-circ",
                "enzyme": name,
                "circular": True,
                "seq": seq,
            })
    return cases


def pydna_cut(case):
    enz = getattr(Restriction, case["enzyme"])
    d = Dseq(case["seq"], circular=case["circular"])
    frags = d.cut(enz)
    return [
        {"watson": str(f.watson).upper(), "crick": str(f.crick).upper(), "ovhg": int(f.ovhg)}
        for f in frags
    ]


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    # Absolute: see the note in xcheck_seguid.py.
    pl = os.path.abspath(sys.argv[1])
    cases = build_cases()

    payload = "\n".join(
        f"{c['id']}\t{c['enzyme']}\t{'circular' if c['circular'] else 'linear'}\t{c['seq']}"
        for c in cases
    )
    r = subprocess.run([pl, "cut-adapter"], input=payload + "\n",
                       capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        sys.exit(f"pl cut-adapter failed:\n{r.stderr[:3000]}")

    ours = {}
    for line in r.stdout.splitlines():
        if not line.strip():
            continue
        obj = json.loads(line)
        ours[obj["id"]] = obj["fragments"]

    agree = 0
    problems = []
    total_frags = 0
    for c in cases:
        want = pydna_cut(c)
        got = ours.get(c["id"])
        if got is None:
            problems.append((c["id"], "no answer", "", ""))
            continue
        total_frags += len(want)
        if len(want) != len(got):
            problems.append((c["id"], "fragment count", len(want), len(got)))
            continue
        # pydna's fragment order for a circle starts at a different place than
        # ours may; compare as multisets keyed on the full triple.
        key = lambda f: (f["watson"], f["crick"], f["ovhg"])
        if sorted(map(key, want)) != sorted(map(key, got)):
            for i, (a, b) in enumerate(zip(sorted(map(key, want)), sorted(map(key, got)))):
                if a != b:
                    problems.append((c["id"], f"fragment {i}", a, b))
                    break
            continue
        agree += 1

    print("=" * 72)
    print(f"cases            : {len(cases)}")
    print(f"agree with pydna : {agree}")
    print(f"disagree         : {len(problems)}")
    print(f"fragments checked: {total_frags}")
    for cid, what, want, got in problems[:15]:
        print(f"\n  {cid}  [{what}]")
        print(f"    pydna : {want}")
        print(f"    ours  : {got}")
    print("\nwatson, crick and overhang all compared: a fragment can be the right")
    print("length and the wrong shape, and only the overhang shows it")

    pcr_problems = check_pcr(pl)
    return 0 if not problems and not pcr_problems else 1


def build_pcr_cases():
    """Templates and primers, including the awkward ones.

    5' tails are the reason anyone designs a primer by hand -- they are how a
    restriction site or a homology arm gets onto a product that never had one --
    so they have to work, and they are exactly where an off-by-one lands.
    """
    cases = []
    for i in range(12):
        tmpl = rand_seq(rng.randint(300, 900))
        start = rng.randint(0, 60)
        length = rng.randint(120, 250)
        fwd = tmpl[start:start + 20]
        rev = str(Dseq(tmpl[start + length - 20:start + length]).reverse_complement().watson)
        cases.append({"id": f"pcr-plain-{i}", "tmpl": tmpl, "fwd": fwd, "rev": rev,
                      "circular": False})

    # 5' tails that match nothing in the template.
    for i, tail_f, tail_r in [(0, "GAATTC", ""), (1, "", "AAGCTT"),
                              (2, "GGATCCTTAA", "CTGCAGGGCC"),
                              (3, "ACGTACGTACGTACGT", "TTTTTTTTTT")]:
        tmpl = rand_seq(500)
        start, length = 50, 200
        fwd = tail_f + tmpl[start:start + 20]
        rev = tail_r + str(Dseq(tmpl[start + length - 20:start + length]).reverse_complement().watson)
        cases.append({"id": f"pcr-tail-{i}", "tmpl": tmpl, "fwd": fwd, "rev": rev,
                      "circular": False})

    # Circular templates. There was no circular PCR coverage anywhere in this
    # project -- not in the unit tests, not in the bench, not here -- and three
    # of the four known pcr() defects only appear on a circle. pydna decides
    # what these should produce; we do not.
    for i in range(4):
        n = rng.randint(300, 600)
        tmpl = rand_seq(n)
        start = rng.randint(20, n - 200)
        length = rng.randint(80, 150)
        fwd = tmpl[start:start + 20]
        rev = str(Dseq(tmpl[start + length - 20:start + length]).reverse_complement().watson)
        cases.append({"id": f"pcr-circ-{i}", "tmpl": tmpl, "fwd": fwd, "rev": rev,
                      "circular": True})

    # An amplicon that crosses the origin. Entirely routine on a plasmid -- the
    # origin is an arbitrary numbering choice -- and it was returning
    # Err(Inverted), i.e. "your primers face away from each other".
    for i, tail in enumerate(["", "GAATTC"]):
        n = 400
        tmpl = rand_seq(n)
        fwd = tail + tmpl[n - 30:n - 10]        # anneals near the end
        rev = str(Dseq(tmpl[15:35]).reverse_complement().watson)  # ...and wraps past 1
        cases.append({"id": f"pcr-origin-{i}", "tmpl": tmpl, "fwd": fwd, "rev": rev,
                      "circular": True})

    # Primer footprints that overlap, the shape produced by partially
    # overlapping site-directed-mutagenesis primers. The product is the span,
    # not the concatenation.
    for i, (overlap, circular) in enumerate([(10, False), (10, True), (18, False)]):
        tmpl = rand_seq(400)
        start = 100
        fwd = tmpl[start:start + 20]
        rev_start = start + 20 - overlap
        rev = str(Dseq(tmpl[rev_start:rev_start + 20]).reverse_complement().watson)
        cases.append({"id": f"pcr-overlap-{i}", "tmpl": tmpl, "fwd": fwd, "rev": rev,
                      "circular": circular})

    # Templates carrying a repeated motif, where a naive search binds the wrong
    # copy.
    for i in range(4):
        motif = rand_seq(25)
        tmpl = rand_seq(150) + motif + rand_seq(200) + motif + rand_seq(150)
        start = 150
        fwd = tmpl[start:start + 20]
        rev = str(Dseq(tmpl[start + 300:start + 320]).reverse_complement().watson)
        cases.append({"id": f"pcr-repeat-{i}", "tmpl": tmpl, "fwd": fwd, "rev": rev,
                      "circular": False})
    return cases


def check_pcr(pl):
    from pydna.amplify import pcr as pydna_pcr
    from pydna.dseqrecord import Dseqrecord

    cases = build_pcr_cases()
    lines = []
    expected = {}
    # Cases pydna refuses, kept rather than skipped so the refusal itself can be
    # compared. See the check further down.
    declined = {}
    for c in cases:
        try:
            prod = pydna_pcr(c["fwd"], c["rev"], Dseqrecord(c["tmpl"], circular=c["circular"]))
            expected[c["id"]] = str(prod.seq).upper()
        except Exception as e:
            declined[c["id"]] = str(e).splitlines()[0]
            topo = "circular" if c["circular"] else "linear"
            lines.append(f"{c['id']}\tpcr\t{topo}\t{c['tmpl']}"
                         f"\tforward_primer={c['fwd']}\treverse_primer={c['rev']}")
            continue
        topo = "circular" if c["circular"] else "linear"
        lines.append(f"{c['id']}\tpcr\t{topo}\t{c['tmpl']}"
                     f"\tforward_primer={c['fwd']}\treverse_primer={c['rev']}")

    if not lines:
        return []
    r = subprocess.run([pl, "bench-adapter"], input="\n".join(lines) + "\n",
                       capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        print(f"pl bench-adapter failed:\n{r.stderr[:2000]}")
        return [("adapter", "failed", "", "")]

    ours = {}
    for line in r.stdout.splitlines():
        parts = line.split("\t")
        if len(parts) > 1:
            ours[parts[0]] = dict(p.split("=", 1) for p in parts[1:] if "=" in p)

    problems = []
    agree = 0
    from seguid import ldseguid

    def rc(s):
        return s.translate(str.maketrans("ACGT", "TGCA"))[::-1]

    for cid, want_seq in expected.items():
        got = ours.get(cid)
        if not got or "ldseguid" not in got:
            problems.append((cid, "no product", want_seq[:40], got))
            continue
        want_ck = ldseguid(want_seq, rc(want_seq))
        if got["ldseguid"] != want_ck or int(got["product_length"]) != len(want_seq):
            problems.append((cid, "product differs",
                             f"{len(want_seq)} bp {want_ck}",
                             f"{got.get('product_length')} bp {got['ldseguid']}"))
        else:
            agree += 1

    # ...and every case pydna refused, we must refuse too.
    #
    # Refusals used to be dropped on the floor, so "agrees with pydna" meant
    # only "agrees wherever pydna produced something" — a tool that returned a
    # confident product for every impossible reaction would still have scored
    # 100%. Refusing correctly is half the behaviour.
    for cid, why in declined.items():
        got = ours.get(cid)
        if got is None:
            problems.append((cid, "no row at all", f"pydna declined: {why}", None))
        elif "error" not in got:
            problems.append((
                cid,
                "we returned a product pydna refuses",
                f"pydna declined: {why}",
                f"{got.get('product_length')} bp",
            ))
        else:
            agree += 1

    print()
    print("=" * 72)
    print(f"pcr cases        : {len(expected)} with a product, "
          f"{len(declined)} pydna refuses")
    print(f"agree with pydna : {agree}")
    print(f"disagree         : {len(problems)}")
    for cid, what, want, got in problems[:10]:
        print(f"\n  {cid}  [{what}]")
        print(f"    pydna : {want}")
        print(f"    ours  : {got}")
    print("\nproducts compared by ldseguid, so a product that is right except for")
    print("which strand it was reported on still counts as right")
    return problems


if __name__ == "__main__":
    sys.exit(main())
