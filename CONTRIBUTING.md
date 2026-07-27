# Contributing to Polylinker

Thank you — genuinely. This project only works if more than one person cares
about it.

## The one unusual rule

**If you have accepted the SnapGene end-user licence agreement, please do not
contribute to file-format code** (`reference/`, and later `crates/pl-fileio`).
Every other part of the project is open to you.

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

Right now there is no build. The reference implementation is Python with no
dependencies beyond the standard library:

```bash
python reference/python/tests/test_roundtrip.py "path/to/your/*.dna"
```

The browser prototype is a single file with no build step — open
`prototype/dna-reader.html` and drop a `.dna` file on it.

Cross-validation against Biopython (the only external dependency, and only for
tests):

```bash
pip install biopython
python reference/python/tests/validate_digest.py "path/to/your/*.dna"
```

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

## Code of conduct

Be kind, assume good faith, and remember that most people reading this are
biologists first and programmers second. Explain your reasoning; nobody should
have to read the diff to understand the argument.
