#!/usr/bin/env python3
"""Every stage must say what it does about SnapGene arriving through INSDC.

WHAT THIS IS NOT
----------------

**This is not a taint check, and it cannot become one.** It cannot tell you that
a coordinate in `features.tsv` came from SnapGene. Nothing in this repository
can, and the reason is worth stating once, here, in the file that would be the
natural place to put such a check:

  * SnapGene's *prose* is copyrightable expression and `taint_gate.py` measures
    it. That leg works.
  * SnapGene's *boundary convention* -- where they decided "the CMV promoter"
    starts and stops -- is what arrives through INSDC, because ENA folds a
    submitter's SnapGene `/label` into the `/note` and an ordinary depositor who
    annotated their plasmid in SnapGene publishes that convention inside a
    record `features/SOURCING.md` has cleared as a source.
  * A check for that convention would need SnapGene's extents to compare
    against. The artifact `taint_gate.py` is pinned to -- pLannotate's
    `snapgene.csv`, columns `sseqid, Feature, Type, Description` -- **contains no
    coordinate and no sequence**, so it cannot supply one. SnapGene's feature
    bases live in pLannotate's `BLAST_dbs.tar.gz`, a release asset on a host
    `build.ALLOWED_FETCH_HOSTS` refuses, carrying no licence, and acquiring a
    bulk copy of their extents in order to prove we did not copy their extents
    is a larger act of copying than the one being disproved.
  * And the sequences are biology. The T7 promoter is the T7 promoter. An exact
    match to anything in their file proves nothing whatever about copying, so a
    check keyed on sequence agreement would fire on almost every legitimate row
    and be switched off inside a release -- 90.9% of a 481-record INSDC sample
    holds at least one extent that a SnapGene-annotated record also holds, and
    the rule "fewer than two independent submissions annotate this exact extent"
    fires on 46 of the 55 distinct extents in that sample (84%).

So the honest thing available is **structural, not statistical**: every stage
that could take a coordinate off an INSDC record has to declare what it does
about this route, in a fixed vocabulary, and this gate refuses a stage that
declares nothing. It does not prove a row is clean. It proves that no stage
reached the table without somebody answering the question.

WHAT IT ACTUALLY CHECKS, and what each check can catch
-----------------------------------------------------

Presence is the weakest half and it is still worth having, because the failure
it stops is the one that happens: a new stage module whose author never knew the
question existed. `build.load_stage` already refuses a stage that does not
declare its id block, for the same reason and after the same near-miss.

The rest is checked rather than believed, and each of these can fail on real
input:

  * `no_insdc` -- the module names no INSDC host. Add an ENA fetch to
    `stage_curated.py` without changing its declaration and this goes red.
  * `no_feature_table` -- the module names no INSDC *flat-file* endpoint. A
    feature table is only served by the flat-file view, so a stage that fetches
    nothing but `/fasta/` cannot read a depositor's annotation. Point
    `stage_rfam.py` at `/embl/` and this goes red.
  * `feature_table_forced` -- the stage takes an extent off a depositor's
    feature table, but an independent test forces that extent, so agreement with
    anybody's convention is explained by the test. The declaration must NAME
    that test, and this gate DRIVES it: a CDS that translates to its protein
    must pass, one with a single substitution must fail. Neuter the check and
    this goes red.
  * `feature_table_convention` -- the stage ships an extent nothing forces. This
    is the exposed case. The declaration must name a screen for the SnapGene
    tell, which this gate drives against a record carrying it and a record
    without it, and must name the constant holding how many independent
    submissions have to place the feature at exactly the shipped extent, which
    this gate requires to be at least 2. Weaken either and this goes red.

The limits, stated rather than left to be discovered. String constants are read
out of the module's AST, so a URL assembled from fragments defeats the endpoint
scan; docstrings are skipped, so a URL in prose does not trip it. This is a
guard against the accident, not against an author who means to get round it --
and the declaration itself is a sentence a human wrote, which no program can
audit for truth. What the gate buys is that the sentence exists, that it names a
posture from a closed vocabulary, and that the mechanical parts of that posture
hold.

Usage
-----
    python features/build/insdc_posture.py             # check every stage
    python features/build/insdc_posture.py --self-test # prove the gate can fail
"""

from __future__ import annotations

import argparse
import ast
import importlib
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

import build as buildmod  # noqa: E402

DECLARATION = "INSDC_POSTURE"

# What each posture asserts, in the words a stage author has to be able to say
# truthfully about their own module. The keys are the closed vocabulary; a
# declaration naming anything else is refused, because "choose one of four" is
# what makes a declaration a decision rather than a free-text field.
POSTURES = {
    "no_insdc": (
        "This stage fetches nothing from an INSDC host, so no depositor's "
        "annotation can reach it at all."
    ),
    "no_feature_table": (
        "This stage reads INSDC sequence but never a record's feature table. "
        "Every extent it ships comes from somewhere a depositor's SnapGene "
        "session cannot reach -- an open reading frame it computed, or an "
        "alignment from another source."
    ),
    "feature_table_forced": (
        "This stage takes an extent off a depositor's feature table, and an "
        "independent test forces that extent. Agreement with any vendor's "
        "convention is explained by the test rather than by the depositor, and "
        "the test is named in the declaration and driven by this gate."
    ),
    "feature_table_convention": (
        "This stage ships an extent that nothing forces -- a convention it "
        "chose. This is the case exposed to SnapGene arriving through INSDC. "
        "The declaration must name a screen for the SnapGene tell and the "
        "constant setting how many independent submissions must place the "
        "feature at exactly the shipped extent."
    ),
}

REQUIRED_KEYS = {
    "no_insdc": ("posture", "reason"),
    "no_feature_table": ("posture", "reason"),
    "feature_table_forced": ("posture", "reason", "forced_by"),
    "feature_table_convention": ("posture", "reason", "screen", "corroboration"),
}

# Hosts that serve INSDC or INSDC-derived records. A subset of
# build.ALLOWED_FETCH_HOSTS by construction, and asserted to be one in
# `self_test()` so that a host renamed there cannot leave a dead string here.
INSDC_HOSTS = (
    "www.ebi.ac.uk",
    "eutils.ncbi.nlm.nih.gov",
    "ftp.ncbi.nlm.nih.gov",
)

# Endpoints that serve a whole record, feature table included. `/fasta/` is
# deliberately absent: it serves bases and nothing else, which is exactly the
# distinction `no_feature_table` turns on.
FLAT_FILE_MARKERS = ("/embl/", "/genbank/", "rettype=gb", "efetch")

MIN_REASON = 80
"""How long a `reason` has to be. A floor, not a quality bar: it stops `"n/a"`
and it stops nothing else. The uniqueness rule below is the half that stops the
likelier evasion, which is not a short reason but the same reason four times."""

MIN_CORROBORATION = 2
"""The floor `feature_table_convention` may not go under, from SOURCING.md §4:
">=2 independent GenBank exemplars showing where depositors actually place it".
A stage that lowered its own constant to make a row pass is the failure this
number exists to catch, so the number lives HERE, in the gate, and not only in
the stage that has an interest in it."""


# --------------------------------------------------------------------------
# Gathering: what the module says, without running it


def string_constants(path: Path) -> set:
    """Every string constant in a module's AST, docstrings excluded.

    Docstrings are excluded because this file's neighbours describe their own
    URLs in prose -- `stage_uniprot.py`'s header names the ENA FASTA endpoint --
    and a scan that read prose would report the description as the behaviour.
    That cuts the other way too, and the module docstring says so: a URL hidden
    in a docstring is not code and does not fetch anything, but a URL assembled
    from fragments is code and this does not see it.
    """
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    skip = set()
    for node in ast.walk(tree):
        if isinstance(node, (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
            body = getattr(node, "body", None)
            if body and isinstance(body[0], ast.Expr) and isinstance(body[0].value, ast.Constant):
                if isinstance(body[0].value.value, str):
                    skip.add(id(body[0].value))
        # An `x = "..."` immediately followed by a bare string is the attribute
        # docstring idiom this tree uses heavily (see SUBMITTER_MERGE); those
        # are prose too.
        if isinstance(node, (ast.Module, ast.ClassDef, ast.FunctionDef)):
            body = getattr(node, "body", None) or []
            for prev, cur in zip(body, body[1:]):
                if (
                    isinstance(prev, (ast.Assign, ast.AnnAssign))
                    and isinstance(cur, ast.Expr)
                    and isinstance(cur.value, ast.Constant)
                    and isinstance(cur.value.value, str)
                ):
                    skip.add(id(cur.value))
    out = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            if id(node) not in skip:
                out.add(node.value)
    return out


# --------------------------------------------------------------------------
# Judging: separated from gathering so that `self_test()` can drive it


# Synthetic, invented here, and nothing in either string is quoted from anybody.
# `label:` is a *format* convention -- the token ENA emits when it folds a
# submitter's tool-written label into a `/note` -- and the words either side of
# it are made up for this fixture.
TELL_RECORD = (
    "ID   XX99; SV 1; circular DNA; STD; SYN; 20 BP.\n"
    "FT   promoter        1..20\n"
    'FT                   /note="an element this fixture invented; '
    'label: fixture element"\n'
    "SQ   Sequence 20 BP;\n"
    "     acgtacgtac acgtacgtac                                        20\n"
    "//\n"
)
CLEAN_RECORD = TELL_RECORD.replace("; label: fixture element", "")

# A twelve-base CDS and the three residues it codes for, and the same CDS with
# one codon changed. Neither is anybody's data.
FORCED_CDS = "ATGAAAGGTTAA"
FORCED_AA = "MKG"
UNFORCED_AA = "MKW"


def judge(module_name: str, decl, consts: set, resolve) -> list:
    """Problems with one stage's declaration. Empty means it held.

    `resolve(name)` returns the module attribute of that name, or raises
    AttributeError. Passing it in rather than importing here is what lets
    `self_test()` drive every branch below against inputs that must trip it.
    """
    p = []
    if decl is None:
        return [
            f"{module_name} does not declare {DECLARATION}. Every stage that could take "
            f"a coordinate off an INSDC record must say what it does about SnapGene "
            f"arriving that way; choose one of {sorted(POSTURES)} and say why in a "
            f"sentence. See features/SOURCING.md section 0.6."
        ]
    if not isinstance(decl, dict):
        return [f"{module_name}: {DECLARATION} is {type(decl).__name__}, not a dict"]

    posture = decl.get("posture")
    if posture not in POSTURES:
        return [
            f"{module_name}: posture {posture!r} is not one of {sorted(POSTURES)}. "
            f"The vocabulary is closed on purpose: a free-text posture is a sentence "
            f"nobody can check."
        ]

    want = set(REQUIRED_KEYS[posture])
    got = set(decl)
    if got != want:
        missing = sorted(want - got)
        extra = sorted(got - want)
        p.append(
            f"{module_name}: posture {posture!r} needs exactly the keys {sorted(want)}"
            + (f"; missing {missing}" if missing else "")
            + (f"; unexpected {extra}" if extra else "")
        )
        if missing:
            return p

    reason = decl.get("reason")
    if not isinstance(reason, str) or len(reason.strip()) < MIN_REASON:
        p.append(
            f"{module_name}: `reason` is {len(reason.strip()) if isinstance(reason, str) else 0} "
            f"characters and must be at least {MIN_REASON}. Say what this stage does "
            f"with a depositor's coordinates, in this stage's own terms."
        )

    if posture == "no_insdc":
        hit = sorted(h for h in INSDC_HOSTS if any(h in c for c in consts))
        if hit:
            p.append(
                f"{module_name} declares `no_insdc` and names {hit}. Either the fetch is "
                f"new and the declaration is now false, or the host string is dead and "
                f"should go."
            )
    if posture in ("no_insdc", "no_feature_table"):
        hit = sorted(m for m in FLAT_FILE_MARKERS if any(m in c for c in consts))
        if hit:
            p.append(
                f"{module_name} declares `{posture}` and names the record endpoint(s) "
                f"{hit}. A flat file carries the depositor's feature table, so this "
                f"stage can now read a boundary somebody else chose -- which is a "
                f"sourcing decision, not a build change. Move it to "
                f"`feature_table_forced` or `feature_table_convention` and meet what "
                f"that posture asks for."
            )

    if posture == "feature_table_forced":
        p += _drive_forced(module_name, decl["forced_by"], resolve)
    if posture == "feature_table_convention":
        p += _drive_screen(module_name, decl["screen"], resolve)
        p += _check_corroboration(module_name, decl["corroboration"], resolve)
    return p


def _drive_forced(module_name: str, name, resolve) -> list:
    """The named test must accept a CDS that codes for its protein and refuse one
    that does not. Without this the posture is a word."""
    if not isinstance(name, str):
        return [f"{module_name}: `forced_by` must name a callable, not {name!r}"]
    try:
        fn = resolve(name)
    except AttributeError:
        return [f"{module_name}: `forced_by` names {name!r}, which the module does not define"]
    if not callable(fn):
        return [f"{module_name}: `forced_by` names {name!r}, which is not callable"]
    try:
        good = fn(FORCED_CDS, FORCED_AA)
        bad = fn(FORCED_CDS, UNFORCED_AA)
    except Exception as e:  # noqa: BLE001 -- any failure here is this gate's finding
        return [f"{module_name}: `forced_by` {name!r} raised {e.__class__.__name__}: {e}"]
    ok_good = good[0] if isinstance(good, tuple) else good
    ok_bad = bad[0] if isinstance(bad, tuple) else bad
    out = []
    if not ok_good:
        out.append(
            f"{module_name}: `forced_by` {name!r} rejected a CDS that translates to its "
            f"protein exactly, so it cannot be what forces this stage's extents"
        )
    if ok_bad:
        out.append(
            f"{module_name}: `forced_by` {name!r} ACCEPTED a CDS that differs from its "
            f"protein at one residue. A test that cannot fail does not force anything, "
            f"and this posture is the claim that it does."
        )
    return out


def _drive_screen(module_name: str, name, resolve) -> list:
    """The named screen must see the tell in a record that carries it and must not
    see it in one that does not."""
    if not isinstance(name, str):
        return [f"{module_name}: `screen` must name a callable, not {name!r}"]
    try:
        fn = resolve(name)
    except AttributeError:
        return [f"{module_name}: `screen` names {name!r}, which the module does not define"]
    if not callable(fn):
        return [f"{module_name}: `screen` names {name!r}, which is not callable"]
    try:
        hot = fn(TELL_RECORD)
        cold = fn(CLEAN_RECORD)
    except Exception as e:  # noqa: BLE001
        return [f"{module_name}: `screen` {name!r} raised {e.__class__.__name__}: {e}"]
    out = []
    if not hot:
        out.append(
            f"{module_name}: `screen` {name!r} did not see the tell in a record that "
            f"carries it. The stage is counting SnapGene-annotated deposits as "
            f"independent witnesses and nothing else in the tree would notice."
        )
    if cold:
        out.append(
            f"{module_name}: `screen` {name!r} reported the tell in a record without "
            f"one. A screen that fires on everything gets switched off, and then it "
            f"screens nothing."
        )
    return out


def _check_corroboration(module_name: str, name, resolve) -> list:
    if not isinstance(name, str):
        return [f"{module_name}: `corroboration` must name a constant, not {name!r}"]
    try:
        value = resolve(name)
    except AttributeError:
        return [
            f"{module_name}: `corroboration` names {name!r}, which the module does not define"
        ]
    if not isinstance(value, int) or isinstance(value, bool):
        return [f"{module_name}: `corroboration` {name!r} is {value!r}, not an integer"]
    if value < MIN_CORROBORATION:
        return [
            f"{module_name}: {name} = {value}, under the floor of {MIN_CORROBORATION}. "
            f"SOURCING.md section 4 asks for two independent exemplars SHOWING WHERE "
            f"DEPOSITORS ACTUALLY PLACE IT, and one submission placing a feature at our "
            f"extent is one lab's opinion, not a consensus."
        ]
    return []


# --------------------------------------------------------------------------
# The gate


def stage_modules() -> list:
    """(module name, path) for every stage `build.STAGES` reserves a block for.

    Keyed on STAGES rather than on a glob of `stage_*.py`, so that a stage added
    to the build is covered the moment it is added, and a file that is not a
    stage is not asked to declare anything it has no bearing on.
    """
    out, seen = [], set()
    for stage in buildmod.STAGES:
        name = stage.module or "build"
        if name in seen:
            continue
        seen.add(name)
        out.append((name, HERE / f"{name}.py"))
    return out


def check() -> list:
    """Every stage, judged. Returns the problems; empty means all of them held."""
    problems, reasons = [], {}
    for name, path in stage_modules():
        if not path.exists():
            problems.append(f"{name}: {path} does not exist, so nothing could be checked")
            continue
        try:
            mod = importlib.import_module(name)
        except Exception as e:  # noqa: BLE001 -- fail closed, see the module docstring
            problems.append(
                f"{name} could not be imported ({e.__class__.__name__}: {e}), so its "
                f"posture could not be checked. 'Could not check' is not 'checked and "
                f"clean'."
            )
            continue
        decl = getattr(mod, DECLARATION, None)
        consts = string_constants(path)
        found = judge(name, decl, consts, lambda n, m=mod: getattr(m, n))
        problems += found
        if isinstance(decl, dict) and isinstance(decl.get("reason"), str):
            key = " ".join(decl["reason"].split()).casefold()
            if key in reasons:
                problems.append(
                    f"{name} declares the same `reason` as {reasons[key]}, word for "
                    f"word. Four stages do four different things with a depositor's "
                    f"coordinates; one sentence cannot be true of all of them."
                )
            else:
                reasons[key] = name
        if not found:
            print(f"  ok    {name:14s} {decl['posture']}")
    return problems


def self_test() -> None:
    """Prove the gate can fail. A gate that cannot fail proves nothing.

    Every branch of `judge()` is driven with an input that must trip it, using
    fakes rather than the real stages, so these hold on a tree where every real
    declaration is correct -- which is exactly when the checks are otherwise
    unfalsifiable.
    """
    def only(problems, needle, label):
        assert problems, f"{label}: nothing was reported"
        assert any(needle in p for p in problems), f"{label}: got {problems}"

    def resolve_none(_n):
        raise AttributeError(_n)

    good_reason = "x" * MIN_REASON

    only(judge("fake", None, set(), resolve_none), "does not declare", "a missing declaration")
    only(judge("fake", "no_insdc", set(), resolve_none), "not a dict", "a declaration that is a string")
    only(
        judge("fake", {"posture": "careful", "reason": good_reason}, set(), resolve_none),
        "is not one of",
        "a posture outside the vocabulary",
    )
    only(
        judge("fake", {"posture": "no_insdc", "reason": "n/a"}, set(), resolve_none),
        "must be at least",
        "a one-word reason",
    )
    only(
        judge("fake", {"posture": "feature_table_convention", "reason": good_reason},
              set(), resolve_none),
        "needs exactly the keys",
        "a convention posture with no screen",
    )
    only(
        judge("fake", {"posture": "no_insdc", "reason": good_reason},
              {"https://www.ebi.ac.uk/ena/browser/api"}, resolve_none),
        "declares `no_insdc` and names",
        "a no_insdc stage that names an INSDC host",
    )
    only(
        judge("fake", {"posture": "no_feature_table", "reason": good_reason},
              {"https://www.ebi.ac.uk/ena/browser/api/embl/X"}, resolve_none),
        "names the record endpoint",
        "a no_feature_table stage that fetches a flat file",
    )
    # ...and the same stage fetching only FASTA is not reported, or the check
    # would fire on stage_rfam and stage_uniprot's FASTA leg and be worthless.
    assert not judge(
        "fake", {"posture": "no_feature_table", "reason": good_reason},
        {"https://www.ebi.ac.uk/ena/browser/api/fasta/X"}, resolve_none,
    ), "a FASTA-only stage was reported as reading a feature table"

    # The two driven checks, against callables that must fail them.
    fakes = {
        "accepts_anything": lambda *_a: (True, ""),
        "accepts_nothing": lambda *_a: (False, ""),
        "blind": lambda _t: False,
        "cries_wolf": lambda _t: True,
        "not_callable": 7,
    }
    resolve_fake = fakes.__getitem__

    def forced(name):
        return judge("fake", {"posture": "feature_table_forced", "reason": good_reason,
                              "forced_by": name}, set(), resolve_fake)

    only(forced("accepts_anything"), "ACCEPTED a CDS that differs", "a translation check that always passes")
    only(forced("accepts_nothing"), "rejected a CDS that translates", "a translation check that always fails")
    only(forced("not_callable"), "not callable", "a forced_by naming a number")

    def convention(screen, corr="floor_two"):
        return judge("fake", {"posture": "feature_table_convention", "reason": good_reason,
                              "screen": screen, "corroboration": corr},
                     set(), {**fakes, "floor_two": 2, "floor_one": 1}.__getitem__)

    only(convention("blind"), "did not see the tell", "a screen that sees nothing")
    only(convention("cries_wolf"), "reported the tell in a record without one", "a screen that fires on everything")
    only(convention("blind", "floor_one"), "under the floor", "a corroboration floor of one")

    # And the constants this file leans on are the ones build.py actually has.
    for host in INSDC_HOSTS:
        assert host in buildmod.ALLOWED_FETCH_HOSTS, (
            f"{host} is not in build.ALLOWED_FETCH_HOSTS; this file is scanning for a "
            f"host no stage can fetch from, which is a check that cannot fail"
        )
    assert set(REQUIRED_KEYS) == set(POSTURES), "a posture with no required-key entry"

    print(f"  PASS {DECLARATION} absent, malformed, or outside the vocabulary is refused")
    print("  PASS a declared posture contradicted by the module's own URLs is refused")
    print("  PASS a named translation test that cannot fail is refused")
    print("  PASS a named SnapGene screen that sees nothing, or everything, is refused")
    print(f"  PASS a corroboration floor under {MIN_CORROBORATION} is refused")


def main() -> int:
    ap = argparse.ArgumentParser(description="INSDC coordinate-posture declaration gate")
    ap.add_argument("--self-test", action="store_true",
                    help="prove the gate can fail, without checking the real stages")
    args = ap.parse_args()

    print("INSDC coordinate posture -- what every stage says about SnapGene via INSDC")
    print("  this is NOT a taint check; it cannot show a coordinate came from SnapGene.")
    print("  It refuses a stage that never answered the question. See the module docstring.")
    print("\nSelf-test")
    self_test()
    if args.self_test:
        return 0

    print(f"\nStages declared in build.STAGES: {len(stage_modules())}")
    problems = check()
    if problems:
        print(f"\n{len(problems)} problem(s):")
        for p in problems:
            print(f"  !! {p}")
        return 1
    print("\nEvery stage declares a posture and every mechanical part of it holds.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
