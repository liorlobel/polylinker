"""Check the Python bindings against Biopython, from Python.

    python xcheck_pybindings.py <path to polylinker.pyd or .so>

The bindings exist so that a script already using Biopython can call Polylinker
for the parts that are hard to get right, without rewriting the pipeline. That
argument only holds if the two agree where they overlap, *called the way a user
would call them* — the Rust side is already cross-validated, and this checks the
boundary rather than the logic: an argument silently transposed, a coordinate
convention shifted at the FFI edge, an error returned as a value.

The last of those is the one worth naming. Every fallible binding raises rather
than returning a sentinel: a Tm of `0.0` reads as a cold oligo to
`if tm > 60`, so a failure that arrives as a number is worse than no binding at
all. That is asserted here, not assumed.

Exits 1 on any disagreement and on comparing nothing.
"""
import importlib.util
import os
import random
import sys

from Bio.Seq import Seq
from Bio import Restriction

rng = random.Random(20260730)


def load(path):
    spec = importlib.util.spec_from_file_location("polylinker", path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def rand_seq(n):
    return "".join(rng.choice("ACGT") for _ in range(n))


def main(argv):
    if not argv or not os.path.isfile(argv[0]):
        print("usage: xcheck_pybindings.py <polylinker.pyd|.so>")
        return 1
    pl = load(os.path.abspath(argv[0]))

    checks = 0
    bad = []

    # --- reverse complement -------------------------------------------------
    for _ in range(200):
        s = rand_seq(rng.randint(1, 120))
        checks += 1
        if pl.reverse_complement(s) != str(Seq(s).reverse_complement()):
            bad.append(("reverse_complement", s))

    # --- restriction digestion ---------------------------------------------
    # Biopython's Restriction is the oracle the Rust side already answers to;
    # this checks that the *binding* passes the arguments through unchanged,
    # including the circular flag, which is the easiest thing to drop at an FFI
    # boundary and the hardest to notice.
    names = [n for n, _ in pl.enzymes()]
    shared = [n for n in names if hasattr(Restriction, n)]
    for _ in range(60):
        n = rng.randint(200, 2000)
        seq = rand_seq(n)
        name = rng.choice(shared)
        enz = getattr(Restriction, name)
        for circular in (False, True):
            checks += 1
            ours = pl.cut_positions(seq, name, circular)
            theirs = enz.search(Seq(seq), linear=not circular)
            if sorted(ours) != sorted(theirs):
                bad.append((f"cut_positions {name} circular={circular}",
                            f"{sorted(ours)[:6]} vs {sorted(theirs)[:6]}"))

    # Sites planted *across the origin*, because random sequence almost never
    # has one and without these the circular flag is untestable: dropping it at
    # the boundary changed nothing at all in the first version of this file.
    # A site split across the join is the only case where circular and linear
    # must give different answers.
    origin_cases = 0
    for name in shared:
        enz = getattr(Restriction, name)
        site = str(enz.site)
        if len(site) < 4 or set(site) - set("ACGT"):
            continue  # an ambiguous site would need a resolved instance
        for k in range(1, len(site)):
            # tail of the site at the end, head of it at the start.
            seq = site[k:] + rand_seq(300) + site[:k]
            circ = sorted(pl.cut_positions(seq, name, True))
            lin = sorted(pl.cut_positions(seq, name, False))
            want_circ = sorted(enz.search(Seq(seq), linear=False))
            want_lin = sorted(enz.search(Seq(seq), linear=True))
            checks += 2
            origin_cases += 1
            if circ != want_circ:
                bad.append((f"cut_positions {name} across the origin",
                            f"{circ} vs {want_circ}"))
            if lin != want_lin:
                bad.append((f"cut_positions {name} linear", f"{lin} vs {want_lin}"))
            if circ == lin:
                bad.append((f"cut_positions {name}",
                            "circular and linear agree on a site that spans the "
                            "origin, so this case proves nothing"))
    if origin_cases < 20:
        bad.append(("setup", f"only {origin_cases} origin-spanning cases built"))

    # A digest's fragments must add up to the molecule, whatever the enzymes.
    for _ in range(40):
        n = rng.randint(500, 3000)
        seq = rand_seq(n)
        chosen = rng.sample(shared, rng.randint(1, 3))
        for circular in (False, True):
            checks += 1
            frags = pl.digest(seq, chosen, circular)
            if frags and sum(frags) != n:
                bad.append((f"digest {chosen} circular={circular}",
                            f"{sum(frags)} != {n}"))
            if frags != sorted(frags, reverse=True):
                bad.append((f"digest {chosen}", "not descending"))

    # --- translation and the genetic codes ---------------------------------
    ids = [i for i, _, _ in pl.genetic_codes()]
    for table in ids:
        for _ in range(6):
            seq = rand_seq(rng.randint(1, 40) * 3)
            checks += 1
            ours = pl.translate(seq, table)
            theirs = str(Seq(seq).translate(table=table))
            if ours != theirs:
                bad.append((f"translate table {table}", f"{ours} vs {theirs}"))

    # The fact the binding refuses to default: 13 codes do not stop at TGA.
    checks += 1
    readthrough = [i for i, _, tga_stop in pl.genetic_codes() if not tga_stop]
    if len(readthrough) != 13:
        bad.append(("genetic_codes", f"{len(readthrough)} read through TGA, expected 13"))

    # --- checksums ----------------------------------------------------------
    # cdseguid is invariant to rotation and to which strand is written first.
    # Getting that wrong at the boundary -- passing the strands transposed --
    # produces a checksum that is stable and wrong.
    for _ in range(40):
        seq = rand_seq(rng.randint(20, 300))
        r = rng.randrange(len(seq))
        rotated = seq[r:] + seq[:r]
        checks += 1
        if pl.cdseguid(seq) != pl.cdseguid(rotated):
            bad.append(("cdseguid", "not rotation invariant"))
        checks += 1
        rc = str(Seq(seq).reverse_complement())
        if pl.cdseguid(seq) != pl.cdseguid(rc):
            bad.append(("cdseguid", "not strand invariant"))

    # --- errors are exceptions ---------------------------------------------
    # The property that matters most for a numeric API. A Tm of 0.0 reads as a
    # cold oligo; a failure must not arrive as a number.
    for bad_input in ("ACGTN", "ACGU", "", "A", "acgt-acgt"):
        checks += 1
        try:
            v = pl.melting_temperature(bad_input)
            bad.append(("melting_temperature", f"{bad_input!r} returned {v} instead of raising"))
        except ValueError:
            pass
        except Exception as e:
            bad.append(("melting_temperature", f"{bad_input!r} raised {type(e).__name__}"))

    for fn, args in (
        (pl.cut_positions, ("ACGT", "NoSuchEnzyme", False)),
        (pl.translate, ("ACGT", 99)),
        (pl.methods, ("no-such-topic",)),
    ):
        checks += 1
        try:
            fn(*args)
            bad.append((fn.__name__, "a bad name returned instead of raising"))
        except KeyError:
            pass
        except Exception as e:
            bad.append((fn.__name__, f"raised {type(e).__name__}, expected KeyError"))

    # A Tm that *can* be computed still agrees with the Rust side's oracle
    # range; the binding must not, say, be passing mM where M was meant.
    checks += 1
    t = pl.melting_temperature("GTAAAACGACGGCCAGT")
    if not 45.0 < t < 55.0:
        bad.append(("melting_temperature", f"M13 forward came out at {t:.1f} C"))

    print("=" * 74)
    print(f"checks: {checks:,}")
    print(f"disagreements: {len(bad)}")
    print()
    print("Biopython is the oracle the Rust side already answers to, so this")
    print("checks the boundary rather than the logic: arguments passed through")
    print("unchanged, coordinates unshifted, and failures raised rather than")
    print("returned as a number a caller would read as a measurement.")

    for what, detail in bad[:8]:
        print(f"\n  {what}: {detail}")

    if checks == 0:
        print("\nFAIL: checked nothing")
        return 1
    if bad:
        print(f"\nFAIL: {len(bad)} disagreement(s)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
