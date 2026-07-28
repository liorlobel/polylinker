#!/usr/bin/env python3
"""Prove that the shipped database asserts only what a human signed.

This file replaces `check_proposed.py`, which asserted a rule the PI repealed on
2026-07-28 when he turned curator sign-off on: *every row is proposed*. A file
whose name states a repealed rule is itself a stale assertion, and this project
treats that as a defect, so the name moved with the rule.

The rule that remains is `docs/PLAN.md` §8.3 rule 6, which did **not** change:
**AI may propose, never assert.** What changed is the mechanism by which a human
asserts. A row may sit above `proposed` if, and only if, `features/SIGNOFF.tsv`
names it, with a matching curator and a `content_sha256` that still equals the
row's recomputed content digest. Everything else is `proposed` with an empty
curator, and `Db::reviewed()` ships only the remainder.

THE GOVERNING INVARIANT, which every clause below preserves: a missing, stale,
malformed or unreadable sign-off can only ever REMOVE trust, never add it.

Why this is a separate file from build.py
-----------------------------------------

build.py decides each row's status and then writes it, so of course the file it
just wrote agrees with the decision it just made. Checking the rule inside the
writer proves only that the writer is consistent with itself. This reads the
bytes back off disk and knows nothing about how they got there — so it also
catches a row edited by hand, a merge that resurrected an old table, a
`SIGNOFF.tsv` line added without rebuilding, and a stage that found some other
route into the file.

Why it runs its negative controls first
---------------------------------------

`§8.3: a check that cannot fail proves nothing.` A verifier that walks a
conforming file and prints OK is indistinguishable from one with the comparison
inverted, or one whose loop never runs because the parser silently returned zero
rows. That failure is silent and permanent: green forever, checking nothing.

So `main()` plants each forbidden edit into the real tables and requires
`violations()` to catch it, before it will certify anything.

**And one INVERTED control, which is the most valuable and the easiest to
skip:** change only `date_added` on a signed row and require `violations()` to
report *nothing*. A check that fires on everything is exactly as worthless as
one that fires on nothing — and without this control, the digest could quietly
be defined over the whole row, which would lapse every signature in the
repository on every build, because build.py stamps `date_added` from the clock.

Usage
-----
    python features/build/check_signoff.py            # exit 0 if clean and provably checkable
    python features/build/check_signoff.py --quiet
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from build import (  # noqa: E402
    CLEARED_SOURCES,
    Row,
    canon_alias_list,
    content_digest,
    parse_flag,
    unesc,
)
from lib_columns import (  # noqa: E402
    FEATURE_COLUMNS,
    PROVENANCE_COLUMNS,
    SIGNOFF_COLUMNS,
)

ID = FEATURE_COLUMNS.index("id")
STATUS = FEATURE_COLUMNS.index("review_status")
CURATOR = FEATURE_COLUMNS.index("curator")
NT = FEATURE_COLUMNS.index("reference_nt")
AA = FEATURE_COLUMNS.index("reference_aa")
DESCRIPTION = FEATURE_COLUMNS.index("description")
DATE_ADDED = FEATURE_COLUMNS.index("date_added")
PATENT = FEATURE_COLUMNS.index("patent_flag")

S_ID = SIGNOFF_COLUMNS.index("record_id")
S_STATUS = SIGNOFF_COLUMNS.index("review_status")
S_CURATOR = SIGNOFF_COLUMNS.index("curator")
S_DIGEST = SIGNOFF_COLUMNS.index("content_sha256")


def data_rows(text: str, columns: list[str], what: str) -> tuple[list[list[str]], list[str]]:
    """Split a TSV into data rows, checking the header against the schema.

    The header check is the other half of the pin between `lib_columns.py` and
    `crates/pl-features/src/lib.rs`: the Rust loader compares the header against
    its own constant and refuses the file if they differ, so a column added on
    one side and not the other is caught. Repeating it here means the same
    mismatch is named by the build rather than surfacing as a Rust load error
    with no indication of which side moved.
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


def row_from_tsv(cells: list[str], prov: list[list[str]]) -> Row:
    """Rebuild a `Row` from the bytes on disk, so its digest can be recomputed.

    `description` and `notes` are unescaped, because both implementations of the
    digest hash the unescaped canonical value — see `Db::content_digest`. The
    alternative, hashing the on-disk bytes, is fragile:
    `esc(unesc(x)) != x` for an unrecognised escape such as `\\q`.
    """
    rid = cells[ID]
    return Row(
        id=rid,
        name=cells[FEATURE_COLUMNS.index("name")],
        # Through the same canonicaliser the build and `Db::parse` use, so a
        # hand-edited cell with a stray space round an alias hashes here exactly
        # as it will in the shipped binary.
        aliases=canon_alias_list(cells[FEATURE_COLUMNS.index("aliases")].split("|")),
        cls=cells[FEATURE_COLUMNS.index("class")],
        genbank_key=cells[FEATURE_COLUMNS.index("genbank_key")],
        reference_nt=cells[NT],
        reference_aa=cells[AA],
        boundary_rule=cells[FEATURE_COLUMNS.index("boundary_rule")],
        boundary_evidence=cells[FEATURE_COLUMNS.index("boundary_evidence")],
        description=unesc(cells[DESCRIPTION]),
        notes=unesc(cells[FEATURE_COLUMNS.index("notes")]),
        patent_flag=cells[FEATURE_COLUMNS.index("patent_flag")],
        provenance=[p for p in prov if p[0] == rid],
    )


def violations(features_text: str, provenance_text: str, signoff_text: str) -> list[str]:
    """Every way the shipped tables could assert something nobody signed.

    Returns a list of problems; empty means clean. Never raises on bad input —
    a parse failure is itself reported as a violation, because "the file could
    not be read" must not be able to masquerade as "the file is fine".
    """
    rows, problems = data_rows(features_text, FEATURE_COLUMNS, "features.tsv")
    prov, prov_problems = data_rows(provenance_text, PROVENANCE_COLUMNS, "provenance.tsv")
    problems += prov_problems

    # A blank or absent sign-off table is the safe degenerate case and not a
    # problem: "nothing is signed" is a legitimate state. A table that is
    # present but malformed IS one, and yields no signatures, so every row must
    # then be 'proposed'.
    signed: dict[str, list[str]] = {}
    if signoff_text.strip():
        sign_rows, sign_problems = data_rows(signoff_text, SIGNOFF_COLUMNS, "SIGNOFF.tsv")
        problems += sign_problems
        if not sign_problems:
            doubled: set = set()
            for s in sign_rows:
                # Dropped, not last-wins: a file that says two things about one
                # record says nothing about it, which is the same downgrade
                # every other unreadable state resolves to. build.py's
                # read_signoff() and the Rust parse_signoff() do the same.
                if s[S_ID] in signed or s[S_ID] in doubled:
                    problems.append(
                        f"SIGNOFF.tsv: {s[S_ID]} is signed twice, so neither line is "
                        f"applied; the file holds one current signature per record"
                    )
                    doubled.add(s[S_ID])
                    signed.pop(s[S_ID], None)
                    continue
                signed[s[S_ID]] = s

    # An empty table would satisfy every per-row rule below vacuously. That is
    # the shape of failure this whole file is written against, so it is a
    # violation in its own right rather than a quiet pass.
    if not rows:
        problems.append(
            "features.tsv holds no data rows, so every per-row check below would "
            "pass without examining anything"
        )
        return problems

    ids = [r[ID] for r in rows]
    for rid in sorted(set(signed) - set(ids)):
        problems.append(
            f"SIGNOFF.tsv signs {rid}, which is not a row of features.tsv. A "
            f"signature pointing at nothing is a silent lie about coverage."
        )

    for r in rows:
        rid = r[ID] or "(unnamed row)"
        status = r[STATUS].strip().lower()
        curator = r[CURATOR].strip()
        s = signed.get(r[ID])

        # The loader refuses a row whose patent_flag it cannot read, rather than
        # reading it as 0, so such a row has no digest and no signature can be
        # checked against it. Reported here for the same reason: a hand-edited
        # `Y` in that column used to hash as 0 on this side and 1 in Rust, so CI
        # stayed green while the shipped binary dropped the approval and the
        # patent claim flipped.
        if parse_flag(r[PATENT]) is None:
            problems.append(
                f"{rid}: patent_flag {r[PATENT]!r} is not a boolean, so the loader "
                f"refuses this row outright and no signature on it means anything"
            )
            continue

        if s is None:
            if status != "proposed":
                problems.append(
                    f"{rid}: review_status is {r[STATUS]!r} but features/SIGNOFF.tsv "
                    f"does not name it. Only a named human may move a row past "
                    f"'proposed', and the only way to say so is a line in that file."
                )
            if curator:
                problems.append(
                    f"{rid}: curator is {curator!r} with no sign-off line. A "
                    f"machine-assembled row carries no signature."
                )
            continue

        if status != s[S_STATUS].strip().lower():
            problems.append(
                f"{rid}: features.tsv says review_status {r[STATUS]!r} but "
                f"SIGNOFF.tsv says {s[S_STATUS]!r}"
            )
        if curator != s[S_CURATOR].strip():
            problems.append(
                f"{rid}: features.tsv names curator {curator!r} but SIGNOFF.tsv "
                f"names {s[S_CURATOR].strip()!r}"
            )
        now = content_digest(row_from_tsv(r, prov))
        if now != s[S_DIGEST].strip().lower():
            problems.append(
                f"{rid}: the row has changed since it was signed. Recorded "
                f"{s[S_DIGEST].strip()}, recomputed {now}. The approval has lapsed; "
                f"re-read the row and replace its line in features/SIGNOFF.tsv."
            )

    dupes = sorted({i for i in ids if ids.count(i) > 1})
    if dupes:
        problems.append(f"duplicate record id(s): {dupes}. A PLF id is a permanent name.")

    # The other promise the file makes: every sequence can say where it came
    # from. Which column holds "the sequence" now depends on the row — a
    # peptide-only synthetic part has residues and no bases, so demanding
    # reference_nt provenance for it would demand a source for a field that is
    # deliberately empty.
    by_field: dict[str, set[str]] = {}
    for p in prov:
        by_field.setdefault(p[1], set()).add(p[0])
    for r in rows:
        field = "reference_aa" if not r[NT] else "reference_nt"
        if r[ID] not in by_field.get(field, set()):
            problems.append(f"{r[ID]}: no {field} provenance")
        if not r[NT] and not r[AA]:
            problems.append(
                f"{r[ID]}: neither a nucleotide nor a protein reference, so nothing "
                f"in either index could ever match it"
            )

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
    # PlasMapper were appended to a copy of the shipped table by hand, and the
    # predecessor of this program exited 0, counted them, and objected to
    # neither.
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


def unsign(features_text: str) -> str:
    """The real table with every signature stripped: `proposed`, no curator.

    The baseline every negative control below is planted into, and the fix for a
    program that only worked while the repository had no signatures at all.

    Both faults were the same mistake in two places. `plant()` writes into data
    row 0 unconditionally, so signing PLF:0001 made "flip review_status to
    reviewed" a no-op and the exactly-one-cell guard reported zero changed cells
    and refused to certify. And `sign()` returns a sign-off table holding ONLY
    its fixture line, so running it against the REAL features.tsv made every
    genuinely-signed row read as reviewed-with-no-sign-off-line. Between them
    they covered every possible signature: the moment the PI signed anything, CI
    went red and accused a correctly signed row of being a violation.

    Stripping first makes the fixture the only signature in play, which is what
    each control needs in order to isolate one thing. It is still the REAL text
    -- same parse path, same column offsets, same row count -- which is the
    property `plant`'s docstring cares about.
    """
    lines, header_seen, out = features_text.splitlines(), False, []
    for line in lines:
        if not line.strip() or line.startswith("#") or not header_seen:
            header_seen = header_seen or (line.strip() and not line.startswith("#"))
            out.append(line)
            continue
        cells = line.split("\t")
        if len(cells) == len(FEATURE_COLUMNS):
            cells[STATUS], cells[CURATOR] = "proposed", ""
        out.append("\t".join(cells))
    return "\n".join(out) + "\n"


def plant(features_text: str, column: int, value: str, which: int = 0) -> str:
    """Return the table with `value` written into `column` of data row `which`.

    Used only by the negative controls. Operates on the real shipped text rather
    than on a fixture so that a control exercises the same parse path, the same
    column offsets and the same row shape as the real check — a control that
    runs against a hand-built two-line fixture can pass while the real parse
    silently returns nothing.
    """
    lines = features_text.splitlines()
    header_seen, seen = False, 0
    for i, line in enumerate(lines):
        if not line.strip() or line.startswith("#"):
            continue
        if not header_seen:
            header_seen = True
            continue
        cells = line.split("\t")
        if len(cells) != len(FEATURE_COLUMNS):
            continue
        if seen != which:
            seen += 1
            continue
        cells[column] = value
        # Rebuild by index. An earlier version tracked position by the length of
        # the output list and was off by one, so it planted the violation *and*
        # deleted the row after it. The control still passed — it was catching
        # the violation it planted — while quietly testing a table one row short
        # of the real one. Slicing the original list makes the edit provably
        # local, which `main()` then asserts.
        return "\n".join(lines[:i] + ["\t".join(cells)] + lines[i + 1:]) + "\n"
    raise SystemExit("could not plant a violation: the table has no such data row")


def sign(features_text: str, provenance_text: str, rid: str, curator: str) -> str:
    """A SIGNOFF.tsv that really does sign `rid` as the file stands.

    The controls need a *valid* signature before they can show that breaking the
    row invalidates it, and a valid digest cannot be written by hand — that is
    the whole point of the digest. So it is computed the way a curator computes
    it: read the row, hash it.
    """
    rows, _ = data_rows(features_text, FEATURE_COLUMNS, "features.tsv")
    prov, _ = data_rows(provenance_text, PROVENANCE_COLUMNS, "provenance.tsv")
    row = next(r for r in rows if r[ID] == rid)
    digest = content_digest(row_from_tsv(row, prov))
    return (
        "\t".join(SIGNOFF_COLUMNS) + "\n"
        + f"{rid}\treviewed\t{curator}\t2026-07-28\t{digest}\tcontrol fixture\n"
    )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dir", default=str(ROOT), help="directory holding the tables")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    d = Path(args.dir)
    features_text = (d / "features.tsv").read_text(encoding="utf8")
    provenance_text = (d / "provenance.tsv").read_text(encoding="utf8")
    signoff_path = d / "SIGNOFF.tsv"
    signoff_text = signoff_path.read_text(encoding="utf8") if signoff_path.exists() else ""

    real_rows, _ = data_rows(features_text, FEATURE_COLUMNS, "features.tsv")
    if not real_rows:
        print("features.tsv has no data rows; there is nothing to certify.")
        return 2

    # The controls run against the real table with its signatures stripped, so
    # the fixture signature is the only one in play and each control isolates
    # one thing. See unsign(). The real check at the bottom uses the real files.
    base = unsign(features_text)
    base_rows, base_problems = data_rows(base, FEATURE_COLUMNS, "features.tsv")
    if base_problems or len(base_rows) != len(real_rows):
        print(f"CONTROL IS INVALID: stripping signatures damaged the table "
              f"({len(real_rows)} rows -> {len(base_rows)}): {base_problems}")
        return 2
    if any(r[STATUS].strip().lower() != "proposed" or r[CURATOR].strip() for r in base_rows):
        print("CONTROL IS INVALID: stripping signatures left a signed row behind.")
        return 2
    first = base_rows[0][ID]
    # A row other than the first, so a control that only ever plants at
    # position 0 cannot be mistaken for one that checks the whole table.
    last = base_rows[-1][ID]

    def caught(label: str, f: str, p: str, s: str) -> bool:
        if not violations(f, p, s):
            print(f"CHECK IS WORTHLESS: planted {label} and violations() reported nothing.")
            print("Refusing to certify the real file with a verifier that cannot fail.")
            return False
        if not args.quiet:
            print(f"  control  {label}: caught")
        return True

    # --- the negative controls, first ------------------------------------
    # Run before the real check, deliberately. If the check is broken, the run
    # should stop here rather than print a green result and then explain that
    # the green result means nothing.
    for label, column, value in (
        ("review_status flipped to 'reviewed' with no sign-off line", STATUS, "reviewed"),
        ("a curator name written onto an unsigned row", CURATOR, "L. Lobel"),
    ):
        planted = plant(base, column, value)
        # The control must differ from its baseline in exactly one cell. A
        # planter that also drops or duplicates rows is testing a table that is
        # not the one being certified, and it will still appear to "catch" the
        # violation it planted.
        planted_rows, _ = data_rows(planted, FEATURE_COLUMNS, "features.tsv")
        differing = [
            (a[ID], i) for a, b in zip(base_rows, planted_rows)
            for i in range(len(FEATURE_COLUMNS)) if a[i] != b[i]
        ]
        if len(planted_rows) != len(base_rows) or len(differing) != 1:
            print(f"CONTROL IS INVALID: planting {label} changed "
                  f"{len(base_rows)} rows -> {len(planted_rows)} rows and "
                  f"{len(differing)} cell(s); it must change exactly one cell.")
            return 2
        if not caught(label, planted, provenance_text, ""):
            return 2

    # The sign-off controls proper. A genuinely signed row is constructed first,
    # so each control shows a real approval being invalidated rather than an
    # absent one failing to appear.
    good_status = plant(base, STATUS, "reviewed")
    good = plant(good_status, CURATOR, "L. Lobel")
    good_sign = sign(base, provenance_text, first, "L. Lobel")
    if violations(good, provenance_text, good_sign):
        print("CONTROL IS INVALID: a correctly signed row was reported as a violation:")
        for p in violations(good, provenance_text, good_sign):
            print(f"    {p}")
        return 2
    if not args.quiet:
        print("  control  a correctly signed row: accepted (so the checks below can fail)")

    # ...and the same at a row that is NOT the first. Everything above plants at
    # position 0, so without this a verifier keyed on position would pass every
    # control and still refuse the one state the PI actually authorised: a
    # signature on a row somewhere in the middle of the table.
    n = len(base_rows) - 1
    last_good = plant(plant(base, STATUS, "reviewed", which=n), CURATOR, "L. Lobel", which=n)
    last_sign = sign(base, provenance_text, last, "L. Lobel")
    if violations(last_good, provenance_text, last_sign):
        print(f"CONTROL IS INVALID: a correct signature on {last} (row {n + 1} of "
              f"{len(base_rows)}) was reported as a violation:")
        for p in violations(last_good, provenance_text, last_sign):
            print(f"    {p}")
        return 2
    if not args.quiet:
        print(f"  control  a correctly signed row that is not the first ({last}): accepted")

    if not caught("a curator differing from the sign-off's",
                  plant(good_status, CURATOR, "Somebody Else"), provenance_text, good_sign):
        return 2

    # One base of a signed row's sequence. The make-or-break case: this is what
    # a signature is for, and the id-stability audit in build.py cannot catch it
    # on a fresh clone, where there is no previous features.tsv to compare with.
    seq = next(r for r in base_rows if r[ID] == first)[NT]
    flipped = ("A" + seq[1:]) if seq[:1] != "A" else ("C" + seq[1:])
    if seq:
        if not caught("one base changed in a signed row",
                      plant(good, NT, flipped), provenance_text, good_sign):
            return 2

    if not caught("one word changed in a signed row's description",
                  plant(good, DESCRIPTION,
                        next(r for r in base_rows if r[ID] == first)[DESCRIPTION] + " Extra."),
                  provenance_text, good_sign):
        return 2

    if not caught("a sign-off naming a record that does not exist",
                  base, provenance_text,
                  sign(base, provenance_text, first, "L. Lobel")
                  .replace(first, "PLF:9999")):
        return 2

    # A record signed twice yields NOTHING for that record, so the row must be
    # `proposed` -- and `good` says `reviewed`, so the doubled file is caught.
    # The invariant every docstring states is that a malformed sign-off can only
    # ever remove trust; last-wins would have granted it.
    doubled = good_sign.rstrip("\n") + "\n" + good_sign.splitlines()[1] + "\n"
    if not caught("a record signed twice", good, provenance_text, doubled):
        return 2

    if not caught("an empty table", "", "", ""):
        return 2

    # The exact two rows that were appended to a copy of the shipped table by
    # hand, which the predecessor of this program counted, accepted and
    # certified green. The NO_GO list in features/SOURCING.md was author
    # discipline and nothing else; discipline is not a control, and a control
    # nobody plants is not a control either. Also a phantom `field`, which
    # attributes nothing and did so silently on 40 shipped rows.
    tab = "\t"
    for label, cells in (
        ("an Addgene-sourced provenance row",
         [first, "description", "addgene", "Addgene-52961",
          "noncommercial-informational-only", "https://www.addgene.org/52961/",
          "2026-07-28", "deadbeef"]),
        ("a PlasMapper-sourced provenance row",
         [first, "reference_nt", "plasmapper", "FeatureDB-scraped-from-Addgene",
          "GPL-3.0-scraped", "https://plasmapper.ca/featuredb",
          "2026-07-28", "cafebabe"]),
        ("a provenance row keyed on a field that is not a column",
         [first, "citation", "rfam", "RF00000", "CC0-1.0",
          "https://ftp.ebi.ac.uk/pub/databases/Rfam/CURRENT/Rfam.seed.gz",
          "2026-07-28", "beef"]),
    ):
        tainted = provenance_text.rstrip("\n") + "\n" + tab.join(cells) + "\n"
        if not caught(label, base, tainted, ""):
            return 2

    # A hand-edited patent_flag. Reachable only by hand -- the build normalises
    # and validate_row refuses anything but 0 or 1 -- which is exactly the
    # threat model. Two spellings, one of each truth value, because the old
    # case-sensitive membership test was wrong in BOTH directions: `TRUE` on a 1
    # row hashed as 0 here and 1 in Rust (CI red, loader fine), `Y` on a 0 row
    # hashed as 0 here and 1 in Rust (CI green, loader dropped the approval and
    # the patent claim flipped).
    same_claim = (
        ("TRUE", "Yes", "T") if parse_flag(next(r for r in base_rows if r[ID] == first)[PATENT])
        else ("FALSE", "No", "N")
    )
    for spelling in same_claim:
        respelled = plant(good, PATENT, spelling)
        still = violations(respelled, provenance_text, good_sign)
        if still:
            print(f"CHECK IS WORTHLESS IN THE OTHER DIRECTION: re-spelling patent_flag as")
            print(f"{spelling!r} is not a change to the claim, and it invalidated the")
            print("signature. The two implementations of the digest have diverged again.")
            for p in still:
                print(f"    {p}")
            return 2
    if not args.quiet:
        print(f"  control  patent_flag re-spelled {'/'.join(same_claim)}: "
              f"correctly NOT a violation")

    # An alias with a stray space. `Db::parse` trims aliases before the Rust
    # digest sees them, so this side must too -- otherwise a one-character
    # curation typo produces a signature that verifies here and lapses in the
    # shipped binary, which is the worst shape of failure the scheme has.
    aliased = plant(good, FEATURE_COLUMNS.index("aliases"),
                    " " + next(r for r in base_rows if r[ID] == first)[
                        FEATURE_COLUMNS.index("aliases")] + " |")
    still = violations(aliased, provenance_text, good_sign)
    if still:
        print("CHECK IS WORTHLESS IN THE OTHER DIRECTION: padding an alias with spaces")
        print("invalidated the signature here, while Db::parse trims it away and keeps")
        print("the signature. The two implementations of the digest have diverged.")
        for p in still:
            print(f"    {p}")
        return 2
    if not args.quiet:
        print("  control  an alias padded with spaces: correctly NOT a violation")

    # --- THE INVERTED CONTROL --------------------------------------------
    # The one most likely to be skipped, and the most valuable. `date_added` is
    # stamped from the clock on every row on every run, so if the digest covered
    # it, every signature in the repository would lapse on every build — and a
    # check that fires on everything is exactly as worthless as one that fires
    # on nothing.
    restamped = plant(good, DATE_ADDED, "2099-01-01")
    still = violations(restamped, provenance_text, good_sign)
    if still:
        print("CHECK IS WORTHLESS IN THE OTHER DIRECTION: changing only date_added on a")
        print("signed row invalidated its signature, so every build would lapse every")
        print("sign-off in the repository. The digest must not cover the build clock.")
        for p in still:
            print(f"    {p}")
        return 2
    if not args.quiet:
        print("  control  date_added changed on a signed row: correctly NOT a violation")

    # --- the real check ---------------------------------------------------
    problems = violations(features_text, provenance_text, signoff_text)
    if problems:
        print(f"\n{len(problems)} violation(s) in {d}:")
        for p in problems:
            print(f"  - {p}")
        return 1

    rows, _ = data_rows(features_text, FEATURE_COLUMNS, "features.tsv")
    prov, _ = data_rows(provenance_text, PROVENANCE_COLUMNS, "provenance.tsv")
    sign_rows, _ = (
        data_rows(signoff_text, SIGNOFF_COLUMNS, "SIGNOFF.tsv") if signoff_text.strip() else ([], [])
    )
    n_signed = sum(1 for r in rows if r[STATUS].strip().lower() != "proposed")
    print(
        f"\n{len(rows)} row(s), {len(prov)} provenance row(s), {len(sign_rows)} "
        f"signature(s) on file."
    )
    if n_signed == 0:
        print("Every row is 'proposed' with an empty curator, so Db::reviewed() yields")
        print("zero records. Nothing here asserts anything.")
    else:
        print(f"{n_signed} row(s) carry a curator sign-off whose content digest still")
        print(f"matches; the other {len(rows) - n_signed} are 'proposed'.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
