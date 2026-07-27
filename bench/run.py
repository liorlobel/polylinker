"""Score any tool against polylinker-bench.

The bench is data, not a test suite for one program. A tool is scored through a
small adapter that answers cases; writing one for SnapGene, Benchling, UGENE or
pydna is the same twenty lines as the one for Polylinker.

The adapter contract, kept deliberately small so the barrier is low:

    <adapter> --capabilities
        prints one operation name per line -- the ones it will attempt

    <adapter>
        reads   id \\t operation \\t topology \\t sequence [\\t key=value]...
        writes  id \\t key=value...          answers
        or      id \\t unsupported           declines the case

Anything the tool declines, or does not list as a capability, is reported as
UNSUPPORTED rather than quietly dropped. A pass rate computed only over the
cases a tool chose to attempt is not a pass rate.

Usage:
    python bench/run.py bench/polylinker-bench.json -- target/release/pl.exe bench-adapter
    python bench/run.py bench/polylinker-bench.json --verbose -- pl bench-adapter
"""

import json
import subprocess
import sys


def load(path):
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def capabilities(cmd):
    """Ask the adapter what it supports.

    An adapter that cannot even be launched is a broken invocation, not a tool
    with no capabilities -- and the difference matters, because "declined" cases
    stay in the denominator and are scored as 0%. Reporting `all 0 0 176 176
    0.0%` for a typo in the command line looks exactly like a catastrophic
    regression, and on Windows it is easy to trigger: `subprocess` will not
    resolve a relative executable path written with forward slashes, so
    a relative `target/release/pl.exe` fails where a backslash path works.
    Fail loudly instead.
    """
    try:
        r = subprocess.run(cmd + ["--capabilities"], capture_output=True,
                           text=True, encoding="utf-8", timeout=60)
    except FileNotFoundError:
        sys.exit(
            f"cannot run the adapter: {cmd[0]!r} not found. "
            "On Windows, use a backslash or an absolute path -- subprocess does "
            "not resolve a relative executable path written with '/'."
        )
    except Exception as e:
        sys.exit(f"cannot run the adapter {cmd!r}: {e}")
    if r.returncode != 0:
        sys.exit(
            f"adapter --capabilities failed (exit {r.returncode}): {r.stderr[:2000]}"
        )
    caps = {l.strip() for l in r.stdout.splitlines() if l.strip()}
    if not caps:
        sys.exit("adapter declared no capabilities at all; nothing would be scored")
    return caps


def encode(case):
    """One case as a tab-separated line, or None if it cannot be expressed.

    Multi-fragment operations put the fragments in the sequence column, joined
    by commas. The column is positional so it cannot simply be omitted, and a
    separate `fragments=` parameter would mean two ways to say where the input
    is. Commas cannot occur in a sequence, so the encoding is unambiguous.
    """
    inp = case["input"]
    seq = inp.get("sequence")
    if seq is None:
        frags = inp.get("fragments")
        if not frags:
            return None
        seq = ",".join(frags)
    fields = [case["id"], case["operation"], inp.get("topology", "linear"), seq]
    for k, v in sorted(case.get("params", {}).items()):
        fields.append(f"{k}={v}")
    return "\t".join(fields)


def ask(cmd, lines):
    r = subprocess.run(cmd, input="\n".join(lines) + "\n", capture_output=True,
                       text=True, encoding="utf-8", timeout=600)
    if r.returncode != 0:
        sys.exit(f"adapter failed (exit {r.returncode}):\n{r.stderr[:3000]}")
    answers = {}
    for line in r.stdout.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t")
        cid = parts[0]
        if len(parts) > 1 and parts[1] == "unsupported":
            answers[cid] = None
            continue
        answers[cid] = dict(p.split("=", 1) for p in parts[1:] if "=" in p)
    return answers


def compare(expect, answer):
    """Return a list of (key, expected, got) for every disagreement."""
    bad = []
    for key, want in expect.items():
        got = answer.get(key)
        if got is None:
            bad.append((key, want, "<missing>"))
            continue
        if key == "cut_positions":
            got_list = [int(x) for x in got.split(",") if x] if got else []
            if got_list != list(want):
                bad.append((key, want, got_list))
        elif isinstance(want, int):
            try:
                if int(got) != want:
                    bad.append((key, want, int(got)))
            except ValueError:
                bad.append((key, want, got))
        else:
            if str(got) != str(want):
                bad.append((key, want, got))
    return bad


def main():
    argv = sys.argv[1:]
    if "--" not in argv:
        sys.exit(__doc__)
    split = argv.index("--")
    opts, cmd = argv[:split], argv[split + 1:]
    verbose = "--verbose" in opts
    paths = [o for o in opts if not o.startswith("-")]
    if not paths or not cmd:
        sys.exit(__doc__)

    doc = load(paths[0])
    cases = doc["cases"]
    caps = capabilities(cmd)
    print(f"{doc['name']} {doc['version']} ({doc['licence']}) — {len(cases)} cases")
    print(f"tool: {' '.join(cmd)}")
    print(f"declares support for: {', '.join(sorted(caps)) or '(nothing)'}\n")

    askable, unsupported = [], []
    for c in cases:
        line = encode(c)
        if c["operation"] not in caps or line is None:
            unsupported.append(c)
        else:
            askable.append((c, line))

    answers = ask(cmd, [l for _, l in askable]) if askable else {}

    passed, failed = [], []
    for c, _ in askable:
        a = answers.get(c["id"])
        if a is None:
            unsupported.append(c)
            continue
        bad = compare(c["expect"], a)
        (failed if bad else passed).append((c, bad))

    # Scorecard, broken down so a weak area cannot hide behind a strong one.
    ops = sorted({c["operation"] for c in cases})
    print(f"{'operation':<12} {'pass':>6} {'fail':>6} {'unsup':>6} {'total':>6}   rate")
    print("-" * 56)
    for op in ops:
        p = sum(1 for c, _ in passed if c["operation"] == op)
        f = sum(1 for c, _ in failed if c["operation"] == op)
        u = sum(1 for c in unsupported if c["operation"] == op)
        t = p + f + u
        rate = f"{100.0 * p / t:5.1f}%" if t else "    -"
        print(f"{op:<12} {p:>6} {f:>6} {u:>6} {t:>6}   {rate}")
    print("-" * 56)
    tp, tf, tu = len(passed), len(failed), len(unsupported)
    total = tp + tf + tu
    print(f"{'all':<12} {tp:>6} {tf:>6} {tu:>6} {total:>6}   {100.0 * tp / total:5.1f}%")

    # Tier 1 is the class that costs bench time when it is wrong.
    t1 = [(c, b) for c, b in passed + failed if c["hazard_tier"] == 1]
    t1p = sum(1 for c, b in t1 if not b)
    t1u = sum(1 for c in unsupported if c["hazard_tier"] == 1)
    print(f"\nhazard tier 1 (silent and expensive): {t1p} passed, "
          f"{len(t1) - t1p} failed, {t1u} unsupported")

    if failed:
        print(f"\n{'=' * 56}\nFAILURES\n{'=' * 56}")
        for c, bad in failed[: (len(failed) if verbose else 12)]:
            print(f"\n{c['id']}  (tier {c['hazard_tier']})")
            print(f"  {c['description']}")
            print(f"  oracle: {c['oracle']['tool']} {c['oracle']['version']}")
            for key, want, got in bad:
                print(f"    {key}:")
                print(f"      expected {want}")
                print(f"      got      {got}")
        if not verbose and len(failed) > 12:
            print(f"\n  ...and {len(failed) - 12} more (pass --verbose)")

    if unsupported:
        by_op = {}
        for c in unsupported:
            by_op[c["operation"]] = by_op.get(c["operation"], 0) + 1
        print("\nunsupported: " + ", ".join(f"{k} {v}" for k, v in sorted(by_op.items())))

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
