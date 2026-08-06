# Contributing to Polylinker

Thank you — genuinely. This project only works if more than one person cares
about it.

## The one unusual rule

**If you have accepted the SnapGene end-user licence agreement, please do not
contribute to file-format code** (`reference/` and `crates/pl-fileio`). Every
other part of the project is open to you.

This is not about secrecy. SnapGene's EULA §4 prohibits reverse engineering with
no "except as permitted by applicable law" carve-out, and under US precedent a
clicked EULA can waive statutory interoperability exceptions that would
otherwise apply. Keeping format work in the hands of non-licensees keeps the
provenance chain clean for everyone downstream.

Related rules, which apply to everyone:

- **Never** run a decompiler, disassembler, debugger, `strings`, Ghidra, IDA or
  Hopper against a SnapGene binary, or open its resource bundle. There is no
  technical need: the format is described by four independent open-source
  implementations and by [`docs/DNA-FORMAT.md`](docs/DNA-FORMAT.md).
- **Never** extract features, enzyme sets, colours or plasmid libraries from a
  SnapGene installation.
- **Never** contact Dotmatics or GSL Biotech for a format specification. Any
  spec would arrive under terms that contractually bind everyone downstream.
  Asking is strictly worse than not asking.
- Every new piece of format knowledge gets a row in
  [`PROVENANCE.md`](PROVENANCE.md), in the same commit as the code.

## Correctness comes before features

This is software that tells people what molecule they have made. A wrong answer
that looks right can cost someone a month of bench work, and there is a
documented case of exactly that happening because a tool hid restriction sites
behind an enzyme-set filter without saying so.

Therefore:

- **No biology routine is merged without a differential test against an
  established implementation.** Biopython, pydna and NEB's published values are
  the oracles. See `reference/python/tests/validate_digest.py` for the pattern —
  it cross-checks 5,587 cut sites across 33 real plasmids.
- **Prefer property tests over examples** where a property exists. "Rotating a
  circular sequence does not change its restriction-site set" catches a whole
  class of origin-spanning bugs that no example test will.
- **Never fail silently.** If a computation cannot be performed, or a file
  cannot be fully preserved, say so in the UI. Silence is the failure mode that
  ends this project's usefulness.

## If you change a parser, bump `ENGINE`

`pl_index::ENGINE` is the version of the *derivation*, not of the file format,
and it exists for a failure nobody catches. Every derived column in the library
index — the searchable text, the feature count, the state, the topology — is a
function of the parser, not only of the file. Teach `genbank.rs` a location form
it used to report as unrepresentable, and on the next rescan every file is
"unchanged", every row is reused, and your fix never reaches anybody's library.
`pl index --verify` will not catch it either, because the file's content hash
still matches. The user sees `3,002 unchanged (reused)`, which reads as success.

So: **touch `crates/pl-fileio/src/**`, `pl-core/src/iupac.rs` or
`pl-core/src/seguid.rs`, and bump `ENGINE` in the same commit.** A needless bump
costs one rebuild, a few seconds. A missed one costs a wrong answer that nothing
reports.

## Getting set up

```bash
cargo build --release
cargo test --workspace
```

That is the whole of it on Linux and macOS. On **Windows you need a linker** —
`rustup` does not ship one, and the failure is a confusing
`linker 'link.exe' not found` deep in a build log. `README.md`'s *Building*
section has the one-line `winget` command that installs the MSVC toolset.

Two things about the dependency graph are worth knowing before your first build.
`cargo build -p pl --release` resolves nothing outside this workspace, so the
CLI builds on a machine with no registry access at all. A full
`cargo build --workspace` does need the network, because two members take
outside crates — see the rule below.

The browser prototype is built rather than committed:
`.\tools\build-web.ps1` produces `prototype/dna-reader.html`, one self-contained
file.

`reference/python/` is still there, and `snapdna.py` is still a stdlib-only
`.dna` reader and writer — but it is a **second** implementation, kept because
two independent readers of an undocumented format agreeing is evidence and one
agreeing with itself is not. The reference implementation is the Rust in
`crates/`.

### Run the gate before you submit

```powershell
pwsh -NoProfile -File tools/ci.ps1
pwsh -NoProfile -File tools/ci.ps1 -Corpus "C:\path\to\your\plasmids"
```

65 steps, and it is the only place all of them run together.
`.github/workflows/ci.yml` re-runs most of the same checks — a three-OS matrix
(`ubuntu-latest`, `windows-latest`, `macos-latest`), a wasm32 job, and a job
that installs the Python oracles — but it does **not** invoke this script, so it
is a second list of steps rather than the same one. Some of the gate's checks
exist only here. Run it before you submit, and "CI is green" becomes something
you know rather than something you find out.

A step whose tooling is missing **skips with a reason** rather than failing —
no Python, no corpus, no `node`, no `wix`. That is deliberate: a gate that goes
red for a missing optional package teaches people to ignore it. It also means a
green run on your machine may have skipped more than a green run on a runner, so
read the skip list.

The corpus is the one thing nobody can hand you. Real `.dna` files are not
redistributable, so the corpus suites read `PL_CORPUS` and skip cleanly without
it. Addgene distributes `.dna` files publicly if you want to build one — but see
the EULA rule above before you do.

The oracles are Python, and installing them turns on the differential steps —
the ones that check our answers against somebody else's:

```bash
pip install biopython seguid pydna pillow fonttools resvg-py numpy
```

### The zero-dependency rule, and the two exemptions

Everything under `crates/` depends on nothing but other members of this
workspace. **One crate is exempt: `crates/pl-py`**, which is PyO3 bindings and
cannot exist without `pyo3`. Under `bins/`, **`bins/pl-gui` is the other**, and
it takes `eframe`, `rfd` and `egui-phosphor`. Both exemptions are the same
argument: they face outwards, they are distribution rather than logic, and
everything either one decides about a molecule it asks a crate that has no
dependencies. `bins/pl` and `bins/pl-mcp` are inside the rule.

So: **a new dependency in `crates/` is a design change, not a convenience**, and
a pull request that adds one should say in its own words why the thing cannot be
written. That is not rhetorical — the PNG exporter hand-writes DEFLATE for
exactly this reason, and the argument is in `crates/pl-draw`.

`crates/pl-design/tests/purity.rs` goes further and is worth reading before you
write anything for that crate: it reads the crate's own sources and rejects
`std::fs`, `std::env`, `std::process`, `std::net`, the ambient clocks,
`HashMap`/`HashSet` (`RandomState` is seeded per process, so a report ordered by
hash iteration differs between runs) and `partial_cmp`. `crates/pl-index` and
`crates/pl-design` do no I/O at all; `crates/pl-scan` is the one crate that
touches the filesystem.

### Prove your test can fail

A test that passes against broken code is worse than no test, because it is
counted. This repository has shipped six of them, and every one was found by
reading rather than by a red gate — the benchmark step that reported `ok` for a
score of zero, `validate_digest.py` exiting 0 on mismatches, a wasm comparison
running with no corpus and comparing nothing.

So the expectation for a new check is: **break the thing on purpose, watch the
check go red, put it back.** Say so in the commit message, in one line, naming
what you broke and how many failures it produced. Several tests in the tree
carry that sentence in a comment; that is the pattern to copy.

`reference/python/tests/xcheck_oracles.py` is the mechanised form of this — it
injects broken behaviour into the oracles and demands they notice, each case
paired with a control. It runs on a bare checkout with no corpus and no build,
as the gate step *the oracles can fail*.

### Where the oracles live

`reference/python/tests/`, one `xcheck_*.py` per claim, most of them driving the
release binary and comparing against something that is not ours: Biopython
(digests, motif search, melting temperature, ORFs, GenBank, chromatograms), pydna
(fragments, PCR, primer binding), the `seguid` reference implementation
(checksums), SciPy (the gel spline), Pillow and `resvg` (rasters), fontTools
(glyph outlines). `bench/` is the other kind — `polylinker-bench`, a CC0 truth
set of 176 cases whose expected values came from an independent oracle, run by
`bench/run.py` and by the gate.

If you are adding a biology routine, the oracle comes with it. See
*Correctness comes before features* above.

## Where help is most useful

In rough order of value:

1. **A `.dna` corpus from a non-licensee.** Addgene distributes `.dna` files
   publicly. Re-deriving the spec in `docs/DNA-FORMAT.md` from those, by someone
   who has never accepted the SnapGene EULA, is the single highest-value
   contribution available today.
2. **Closing the open question in `docs/DNA-FORMAT.md` §4** — does SnapGene open
   a file written without blocks 2 and 3? The prototype generates one.
3. **The features database.** Curation is the bottleneck, not code. Every entry
   needs a source, accession, licence and a human sign-off.
4. **The circular map renderer.** Automatic non-overlapping label placement with
   leader lines is the hardest layout problem in the product.

## Conduct

This section used to be three lines of sentiment with nothing behind it, which
is a stub wearing a policy's clothes. Here is what is actually true.

**The expectation.** Be kind and assume good faith. Most people reading this are
biologists first and programmers second, so explain your reasoning — nobody
should have to read the diff to understand the argument. Harassment, personal
attacks, and demeaning anyone for what they do not know yet are not welcome
here, and neither is treating a wrong answer about someone's field as a wrong
answer about them.

**Who enforces it, and how.** One person: Lior Lobel, who maintains this
repository. There is no committee, no rota and no appeals process, and it would
be dishonest to imply otherwise — see `SECURITY.md`, which says the same thing
about vulnerability reports and for the same reason. What that person can do is
hide a comment, close a thread, reject a contribution and block an account, and
those are the whole toolbox.

**How to raise something.** Open an issue if it belongs in public, or use
GitHub's *Report abuse* on the comment itself if it does not. There is
deliberately no email address here, for the reason `SECURITY.md` gives: a
published address collects spam until it is unusable, and it is one more thing
that has to survive somebody changing university.

**The Contributor Covenant is not adopted**, and the reason is worth stating
rather than leaving as a gap: it comes with enforcement guidelines that assume a
group of community leaders and a staged response, and this project has one
person and no process. Adopting a document whose enforcement half would be
fiction is worse than the paragraph above, which is short and true. If this
project ever has more than one maintainer, the Covenant becomes the right answer
and this section should be replaced by it, cited by version.
