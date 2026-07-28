"""Cross-validate the genetic codes and the ORF finder against Biopython.

    python xcheck_translate.py target/release/pl.exe

Four checks, deliberately of different kinds, because the first three tables
this project shipped were transcribed by hand and a mistyped amino acid in a
table nobody uses often is invisible until the day somebody uses it.

  A. **The tables themselves.** All 27 NCBI codes, amino acids and initiation
     codons, against `Bio.Data.CodonTable`. This is the direct comparison and
     the reason the other 24 could be generated rather than typed.

  B. **Every reported ORF, re-translated from its own coordinates.** Take what
     `pl orfs` says — start, end, strand, wrap — read those bases out of the
     input, hand them to Biopython with the same table, and the protein must
     come back identical. This checks the coordinates and the translation at
     once: a span that is off by three, or on the wrong strand, or wrapped the
     wrong way round, produces a different protein even when it is still a
     perfectly plausible one.

  C. **The set of ORFs, enumerated a different way.** Not a transcription of
     the Rust — this project has already shipped a "cross-check" that only ever
     compared a Python copy of the algorithm against itself. Here the frames are
     translated by Biopython and the ORFs are found by splitting the *protein*
     on its stops, which has no notion of a reading frame at all. Two
     formulations that disagree mean one of them is wrong.

  D. **Rotation invariance on circular molecules.** Rotating a circle changes
     no biology, so every ORF must come back with its coordinates shifted by
     exactly the rotation. Origin handling is where circular sequence code goes
     wrong, and this catches it without needing an oracle.

Exits 1 on any disagreement and on comparing nothing.
"""
import json
import os
import random
import subprocess
import sys
import warnings

from Bio.Data import CodonTable
from Bio.Seq import Seq

warnings.simplefilter("ignore")

BASES = "TCAG"  # NCBI's codon order
rng = random.Random(20260727)


def codons():
    return [a + b + c for a in BASES for b in BASES for c in BASES]


def bio_table(i):
    return CodonTable.unambiguous_dna_by_id[i]


def bio_ids():
    return sorted(CodonTable.unambiguous_dna_by_id)


# ---------------------------------------------------------------- A. tables

def check_tables(exe):
    """Every code's amino acids and start codons, against Biopython.

    The shipped tables are dumped and compared as data. Whether the *translator*
    then honours them is check B's job, on real spans -- reading each of 64
    codons back through the ORF finder would be 3,456 subprocesses to learn
    something check B already establishes.
    """
    r = subprocess.run([exe, "orfs", "--tables", "--json"],
                       capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"pl orfs --tables: {r.stderr.strip()}")
    ours = {t["id"]: t for t in json.loads(r.stdout)["tables"]}

    bad = []
    if sorted(ours) != bio_ids():
        bad.append(("the set of codes", sorted(ours), bio_ids()))

    for i in sorted(set(ours) & set(bio_ids())):
        t = bio_table(i)
        want_aa = "".join(t.forward_table.get(c, "*") for c in codons())
        # NCBI's own convention for this line, which the shipped tables carry
        # verbatim: `M` where a codon may initiate, `*` where it terminates,
        # `-` where it does neither. Reading `*` as "not a start" is not
        # optional -- a translator that took anything other than `-` for a
        # start would begin ORFs at stop codons in every table.
        want_start = "".join(
            "M" if c in t.start_codons else "*" if c in t.stop_codons else "-"
            for c in codons())
        if ours[i]["aas"] != want_aa:
            where = [(c, g, w) for c, g, w in
                     zip(codons(), ours[i]["aas"], want_aa) if g != w]
            bad.append((f"table {i} amino acids", where[:6],
                        f"{len(where)} codon(s) differ"))
        if ours[i]["starts"] != want_start:
            where = [c for c, g, w in
                     zip(codons(), ours[i]["starts"], want_start) if g != w]
            bad.append((f"table {i} start codons", where[:6],
                        f"{len(where)} codon(s) differ"))
    return bad


# ------------------------------------------------------------------- driver

def run_orfs(exe, seq, table, circular, extra=()):
    args = [exe, "orfs", "--json", "--seq", seq, "--table", str(table)]
    if circular:
        args.append("--circular")
    args += list(extra)
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"pl orfs: {r.stderr.strip()}")
    return json.loads(r.stdout)


def span(seq, o):
    """The ORF's bases, read the way its coordinates say to."""
    n = len(seq)
    length = o["aa_len"] * 3 + (3 if o["complete"] else 0)
    s = "".join(seq[(o["start"] - 1 + j) % n] for j in range(length))
    return str(Seq(s).reverse_complement()) if o["strand"] == "-" else s


# ------------------------------------------------- B. re-translate each ORF

def check_orfs_translate(seq, table, doc):
    bad = []
    t = bio_table(table)
    for o in doc["orfs"]:
        s = span(seq, o)
        # An ORF's protein is a CDS translation, not a raw one: a ribosome
        # initiating at GTG, TTG or ATT still puts methionine there, GenBank CDS
        # records show M, and Biopython implements the same convention behind
        # `cds=True`. Comparing against a plain `translate()` asserted the raw
        # residue and would fail every alternative-start marker on the shelf —
        # tet(A) came out starting with V.
        #
        # `cds=True` also drops the terminal stop and refuses a sequence with no
        # stop at all, so the two cases are spelled out rather than papered over.
        if o["complete"]:
            want = str(Seq(s).translate(table=table, cds=True)) + "*"
        else:
            raw = str(Seq(s).translate(table=table))
            want = ("M" + raw[1:]) if raw else raw
        if want != o["protein"]:
            bad.append((o, "protein", o["protein"], want))
            continue
        if s[:3] != o["start_codon"]:
            bad.append((o, "start codon", o["start_codon"], s[:3]))
        if o["complete"] and s[-3:] not in t.stop_codons:
            bad.append((o, "does not end at a stop", s[-3:], t.stop_codons))
        body = want[:-1] if o["complete"] else want
        if "*" in body:
            bad.append((o, "stop inside the ORF", body, ""))
    return bad


# ------------------------------------------------ C. independent enumeration

def enumerate_linear(seq, table, min_aa, include_incomplete):
    """ORFs found by splitting the translated frames on their stops.

    No reading-frame bookkeeping and no codon loop: Biopython translates each
    frame, and an ORF is the first start codon in a run of non-stop residues.
    Structurally different from the Rust, which is the point of having it.
    """
    n = len(seq)
    t = bio_table(table)
    out = set()
    for strand in "+-":
        s = seq if strand == "+" else str(Seq(seq).reverse_complement())
        for f in range(3):
            usable = (n - f) // 3 * 3
            if usable <= 0:
                continue
            # Segment on the *stop codon list*, not on `*` in the translation.
            # In tables 27, 28 and 31 a stop codon also encodes a residue, so
            # Biopython renders it as that residue and a protein-string split
            # would find no stops at all in those three codes.
            marked = "".join(
                "*" if s[f + 3 * c:f + 3 * c + 3] in t.stop_codons else "."
                for c in range(usable // 3))
            prot = marked
            pos = 0
            for seg in prot.split("*"):
                complete = pos + len(seg) < len(prot)  # a '*' followed it
                if not complete and not include_incomplete:
                    pos += len(seg) + 1
                    continue
                for c in range(len(seg)):
                    dna = s[f + 3 * (pos + c):f + 3 * (pos + c) + 3]
                    if dna in t.start_codons:
                        aa_len = len(seg) - c
                        if aa_len >= min_aa:
                            frm = f + 3 * (pos + c)
                            to = frm + aa_len * 3 + (3 if complete else 0)
                            if strand == "+":
                                a, b = frm, to - 1
                            else:
                                a, b = n - to, n - 1 - frm
                            out.add((strand, a + 1, b + 1, aa_len, complete))
                        break
                pos += len(seg) + 1
    return out


def ours_set(doc):
    return {(o["strand"], o["start"], o["end"], o["aa_len"], o["complete"])
            for o in doc["orfs"]}


# ------------------------------------------------------ D. rotation on a circle

def rotated_set(doc, n, r):
    """Our ORFs with every coordinate shifted by `r` around the circle."""
    return {(o["strand"], (o["start"] - 1 + r) % n + 1, (o["end"] - 1 + r) % n + 1,
             o["aa_len"], o["complete"]) for o in doc["orfs"]}


def main(argv):
    exe = None
    if argv and os.path.isfile(argv[0]):
        exe = os.path.abspath(argv[0])
    if exe is None:
        print("usage: xcheck_translate.py <path to pl.exe>")
        return 1

    bad = []
    print("comparing 27 genetic codes against Biopython ...")
    bad += [("A", *b) for b in check_tables(exe)]

    # Random sequences, not hand-written ones: a hand-written fixture is how
    # this project keeps producing accidental palindromes and accidental start
    # codons in frames it was not thinking about.
    orfs_seen = 0
    seqs = 0
    for i in range(60):
        n = rng.randint(60, 400)
        seq = "".join(rng.choice("ACGT") for _ in range(n))
        table = rng.choice(bio_ids())
        circular = i % 2 == 0
        min_aa = rng.choice([1, 5, 10])
        doc = run_orfs(exe, seq, table, circular, ["--min-aa", str(min_aa)])
        seqs += 1
        orfs_seen += len(doc["orfs"])

        bad += [("B", f"table {table}, {n} bp", *b[1:])
                for b in check_orfs_translate(seq, table, doc)]

        if not circular:
            want = enumerate_linear(seq, table, min_aa, True)
            got = ours_set(doc)
            if got != want:
                bad.append(("C", f"table {table}, {n} bp linear",
                            sorted(got - want)[:4], sorted(want - got)[:4]))
        else:
            r = rng.randint(1, n - 1)
            rot = seq[-r:] + seq[:-r]  # rotating right by r shifts coords by +r
            d2 = run_orfs(exe, rot, table, True, ["--min-aa", str(min_aa)])
            if ours_set(d2) != rotated_set(doc, n, r):
                a, b = ours_set(d2), rotated_set(doc, n, r)
                bad.append(("D", f"table {table}, {n} bp circular, rotated {r}",
                            sorted(a - b)[:4], sorted(b - a)[:4]))

    print("=" * 74)
    print(f"genetic codes compared: {len(bio_ids())}  (all 64 codons each)")
    print(f"sequences compared    : {seqs}")
    print(f"ORFs re-translated    : {orfs_seen}")
    print(f"disagreements         : {len(bad)}")
    print()
    print("Biopython supplies the tables and translates every reported span")
    print("back from its own coordinates. Linear ORF sets are enumerated a")
    print("second way, in protein space; circular ones must survive rotation.")

    for b in bad[:8]:
        print(f"\n  [{b[0]}] {b[1]}")
        for x in b[2:]:
            print(f"      {x}")

    if seqs == 0 or orfs_seen == 0:
        print("\nFAIL: compared nothing")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} disagreement(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
