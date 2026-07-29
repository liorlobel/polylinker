"""Cross-validate degenerate motif search against Biopython.

    python xcheck_motif.py [target/release/pl.exe]

`validate_digest.py` and `xcheck_clone.py` cover restriction sites, and **every
site in the shipped table is a non-degenerate palindrome**. So before this file
existed, no test anywhere compared a degenerate pattern, or a minus-strand hit,
against an implementation that is not ours -- and degenerate both-strand search
is the library's headline query. The oracle covered zero of its interesting
cases.

Biopython is the oracle in two independent ways:

  * `Bio.Seq.Seq.reverse_complement` for the minus strand, and
  * `Bio.Data.IUPACData.ambiguous_dna_values` compiled to a regex for the
    degeneracy, which is a different mechanism from our 4-bit masks.

Overlapping matches need a lookahead; `re.finditer` without one silently skips
the second of two overlapping sites, which would make us look wrong where we
are right. Circular molecules are searched on `seq + seq[:k-1]`, deduplicated
modulo n -- the doubling Biopython's own restriction search uses.

Exits 1 on any disagreement and on comparing nothing.
"""
import os
import re
import subprocess
import sys

from Bio.Data.IUPACData import ambiguous_dna_values  # noqa: E402
from Bio.Seq import Seq  # noqa: E402

# Patterns chosen for what they exercise, not for looking impressive.
PATTERNS = [
    "GAATTC",        # palindromic, specific -- must be reported once, not twice
    "GGATCC",
    "ATG",           # tiny, asymmetric: the minus-strand case
    "A",             # 1 bp, degenerate-free, enormous hit count
    "N",             # matches every unambiguous base and nothing else
    "GGWCC",         # palindromic *because* W is self-complementary
    "RGCY",          # palindromic across the centre, not by letter identity
    "GGTCTC",        # BsaI: asymmetric, so + and - are distinct sites
    "GAAGAC",        # BbsI
    "CCANNNNNTGG",   # XcmI-like: an interrupted palindrome
    "GCCNNNNNGGC",   # BglI-like
    "TTGACAWWWWWWTATAAT",   # sigma-70 promoter consensus, long and degenerate
    "SSSS",
    "NNNN",          # every window matches; the pathological count case
    "BDHV",          # the four three-fold codes together
    "ACGTACGTACGTACGTACGTACGT",  # long and specific: usually zero hits
    "YR",
    "WSWS",
    "GCGGCCGC",      # NotI
    "CCCGGG",        # SmaI/XmaI share a site
]


def to_regex(pattern):
    """IUPAC pattern as a regex, via Biopython's own code table.

    A different mechanism from our masks: a disagreement means one of the two
    is wrong, rather than one bug appearing twice.

    **The subject may be ambiguous too**, and the character class has to say so.
    The first version of this expanded the pattern and treated the subject
    literally, so pattern `N` compiled to `[GATC]` and did not match a subject
    `N` -- and it accused us of four false hits in `NNNNGAATTCNNNN`. We were
    right. The rule, stated once:

        subject X matches pattern P  iff  bases(X) is a non-empty subset
                                          of bases(P)

    An unknown base satisfies "any base is acceptable", because whatever it
    turns out to be is acceptable. It does not satisfy "must be A", because it
    might be C -- which is the asymmetry `pl_core::iupac::matches` documents and
    the reason an N in a plasmid can silently *lose* a site. Built here from set
    containment over `ambiguous_dna_values`, which is still a different
    mechanism from a 4-bit mask.
    """
    out = []
    for c in pattern.upper():
        want = set(ambiguous_dna_values[c])
        allowed = "".join(
            sorted(k for k, v in ambiguous_dna_values.items() if set(v) <= want)
        )
        out.append(allowed if len(allowed) == 1 else "[" + allowed + "]")
    # Lookahead, so overlapping sites are all found. Without it `AA` in `AAA`
    # yields one match, not two, and we would look wrong where we are right.
    return re.compile("(?=(" + "".join(out) + "))")


def bio_find(pattern, seq, circular):
    """Every 1-based start, on both strands, as (start, strand)."""
    n = len(seq)
    k = len(pattern)
    if k == 0 or n == 0 or k > n:
        return set()
    ext = seq + seq[: k - 1] if circular else seq

    fwd = to_regex(pattern)
    rev = to_regex(str(Seq(pattern).reverse_complement()))
    palindromic = str(Seq(pattern).reverse_complement()).upper() == pattern.upper()

    out = set()
    for m in fwd.finditer(ext):
        if m.start() < n:
            out.add((m.start() % n + 1, "both" if palindromic else "+"))
    if not palindromic:
        for m in rev.finditer(ext):
            if m.start() < n:
                out.add((m.start() % n + 1, "-"))
    return out


def ours(exe, pattern, seq, circular):
    """The Rust answer, through the CLI's motif search over `--seq`.

    Bare bases carry no topology, so this leg always states one explicitly.
    `ours_from_file` is the other leg, and it deliberately does not.
    """
    out = subprocess.run(
        [exe, "find-motif", pattern, "--seq", seq,
         "--topology", "circular" if circular else "linear", "--json"],
        capture_output=True, text=True,
    )
    if out.returncode != 0:
        raise RuntimeError(f"pl find-motif {pattern}: {out.stderr.strip()}")
    import json
    doc = json.loads(out.stdout)
    return {(h["start"], h["strand"]) for h in doc["hits"]}


def ours_from_file(exe, pattern, seq, circular, tmpdir):
    """The Rust answer over a real record, with NO --topology on the command.

    This is the leg that did not exist, and its absence is why a real defect
    shipped: `cmd_find_motif` discarded the file's declared topology and
    searched every file as linear, so an origin-crossing hit came back absent
    at exit 0. The `--seq` leg above cannot catch that, because it always
    passes `--topology` and so never exercises the branch that reads the file.
    The docstring on `ours` used to say it ran "over a temp FASTA", which is
    what made this gap look already covered.

    GenBank rather than FASTA on purpose: FASTA declares no topology at all, so
    it could not tell "read what the file declared" apart from "default to
    linear".
    """
    body = "\n".join(
        f"{i + 1:>9} {seq[i:i + 60].lower()}" for i in range(0, len(seq), 60)
    )
    record = (
        f"LOCUS       xchk {len(seq)} bp    DNA     "
        f"{'circular' if circular else 'linear'} SYN 01-JAN-2026\n"
        f"ORIGIN\n{body}\n//\n"
    )
    path = os.path.join(tmpdir, "xchk.gb")
    with open(path, "w", encoding="utf8", newline="\n") as fh:
        fh.write(record)

    out = subprocess.run(
        [exe, "find-motif", pattern, path, "--json"],
        capture_output=True, text=True,
    )
    if out.returncode != 0:
        raise RuntimeError(f"pl find-motif {pattern} <file>: {out.stderr.strip()}")
    import json
    doc = json.loads(out.stdout)
    # The record has to come back with the topology it declared, or this
    # comparison is measuring something else and would pass vacuously for every
    # linear case -- which is exactly how the defect survived.
    if bool(doc.get("circular")) is not circular:
        raise RuntimeError(
            f"the file declared {'circular' if circular else 'linear'} and pl "
            f"read it as {'circular' if doc.get('circular') else 'linear'}"
        )
    return {(h["start"], h["strand"]) for h in doc["hits"]}


SEQS = [
    "GAATTCGGATCCAAGCTTGAATTC",
    "ATGATGATGCATCATCAT",
    "AAAAAAAAAA",
    "ACGTACGTACGTACGTACGTACGT",
    "TTCAAAAAAGAA",                       # GAATTC straddles the origin
    "GGTCTCAAAAAGAGACC",                  # BsaI and its reverse complement
    "CCAGGGGGTGGTTGACATTTTTTTATAATGCC",
    "NNNNGAATTCNNNN",                     # ambiguity around a real site
    "ACGT",
    "A",
    "GGWCCGGACCGGTCC".replace("W", "A"),
    "CCANNNNNTGGCCATTTTTTGG".replace("N", "G"),
]


def main(argv):
    exe = None
    if argv and os.path.isfile(argv[0]):
        exe = os.path.abspath(argv[0])
    if exe is None:
        print("usage: xcheck_motif.py <path to pl.exe>")
        return 1

    compared = 0
    from_file = 0
    disagreements = []
    import tempfile
    with tempfile.TemporaryDirectory() as tmpdir:
        for seq in SEQS:
            for pattern in PATTERNS:
                for circular in (True, False):
                    want = bio_find(pattern, seq, circular)
                    got = ours(exe, pattern, seq, circular)
                    compared += 1
                    if got != want:
                        disagreements.append(
                            (pattern, seq, circular, sorted(got), sorted(want)))

                    # The same question again, this time off a real file with
                    # no --topology on the command line, so the answer has to
                    # come from what the record declares. Nothing else in the
                    # repository exercises that path.
                    got = ours_from_file(exe, pattern, seq, circular, tmpdir)
                    from_file += 1
                    if got != want:
                        disagreements.append(
                            (pattern, f"{seq} (from file)", circular,
                             sorted(got), sorted(want)))

    print(f"{'='*74}")
    print(f"patterns              : {len(PATTERNS)}")
    print(f"sequences             : {len(SEQS)}")
    print(f"comparisons (--seq)   : {compared}")
    print(f"comparisons (file)    : {from_file}")
    print(f"disagreements         : {len(disagreements)}")
    print("\ndegenerate codes, both strands and origin wrapping, against a regex")
    print("built from Biopython's own IUPAC table -- a different mechanism from")
    print("our 4-bit masks, so one bug cannot appear identically in both")

    for pattern, seq, circ, got, want in disagreements[:8]:
        print(f"\n  {pattern} in {seq} ({'circular' if circ else 'linear'})")
        print(f"    ours      = {got}")
        print(f"    biopython = {want}")

    if compared == 0 or from_file == 0:
        print("\nFAIL: compared nothing")
        return 1
    if disagreements:
        print(f"\nFAIL: {len(disagreements)} disagreement(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
