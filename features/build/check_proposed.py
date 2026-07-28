#!/usr/bin/env python3
"""Prove that the shipped database asserts nothing.

The rule this file exists to enforce is `docs/PLAN.md` §8.3 rule 6, the one the
project cannot bend: **AI may propose, never assert.** Every row in
`features/features.tsv` is `proposed` with an empty `curator`, so `Db::reviewed()`
yields an empty database and an annotator built on it finds nothing. That is the
intended state of the output, not an unfinished one.

Why this is a separate file from build.py
-----------------------------------------

build.py writes the literal string `"proposed"` and the literal `""` into every
row, so of course the file it just wrote satisfies the rule. Checking the rule
inside the writer proves only that the writer is consistent with itself. This
reads the file back off disk, from the bytes, and knows nothing about how they
got there — so it also catches a row edited by hand, a merge that resurrected an
old table, and a stage that found some other route into the file.

Why it runs a negative control
------------------------------

`§8.3: a check that cannot fail proves nothing.` A verifier that walks a
conforming file and prints OK is indistinguishable from a verifier with the
comparison inverted, or one whose loop never executes because the parser silently
returned zero rows. That failure is *silent and permanent*: it would report a
green result forever while checking nothing.

So `main()` does not merely run `violations()` against the real file. It also
takes the real file, injects each of the two forbidden edits into it, and
requires `violations()` to catch each one. If a planted violation is not caught,
this exits non-zero and says the check is worthless — which is a much more useful
thing to be told than "OK".

Usage
-----
    python features/build/check_proposed.py            # exit 0 if clean and provably checkable
    python features/build/check_proposed.py --quiet
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from build import CLEARED_SOURCES  # noqa: E402
from lib_columns import FEATURE_COLUMNS, PROVENANCE_COLUMNS  # noqa: E402

STATUS = FEATURE_COLUMNS.index("review_status")
CURATOR = FEATURE_COLUMNS.index("curator")
ID = FEATURE_COLUMNS.index("id")


def data_rows(text: str, columns: list[str], what: str) -> tuple[list[list[str]], list[str]]:
    """Split a TSV into data rows, checking the header against the schema.

    The header check is the other half of the pin between `lib_columns.py` and
    `crates/pl-features/src/lib.rs`: the Rust loader compares the header against
    its own `FEATURE_COLUMNS` constant and refuses the file if they differ, so a
    column added on one side and not the other is caught. Repeating it here means
    the same mismatch is named by the build rather than surfacing as a Rust load
    error with no indication of which side moved.
    """
    problems, rows, header_seen = [], [], False
    for line in text.splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        cells = line.split("\t")
        if not header_seen:
            header_seen = True
            if cells != columns:
                problems.append(
                    f"{what}: header does not match lib_columns.py "
                    f"({len(cells)} columns, expected {len(columns)})"
                )
            continue
        if len(cells) != len(columns):
            problems.append(f"{what}: a row has {len(cells)} columns, expected {len(columns)}")
            continue
        rows.append(cells)
    if not header_seen:
        problems.append(f"{what}: no header row")
    return rows, problems


def violations(features_text: str, provenance_text: str) -> list[str]:
    """Every way the shipped tables could break the propose-never-assert rule.

    Returns a list of problems; empty means clean. Never raises on bad input —
    a parse failure is itself reported as a violation, because "the file could
    not be read" must not be able to masquerade as "the file is fine".
    """
    rows, problems = data_rows(features_text, FEATURE_COLUMNS, "features.tsv")
    prov, prov_problems = data_rows(provenance_text, PROVENANCE_COLUMNS, "provenance.tsv")
    problems += prov_problems

    # An empty table would satisfy every per-row rule below vacuously. That is
    # the shape of failure this whole file is written against, so it is a
    # violation in its own right rather than a quiet pass.
    if not rows:
        problems.append(
            "features.tsv holds no data rows, so every per-row check below would "
            "pass without examining anything"
        )
        return problems

    for r in rows:
        rid = r[ID] or "(unnamed row)"
        status = r[STATUS].strip().lower()
        curator = r[CURATOR].strip()
        if status != "proposed":
            problems.append(
                f"{rid}: review_status is {r[STATUS]!r}, not 'proposed'. Only a named "
                f"human may move a row past 'proposed'."
            )
        if curator:
            problems.append(
                f"{rid}: curator is {curator!r}. A machine-assembled row carries no "
                f"signature; the curator field is a human's, or it is empty."
            )

    ids = [r[ID] for r in rows]
    dupes = sorted({i for i in ids if ids.count(i) > 1})
    if dupes:
        problems.append(f"duplicate record id(s): {dupes}. A PLF id is a permanent name.")

    # The other promise the file makes: every sequence can say where it came from.
    sourced = {p[0] for p in prov if p[1] == "reference_nt"}
    unsourced = sorted(set(ids) - sourced)
    if unsourced:
        problems.append(f"no reference_nt provenance for: {unsourced}")

    # And the promise features/NOTICE makes on top of that: which source each
    # field came from, and under what licence. Both halves were unenforced.
    #
    # A `field` value that is not a column attributes nothing, silently: forty
    # shipped rows keyed on `citation` and `peptide_anchor`, neither of which is
    # in the schema, while the column the sourced text really landed in was
    # labelled own-work.
    for p in prov:
        if p[1] not in FEATURE_COLUMNS:
            problems.append(
                f"{p[0]}: provenance names field {p[1]!r}, which is not a column of "
                f"features.tsv, so it attributes nothing"
            )

    # And the source itself, against features/SOURCING.md section 1. This was
    # author discipline and nothing else: two provenance rows citing Addgene and
    # PlasMapper were appended to a copy of the shipped table by hand, and this
    # program exited 0, counted them, and objected to neither.
    for p in prov:
        allowed = CLEARED_SOURCES.get(p[2])
        if allowed is None:
            problems.append(
                f"{p[0]} field {p[1]}: source_db {p[2]!r} is not a source "
                f"features/SOURCING.md section 1 cleared for use as data"
            )
        elif p[4] not in allowed:
            problems.append(
                f"{p[0]} field {p[1]}: {p[2]} is cleared under {sorted(allowed)}, "
                f"not {p[4]!r}"
            )

    return problems


def plant(features_text: str, column: int, value: str) -> str:
    """Return the table with `value` written into `column` of its first data row.

    Used only by the negative control. Operates on the real shipped text rather
    than on a fixture so that the control exercises the same parse path, the same
    column offsets and the same row shape as the real check — a control that runs
    against a hand-built two-line fixture can pass while the real parse silently
    returns nothing.
    """
    lines = features_text.splitlines()
    header_seen = False
    for i, line in enumerate(lines):
        if not line.strip() or line.startswith("#"):
            continue
        if not header_seen:
            header_seen = True
            continue
        cells = line.split("\t")
        if len(cells) != len(FEATURE_COLUMNS):
            continue
        cells[column] = value
        # Rebuild by index. An earlier version tracked position by the length of
        # the output list and was off by one, so it planted the violation *and*
        # deleted the row after it. The control still passed — it was catching
        # the violation it planted — while quietly testing a table one row short
        # of the real one. Slicing the original list makes the edit provably
        # local, which `main()` then asserts.
        return "\n".join(lines[:i] + ["\t".join(cells)] + lines[i + 1:]) + "\n"
    raise SystemExit("could not plant a violation: the table has no data row to edit")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dir", default=str(ROOT), help="directory holding the two TSVs")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    d = Path(args.dir)
    features_text = (d / "features.tsv").read_text(encoding="utf8")
    provenance_text = (d / "provenance.tsv").read_text(encoding="utf8")

    # --- the negative control, first -------------------------------------
    # Run before the real check, deliberately. If the check is broken, the run
    # should stop here rather than print a green result and then explain that
    # the green result means nothing.
    controls = [
        ("review_status flipped to 'reviewed'", STATUS, "reviewed"),
        ("a curator name written onto a proposed row", CURATOR, "L. Lobel"),
    ]
    real_rows, _ = data_rows(features_text, FEATURE_COLUMNS, "features.tsv")
    for label, column, value in controls:
        planted = plant(features_text, column, value)
        # The control must differ from the real table in exactly one cell. A
        # planter that also drops or duplicates rows is testing a table that is
        # not the one being certified, and it will still appear to "catch" the
        # violation it planted.
        planted_rows, _ = data_rows(planted, FEATURE_COLUMNS, "features.tsv")
        differing = [
            (a[ID], i) for a, b in zip(real_rows, planted_rows)
            for i in range(len(FEATURE_COLUMNS)) if a[i] != b[i]
        ]
        if len(planted_rows) != len(real_rows) or len(differing) != 1:
            print(f"CONTROL IS INVALID: planting {label} changed "
                  f"{len(real_rows)} rows -> {len(planted_rows)} rows and "
                  f"{len(differing)} cell(s); it must change exactly one cell.")
            return 2
        if not violations(planted, provenance_text):
            print(f"CHECK IS WORTHLESS: planted {label} and violations() reported nothing.")
            print("Refusing to certify the real file with a verifier that cannot fail.")
            return 2
        if not args.quiet:
            print(f"  control  {label}: caught")

    if not violations("", ""):
        print("CHECK IS WORTHLESS: violations() reported nothing for an empty table.")
        return 2
    if not args.quiet:
        print("  control  an empty table: caught (it would pass every per-row rule vacuously)")

    # The exact two rows that were appended to a copy of the shipped table by
    # hand, which this program then counted, accepted and certified green. The
    # NO_GO list in features/SOURCING.md was author discipline and nothing else;
    # discipline is not a control, and a control nobody plants is not a control
    # either. Also a phantom `field`, which attributes nothing and did so
    # silently on 40 shipped rows.
    first_id = real_rows[0][ID]
    tab = "\t"
    for label, cells in (
        ("an Addgene-sourced provenance row",
         [first_id, "description", "addgene", "Addgene-52961",
          "noncommercial-informational-only", "https://www.addgene.org/52961/",
          "2026-07-28", "deadbeef"]),
        ("a PlasMapper-sourced provenance row",
         [first_id, "reference_nt", "plasmapper", "FeatureDB-scraped-from-Addgene",
          "GPL-3.0-scraped", "https://plasmapper.ca/featuredb",
          "2026-07-28", "cafebabe"]),
        ("a provenance row keyed on a field that is not a column",
         [first_id, "citation", "rfam", "RF00000", "CC0-1.0",
          "https://ftp.ebi.ac.uk/pub/databases/Rfam/CURRENT/Rfam.seed.gz",
          "2026-07-28", "beef"]),
    ):
        tainted = provenance_text.rstrip("\n") + "\n" + tab.join(cells) + "\n"
        if not violations(features_text, tainted):
            print(f"CHECK IS WORTHLESS: planted {label} and violations() reported nothing.")
            print("Refusing to certify the real file with a verifier that cannot fail.")
            return 2
        if not args.quiet:
            print(f"  control  {label}: caught")

    # --- the real check ---------------------------------------------------
    problems = violations(features_text, provenance_text)
    if problems:
        print(f"\n{len(problems)} violation(s) in {d}:")
        for p in problems:
            print(f"  - {p}")
        return 1

    rows, _ = data_rows(features_text, FEATURE_COLUMNS, "features.tsv")
    prov, _ = data_rows(provenance_text, PROVENANCE_COLUMNS, "provenance.tsv")
    print(
        f"\n{len(rows)} row(s), {len(prov)} provenance row(s): every row is 'proposed' "
        f"with an empty curator."
    )
    print("Db::reviewed() therefore yields zero records, which is the intended state.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
