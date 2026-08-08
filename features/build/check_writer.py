#!/usr/bin/env python3
"""Prove the build's writer reads features/SIGNOFF.tsv and never writes it.

THIS CHECK TOUCHES NO NETWORK. That is the point of it existing separately from
the end-to-end build step in `.github/workflows/ci.yml`, and the reason is worth
stating plainly because it is a lesson about gates and not about sequences.

The end-to-end step ran the real `build.py` against live EBI, NCBI, UniProt and
RCSB, and asserted that the writer had run by requiring `features.tsv` to appear
in a scratch directory. That assertion is right -- without it the step passes
whenever the build dies early, which is a check that cannot fail. But it made
the gate's colour depend on four third parties' uptime. On 2026-08-07 EBI timed
out twice in a row (`AAB47270.1`, then `AAB59737.1`) and main went red twice for
a reason no commit under test had anything to do with. A red gate that means
nothing is worse than no gate: it teaches people that red is noise.

So the property was separated from the weather. What this file proves, offline
and deterministically, on every push:

  1. The writer, given the real shipped table and the real signatures, emits
     `features.tsv` and `provenance.tsv` and NO `SIGNOFF.tsv` -- anywhere under
     the output directory, not merely at its top level.
  2. The sign-off file it read is byte-for-byte unchanged afterwards.
  3. The statuses it wrote are the ones the sign-off granted, and rows the
     sign-off does not name come out `proposed` with no curator. This is the
     "reads" half of the sentence, WHICH NOTHING CHECKED BEFORE: the CI step was
     named "The build reads SIGNOFF.tsv and never writes it" and only ever
     tested the second clause, so a build that ignored the file completely
     passed it.

And it runs at full strength while doing so. The rows are not a fixture: they
are the ~900 records of the shipped `features/features.tsv`, rebuilt through
`check_signoff.row_from_tsv` so that every signed cell goes through the same
canonicalisation the digest is defined over, and the signatures are the real
ones in `features/SIGNOFF.tsv`. A network outage used to leave the writer
audited over zero rows; here the row count is a committed fact.

Why the negative controls run first
-----------------------------------

`docs/PLAN.md` §8.3: a check that cannot fail proves nothing. Every assertion
here is of the form "this file did NOT appear" or "those bytes did NOT change",
and every one of them is trivially satisfiable by an audit that looks in the
wrong directory, hashes the wrong file, or never runs the writer at all. Those
failures are silent and permanent -- green forever, checking nothing -- and they
are exactly the failures `check_signoff.py` plants forbidden edits to exclude
and `build.self_test()` feeds gate-tripping input to exclude.

So `main()` hands `audit()` a series of deliberately misbehaving writers and
requires it to catch each one, before it will certify the real one. Sabotaging
the writer rather than the table is what makes these controls test THIS file:
the thing under audit is a program's behaviour, so the control has to be a
program that misbehaves.

**And one INVERTED control**, for the same reason `check_signoff.py` has one: an
honest writer must produce no violations at all. An audit that fires on
everything is exactly as worthless as one that fires on nothing, and without
this control every assertion below could be hard-wired to report a violation and
the file would still look like it worked.

Nothing here writes to the repository. The controls that rewrite a sign-off file
are pointed at a copy in a temporary directory; the real `features/SIGNOFF.tsv`
is only ever read, and hashed to prove it stayed that way.

Usage
-----
    python features/build/check_writer.py            # exit 0 if clean and provably checkable
    python features/build/check_writer.py --quiet
"""

from __future__ import annotations

import argparse
import hashlib
import shutil
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from build import (  # noqa: E402
    apply_signoff,
    read_signoff,
    write_outputs,
)
from check_signoff import (  # noqa: E402
    CURATOR,
    ID,
    STATUS,
    data_rows,
    row_from_tsv,
)
from lib_columns import FEATURE_COLUMNS, PROVENANCE_COLUMNS  # noqa: E402


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def audit(writer, rows: list, decided: dict, signoff: Path, out: Path) -> list[str]:
    """Run `writer` into `out` and report every way it broke the rule.

    `writer` is a parameter, not a hard-wired call to `write_outputs`, purely so
    that main() can hand this function writers that misbehave and check that it
    notices. A control that cannot be expressed cannot be run.
    """
    problems: list[str] = []

    # Hash BEFORE, so the comparison is against what the writer was given rather
    # than against whatever is on disk when we get round to looking. Absence is
    # recorded as a distinct state: a writer that DELETES the sign-off file has
    # not left it unchanged, and comparing a hash against a file that is no
    # longer there would otherwise throw rather than report.
    before = sha256(signoff) if signoff.exists() else None

    # Unguarded on purpose. Catching here and reporting "the writer raised" as a
    # violation would make a control that crashes for an unintended reason count
    # as caught -- a control passing for the wrong reason is the one failure this
    # file cannot afford. An exception escapes as a traceback and a non-zero
    # exit, which is loud, red, and impossible to mistake for a result.
    writer(out, rows, decided)

    # -- 1. the writer ran at all -----------------------------------------
    #
    # The vacuity guard, and the reason the end-to-end step is right to demand
    # this. Every assertion below is about something NOT happening, so all of
    # them pass trivially if the writer never ran.
    features = out / "features.tsv"
    if not features.exists():
        return [f"the writer produced no {features}, so this audit checked nothing"]
    if not (out / "provenance.tsv").exists():
        problems.append("the writer produced no provenance.tsv")

    # -- 2. no SIGNOFF.tsv in the output, at any depth ---------------------
    #
    # `rglob`, not a top-level test. A writer that emits `out/attic/SIGNOFF.tsv`
    # has done the forbidden thing, and the end-to-end step's `test ! -e
    # /tmp/plf-out/SIGNOFF.tsv` would not see it. Matched case-insensitively
    # because the runner is ext4 and half the contributors are on NTFS, where
    # `signoff.tsv` and `SIGNOFF.tsv` are the same file.
    for stray in sorted(out.rglob("*")):
        if stray.is_file() and stray.name.lower() == "signoff.tsv":
            problems.append(
                f"the writer emitted {stray.relative_to(out)} into its output "
                f"directory; a sign-off comes from a human, not from a program"
            )

    # -- 3. the sign-off file it read is untouched -------------------------
    after = sha256(signoff) if signoff.exists() else None
    if after is None and before is not None:
        problems.append(f"the writer deleted {signoff}")
    elif before is not None and after != before:
        problems.append(
            f"the writer modified {signoff} ({before[:12]} -> {after[:12]}); "
            f"a sign-off comes from a human, not from a program"
        )

    # -- 4. it READ the file: every status it wrote is one a human granted --
    #
    # The clause the CI step named and never checked. Without this, a writer
    # that never opened SIGNOFF.tsv -- or one that stamped 'verified' on
    # everything -- satisfies 1 to 3 completely.
    written, header_problems = data_rows(
        features.read_text(encoding="utf8"), FEATURE_COLUMNS, "features.tsv"
    )
    problems += header_problems
    if len(written) != len(rows):
        problems.append(
            f"the writer was given {len(rows)} row(s) and wrote {len(written)}"
        )
    for cells in written:
        rid = cells[ID].strip()
        want_status, want_curator = decided.get(rid, ("proposed", ""))
        got_status = cells[STATUS].strip()
        got_curator = cells[CURATOR].strip()
        if got_status != want_status:
            problems.append(
                f"{rid}: the sign-off granted {want_status!r} and the writer "
                f"wrote {got_status!r}"
            )
        if got_curator != want_curator:
            problems.append(
                f"{rid}: the sign-off names curator {want_curator!r} and the "
                f"writer wrote {got_curator!r}"
            )
    return problems


# --------------------------------------------------------------------------
# The misbehaving writers. Each does exactly one forbidden thing.


def writer_leaks_signoff(out: Path, rows: list, decided: dict) -> int:
    """Writes a plausible SIGNOFF.tsv beside its output. The headline failure."""
    n = write_outputs(out, rows, decided)
    (out / "SIGNOFF.tsv").write_text(
        "record_id\treview_status\tcurator\tsigned_date\tcontent_sha256\tnote\n",
        encoding="utf8",
    )
    return n


def writer_hides_signoff(out: Path, rows: list, decided: dict) -> int:
    """The same, one directory down, where a top-level `test -e` cannot see it."""
    n = write_outputs(out, rows, decided)
    (out / "attic").mkdir(parents=True, exist_ok=True)
    (out / "attic" / "SIGNOFF.tsv").write_text("record_id\n", encoding="utf8")
    return n


def make_writer_signs_itself(signoff: Path):
    """Appends its own signature to the sign-off file it was supposed to read.

    The failure the whole control exists for, and the one that would be quietest
    in practice: the table it writes and the file it signed agree perfectly, so
    `check_signoff.py` certifies the result clean. Only watching the FILE catches
    it.
    """

    def writer(out: Path, rows: list, decided: dict) -> int:
        n = write_outputs(out, rows, decided)
        with signoff.open("a", encoding="utf8") as fh:
            fh.write("PLF:0001\tverified\tthe build\t1970-01-01\t" + "0" * 64 + "\t-\n")
        return n

    return writer


def writer_does_nothing(out: Path, rows: list, decided: dict) -> int:
    """Produces no table. Must be caught, or every check above is vacuous."""
    out.mkdir(parents=True, exist_ok=True)
    return 0


def writer_reference(out: Path, rows: list, decided: dict) -> int:
    """A correct writer, written HERE and owing nothing to build.py.

    This is the control that lets the two possible readings of "the real writer
    was reported as violating the rule" be told apart, and without it they
    cannot be. An audit that fires on every input catches all five saboteurs
    above -- being unable to report a clean result is not something they can
    detect -- so "the controls were caught" is not evidence that the audit
    discriminates. Only an input it reports nothing about is.

    Which makes the diagnosis exact. If this comes back clean and
    `write_outputs` does not, the audit demonstrably can report both answers and
    the finding is about build.py. If this comes back dirty, the audit is broken
    and says nothing about build.py at all. The first version of this file
    printed "CHECK IS WORTHLESS ... fix the audit" for both cases, which on a
    genuinely leaking writer is a false sentence pointing the reader at the
    wrong file.

    Deliberately minimal: it satisfies the schema and the statuses it was
    handed, and does nothing else. It is not a second implementation to be kept
    in step with the first -- if the audit ever needs more of it than this, the
    audit has started checking the table's content, which is check_signoff.py's
    job and not this file's.
    """
    out.mkdir(parents=True, exist_ok=True)
    with (out / "features.tsv").open("w", encoding="utf8", newline="\n") as fh:
        fh.write("\t".join(FEATURE_COLUMNS) + "\n")
        for r in rows:
            status, curator = decided.get(r.id, ("proposed", ""))
            cells = {"id": r.id, "review_status": status, "curator": curator}
            fh.write("\t".join(cells.get(c, "-") for c in FEATURE_COLUMNS) + "\n")
    with (out / "provenance.tsv").open("w", encoding="utf8", newline="\n") as fh:
        fh.write("\t".join(PROVENANCE_COLUMNS) + "\n")
    return 0


def writer_overstates_status(out: Path, rows: list, decided: dict) -> int:
    """Stamps 'verified' on every row regardless of what was signed.

    The negative control for clause 4. A writer doing this has not read the
    sign-off in any sense that matters, and clauses 1 to 3 are all happy with it.
    """
    return write_outputs(out, rows, {rid: ("verified", "the build") for rid in decided})


# --------------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dir", default=str(ROOT), help="directory holding the tables")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    d = Path(args.dir)
    features_text = (d / "features.tsv").read_text(encoding="utf8")
    provenance_text = (d / "provenance.tsv").read_text(encoding="utf8")
    signoff = d / "SIGNOFF.tsv"

    cells, problems = data_rows(features_text, FEATURE_COLUMNS, "features.tsv")
    if problems:
        print(f"features.tsv does not parse, so nothing can be certified: {problems}")
        return 2
    if not cells:
        print("features.tsv has no data rows; there is nothing to certify.")
        return 2

    prov, prov_problems = data_rows(
        provenance_text, PROVENANCE_COLUMNS, "provenance.tsv"
    )
    if prov_problems:
        print(f"provenance.tsv does not parse: {prov_problems}")
        return 2

    # The real shipped rows, rebuilt through the same canonicalisation the
    # digest is defined over, so the signatures in SIGNOFF.tsv still apply to
    # them. This is what keeps the audit at full strength without a network.
    rows = [row_from_tsv(c, prov) for c in cells]

    # The real read path, on the real file. `apply_signoff` is what turns 64 hex
    # characters into a status, and running it here rather than trusting the
    # status column means the audit compares the writer against the SIGN-OFF,
    # not against the table the writer is about to reproduce.
    signed, _ = read_signoff(signoff)
    decided, _ = apply_signoff(rows, signed)
    granted = sorted(rid for rid, (status, _c) in decided.items() if status != "proposed")

    if not args.quiet:
        print(f"{len(rows)} shipped row(s), {len(signed)} signature(s) on file, "
              f"{len(granted)} still valid")

    # THE CONTROL THAT VALIDATES THE CONTROLS. If nothing in the shipped table is
    # signed, `decided` is uniformly 'proposed', and clause 4 -- the entire
    # "reads" half -- is satisfied by a writer that never opens the file. The
    # controls below would still pass and this file would still print OK while
    # proving strictly less than its name claims. That state is legitimate for
    # the repository (nothing signed is a real state SIGNOFF.tsv supports) but it
    # is NOT a state in which this check may certify the read half in silence.
    if not granted:
        print("CANNOT CERTIFY THE READ HALF: no signature in features/SIGNOFF.tsv")
        print("currently grants any shipped row, so 'the writer wrote the status")
        print("the human granted' is satisfied by a writer that ignores the file.")
        print("Sign a row, or delete this check and stop claiming it is tested.")
        return 2

    scratch = Path(tempfile.mkdtemp(prefix="plf-writer-audit-"))
    try:
        # -- negative controls, before anything is certified ---------------
        #
        # Each gets a private output directory, and the two that write to a
        # sign-off file get a private COPY of it. Nothing below can reach
        # features/SIGNOFF.tsv.
        n = 0

        # Each control is a factory taking the sign-off path the audit will
        # watch, so the one that appends a signature is handed the same copy the
        # audit hashes -- wiring it any other way is how a control ends up
        # sabotaging one file while the audit watches another and "catches"
        # nothing for the wrong reason.
        controls = (
            ("a writer that emits SIGNOFF.tsv", lambda _s: writer_leaks_signoff),
            ("a writer that hides SIGNOFF.tsv in a subdirectory",
             lambda _s: writer_hides_signoff),
            ("a writer that appends its own signature", make_writer_signs_itself),
            ("a writer that produces no table", lambda _s: writer_does_nothing),
            ("a writer that stamps 'verified' on everything",
             lambda _s: writer_overstates_status),
        )
        for n, (label, factory) in enumerate(controls, start=1):
            # A private copy, never features/SIGNOFF.tsv. A control that mutates
            # the repository's own sign-off file would be a check with a side
            # effect on the thing it certifies.
            watched = scratch / f"control-{n}-SIGNOFF.tsv"
            shutil.copyfile(signoff, watched)
            if not audit(factory(watched), rows, decided, watched, scratch / f"control-{n}"):
                print(f"CHECK IS WORTHLESS: {label} and audit() reported nothing.")
                print("Refusing to certify the real writer with an audit that cannot fail.")
                return 2
            if not args.quiet:
                print(f"  control  {label}: caught")

        # -- the inverted control ------------------------------------------
        #
        # An input the audit must report NOTHING about. Everything above is an
        # assertion that something did not happen, so all of it is satisfied by
        # an audit hard-wired to complain -- and such an audit catches all five
        # saboteurs too, which is why they are not evidence on their own.
        #
        # `writer_reference`, not `write_outputs`: the honest reference is
        # correct by construction, so a violation here can only be the audit's
        # fault. Using build.py's writer for this conflates the two answers, and
        # then the ONE case that matters -- a real leak -- prints a lecture about
        # fixing the audit.
        broken = audit(writer_reference, rows, decided, signoff, scratch / "reference")
        if broken:
            print("CHECK IS WORTHLESS: a reference writer that does nothing wrong was")
            print("reported as violating the rule, so the audit fires on everything and")
            print("catching the controls above meant nothing. This says NOTHING about")
            print("build.py — fix the audit first:")
            for p in broken:
                print(f"  - {p}")
            return 2
        if not args.quiet:
            print("  control  a reference writer that does nothing wrong: clean")

        # -- the real certification ----------------------------------------
        #
        # Now, and only now, a violation is a finding about build.py: the audit
        # has been shown to report both answers on inputs whose correct answer is
        # known.
        real = audit(write_outputs, rows, decided, signoff, scratch / "real")
        if real:
            print("\nbuild.py's writer BROKE THE RULE. The audit was shown above to catch")
            print("five planted violations and to pass a clean writer, so this is a")
            print("finding about build.py and not about this check:")
            for p in real:
                print(f"  - {p}")
            return 1

        print(f"\nthe writer ran over {len(rows)} shipped row(s), wrote the "
              f"{len(granted)} status(es) a human granted and no others,")
        print(f"emitted no SIGNOFF.tsv, and left {signoff} byte-identical.")
        print("no network was used, so this result does not depend on anyone's uptime.")
        return 0
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
