<#
.SYNOPSIS
    Run the whole CI gate locally, in the same order CI runs it.

.DESCRIPTION
    The repository has no remote yet, so GitHub Actions has never executed.
    This is the same gate, runnable now, so "CI is green" means something
    before there is anywhere to push.

    Steps that need something the machine may not have -- a corpus, Python
    oracles, a wasm target -- are SKIPPED with a reason rather than failing.
    A gate that fails for missing optional tooling teaches people to ignore it.

.PARAMETER Corpus
    Directory of real .dna / .gb files. Corpus tests skip without it, exactly
    as they do in CI, where the corpus cannot legally exist.

.EXAMPLE
    .\tools\ci.ps1
    .\tools\ci.ps1 -Corpus "C:\Users\me\OneDrive\plasmids"
#>
[CmdletBinding()]
param([string]$Corpus)

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
# Guarded for the reason given at length in the step named
# 'the cross-platform scripts touch no Windows-only environment variable
# unguarded' near the end of this file: off Windows this variable is absent, not
# empty, and Join-Path treats a null -Path as a terminating error. The unguarded
# twin of these two lines killed both non-Windows legs of the first release.
if ($env:USERPROFILE) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo/bin'
    if (Test-Path $cargoBin) { $env:PATH = "$cargoBin$([IO.Path]::PathSeparator)$env:PATH" }
}

# Cases polylinker-bench must still pass. Asserted, not merely printed: the
# step used to report ok while the benchmark scored zero. Raise this when the
# score rises; never lower it without saying why in the commit message.
$BenchFloor = 176

$script:failed = @()
$script:skipped = @()
$started = Get-Date

function Step {
    param([string]$Name, [scriptblock]$Body, [scriptblock]$Precondition = { $true })
    if (-not (& $Precondition)) {
        Write-Host ("  SKIP  {0}" -f $Name) -ForegroundColor DarkGray
        $script:skipped += $Name
        return
    }
    Write-Host ("  ....  {0}" -f $Name) -NoNewline

    # A step must PROVE it ran. Three ways this used to report ok while
    # measuring nothing, all of them found by audit rather than by a red gate:
    #
    #   1. The command did not exist. PowerShell leaves $LASTEXITCODE at its
    #      previous value, so a missing `python` inherited the 0 from whatever
    #      ran last. Demonstrated: `nosuchcommand; $LASTEXITCODE` -> 0.
    #   2. The body threw. `$out = & $Body 2>&1` captured the error object and
    #      $LASTEXITCODE was never touched, so every `throw` in a step body was
    #      decorative.
    #   3. The body ran only cmdlets, which never set $LASTEXITCODE at all.
    #
    # The sentinel closes all three: nothing may pass without either a real
    # exit code or an explicit `$global:LASTEXITCODE = 0`.
    $sentinel = 424242
    $global:LASTEXITCODE = $sentinel
    $threw = $null
    try {
        $out = & $Body 2>&1
    } catch {
        $threw = $_
    }

    if ($threw) {
        Write-Host ("`r  FAIL  {0}      " -f $Name) -ForegroundColor Red
        Write-Host "        $threw"
        $script:failed += $Name
    } elseif ($LASTEXITCODE -eq $sentinel) {
        Write-Host ("`r  FAIL  {0}      " -f $Name) -ForegroundColor Red
        Write-Host '        this step reported no exit code, so it may have run nothing at all.'
        Write-Host '        End it with a native command, or set $global:LASTEXITCODE explicitly.'
        $script:failed += $Name
    } elseif ($LASTEXITCODE -eq 0) {
        Write-Host ("`r  ok    {0}      " -f $Name) -ForegroundColor Green
    } else {
        Write-Host ("`r  FAIL  {0}      " -f $Name) -ForegroundColor Red
        $out | Select-Object -Last 25 | ForEach-Object { Write-Host "        $_" }
        $script:failed += $Name
    }
}

function Have($cmd) { $null -ne (Get-Command $cmd -ErrorAction SilentlyContinue) }
function HavePy($mod) {
    if (-not (Have python)) { return $false }
    python -c "import $mod" 2>$null | Out-Null
    return $LASTEXITCODE -eq 0
}

Write-Host "`nPolylinker CI gate" -ForegroundColor Cyan
Write-Host ("rustc {0}" -f (rustc --version))

Write-Host "`nlint" -ForegroundColor Cyan
Step 'rustfmt'  { cargo fmt --all --check }
Step 'clippy'   { cargo clippy --workspace --all-targets -- -D warnings }

Write-Host "`ntests" -ForegroundColor Cyan
Step 'unit tests'    { cargo test --workspace --lib --bins }
Step 'release build' { cargo build --workspace --release }
Step 'corpus tests' {
    $env:PL_CORPUS = (Resolve-Path $Corpus).Path
    cargo test -p pl-fileio --test corpus -- --nocapture
} { -not [string]::IsNullOrWhiteSpace($Corpus) -and (Test-Path $Corpus) }

Write-Host "`nwasm" -ForegroundColor Cyan
$hasWasm = (rustup target list --installed) -contains 'wasm32-unknown-unknown'
Step 'wasm32 build' {
    cargo build -p pl-wasm --target wasm32-unknown-unknown --profile wasm
} { $hasWasm }
# The wasm module's own checks: the ABI, the allocator, the string boundary.
Step 'wasm module self-checks' {
    node crates/pl-wasm/tests/drive_wasm.mjs `
        target/wasm32-unknown-unknown/wasm/pl_wasm.wasm target/release/pl.exe
} { $hasWasm -and (Have node) }

# The same molecules through the wasm build and through the native binary.
#
# `drive_wasm.mjs` takes the corpus as its THIRD argument and skips the whole
# comparison when it is absent (`if (!corpus)`, drive_wasm.mjs:137). This gate
# never passed one, so a step named "wasm module vs native binary" ran the
# self-checks above and compared nothing -- green, while measuring none of the
# thing in its own name. Split out and preconditioned on the corpus, so its
# absence SKIPS loudly instead of passing silently.
Step 'wasm module vs native binary' {
    node crates/pl-wasm/tests/drive_wasm.mjs `
        target/wasm32-unknown-unknown/wasm/pl_wasm.wasm target/release/pl.exe `
        (Resolve-Path $Corpus).Path
} { $hasWasm -and (Have node) -and -not [string]::IsNullOrWhiteSpace($Corpus) -and (Test-Path $Corpus) }

# `pl-index` must never touch the filesystem.
#
# The split between the pure search engine and `pl-scan`, which owns all the
# I/O, is the whole reason the engine can be reasoned about -- and a convention
# nobody can check is a convention that decays.
#
# WHAT THIS STEP ACTUALLY ENFORCES, which is narrower than it used to claim.
# This comment used to say "wasm32 has no filesystem, so this step goes red the
# day a storage concern leaks in". It does not.  `wasm32-unknown-unknown` ships
# the full std filesystem, environment, subprocess and clock surfaces through
# its `unsupported` platform layer: `std::fs::read` compiles here, links here,
# and returns `ErrorKind::Unsupported` at run time on a target this project
# never runs. So this build catches a wasm-incompatible DEPENDENCY and an
# OS-specific API -- both worth having, neither the same claim -- and cannot
# catch a hand-written line of I/O at all.
#
# The source-level half is crates/pl-index/tests/purity.rs, run by the step
# below, which reads the crate's own sources and rejects `std::fs`, `std::env`,
# `std::process`, `std::net` and the ambient clocks, including through a braced
# or rustfmt-wrapped `use`. Two files stating opposite things about one gate is
# how a developer ends up debugging the wrong one, and the one they read first
# is this one.
#
# The wasm build also means the browser tool can search an in-memory Vec<Row>
# through the identical code.
Step 'pl-index stays pure (wasm32)' {
    cargo build -p pl-index --target wasm32-unknown-unknown --profile wasm
} { $hasWasm }
# `unit tests` runs --lib --bins and would never reach an integration test.
Step 'pl-index and pl-scan tests' {
    cargo test -p pl-index -p pl-scan --tests
}
# The same omission, found again, and this one had gone all the way: NOTHING
# ran `crates/pl-design/tests/`.
#
# `unit tests` is `--workspace --lib --bins`, which reaches no integration
# target, so every other integration suite in this file is named explicitly --
# pl-fileio's corpus, the step above, pl-index's scale test, pl-draw's five.
# pl-design was never added to that list, and a grep for `pl-design` over
# `tools/` and `.github/` returned zero hits, so six suites -- design.rs,
# determinism.rs, purity.rs, refusals.rs, scoring.rs, specificity_prefilter.rs,
# 45 tests between them -- had never been run by any gate on any platform.
#
# What was silently unenforced: `nothing_under_src_touches_storage_a_clock_or_
# a_hash_order` (the crate's no-I/O, no-clock, no-hash-iteration-order rule),
# `the_manifest_lists_only_workspace_crates` (the zero-dependency rule), and
# `determinism.rs`, which is the only thing asserting that two runs of the same
# input produce byte-identical output.
#
# The wasm32 backstop `purity.rs`'s header used to invoke does not exist for
# this crate either: the only wasm32 builds here are `-p pl-wasm` and
# `-p pl-index`, and `pl-wasm` does not depend on `pl-design`. So this step is
# the whole enforcement, not a second line of it.
#
# All 45 passed on the first run after being added, which is the good outcome
# and not evidence the step is unnecessary: it means the rules held while
# nothing was checking them, and nothing would have said so if they had not.
Step 'pl-design tests' {
    cargo test -p pl-design --tests
}
# The compiled-in release public key, and the updater that reads it.
#
# No `--tests` filter, unlike the two steps above, and the difference is worth a
# line because it used to have one. When this crate was three constants and no
# code, every test it had was an integration test and `--tests` was the whole
# suite. It now has both: unit tests inside `src/` -- where the private
# constructors they exercise are reachable -- and integration tests in `tests/`.
# `cargo test -p pl-update` runs both. `unit tests` above is
# `--workspace --lib --bins` and would reach the first but not the second, so
# filtering here would silently drop the four suites that hold requirements 1
# and 4.
#
# WHAT WOULD OTHERWISE GO UNNOTICED, and it is now two things.
#
# `RELEASE_PUBLIC_KEY` is 32 byte literals. A wrong nibble in it compiles,
# links, ships, and is first noticed by a user whose updater refuses a genuine
# release -- or does not refuse a forged one. `crates/pl-update/tests/key.rs` is
# the only thing that looks at the value.
#
# And `crates/pl-update/tests/handoff.rs` is the only thing that holds
# docs/RELEASING.md's requirements 1 and 4, which are claims about what the
# crate does NOT do -- no thread, no timer, no clock, no `Command::new` outside
# the one file that runs `curl`, nothing that copies over or launches the file
# it downloaded. Those cannot be caught by calling anything: a version that
# launched the installer would pass every functional test in the crate, because
# the file would still be in the right place with the right hash. So it reads
# the sources, and if this step did not run it, nothing would.
#
# It also pins the coupling `.github/workflows/release.yml` depends on: that
# workflow greps the key's base64 form out of `src/lib.rs` so the signature it
# verifies is checked against the key that actually ships rather than a second
# copy in YAML, and `exactly_one_base64_key_appears_in_the_source_the_release_
# workflow_reads` is what stops a doc-comment edit breaking a release nobody
# can test until the tag is already pushed.
Step 'pl-update tests (the release key, and the updater that reads it)' {
    cargo test -p pl-update
}
# Size and speed at three thousand plasmids.
#
# Nothing else in this gate would notice an index costing 800 MB or a query
# allocating 90 MB, and that is not hypothetical: the first index of a real lab
# drive came out at 5.9 GB with every functional test passing. The test carries
# its own counting global allocator -- zero dependencies, same on all three CI
# operating systems -- and asserts index size, open time, query time and peak
# allocation. Loose tripwires for an order-of-magnitude regression, not
# benchmarks; raise one only with a reason in the commit message, same rule as
# $BenchFloor.
Step 'index size and speed at 3,000 plasmids' {
    cargo test -p pl-index --test scale --release
}

# Does the index agree with the files it was built from?
#
# The deepest risk in the library feature: a wrong sequence offset, or a column
# lost on write, makes the index answer confidently about the wrong molecule --
# and nothing in a self-consistent round-trip can see it. `--no-index` re-reads
# every file and answers from scratch, so the two must agree exactly.
#
# **The index is written by one invocation and read by the next.** Doing both in
# one process checks nothing: `pl find` answers from the library it just built
# in memory, so a bug that only corrupts the file on write stays invisible.
# Verified by injecting exactly that -- truncating the searchable text in
# `to_bytes` -- and watching the single-process form stay green while this form
# fails.
#
# Fixtures are real files under tests/library-fixture rather than strings built
# here: an earlier version constructed GenBank with PowerShell escapes, the
# backtick-n was eaten, every file failed to parse, and both sides agreed on
# nothing -- which the step reported as success.
#
# Structurally blind to anything both paths share: they use the same parser, so
# a mis-parse is wrong identically in both. It catches storage bugs, which is
# what it is for.
Step 'the index agrees with the files' {
    $lab = 'tests/library-fixture'
    $idx = Join-Path ([System.IO.Path]::GetTempPath()) 'pl-ci-index'
    if (Test-Path $idx) { Remove-Item -Recurse -Force $idx }

    & target/release/pl.exe index $lab --index-at $idx 2>$null | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Host '        could not build the index'
        $global:LASTEXITCODE = 1
        return
    }

    $queries = @(
        @('--motif','GAATTC'), @('--motif','GGATCC'), @('--motif','RGCY'),
        @('--motif','N','--limit','5'), @('--motif','GAATTC','--absent'),
        @('--enzyme','EcoRI'), @('--name','a'), @('--text','AmpR'),
        @('--text','spacer'), @('--motif','GAATTC','--topology','undeclared'),
        @('--length','10..30'), @('--state','ok'), @('--features','2..2')
    )
    $bad = 0
    foreach ($q in $queries) {
        $indexed = & target/release/pl.exe find $lab @q --index-at $idx 2>$null | Out-String
        $direct  = & target/release/pl.exe find $lab @q --no-index 2>$null | Out-String
        if ($indexed -cne $direct) {
            $bad++
            Write-Host "        DIFFER: $($q -join ' ')"
            Write-Host "        indexed: $($indexed -replace "`r?`n",' | ')"
            Write-Host "        direct : $($direct  -replace "`r?`n",' | ')"
        }
    }
    # A query set that matches nothing would agree trivially, so assert that
    # the fixture actually produces hits.
    $hits = & target/release/pl.exe find $lab --motif GAATTC --index-at $idx 2>$null | Out-String
    # Parse the count rather than anchoring a regex at the start of the string:
    # `pl find` opens with the motif header, and PowerShell's -match has no
    # Multiline, so the old `^0 records matched` clause could never match and
    # the guard only ever caught a missing footer.
    $matched = if ($hits -match '(\d+)\s+records?\s+matched') { [int]$Matches[1] } else { -1 }
    if ($matched -le 0) {
        Write-Host '        the fixture produced no matches; the comparison is vacuous'
        $bad++
    }
    Remove-Item -Recurse -Force $idx
    if ($bad -gt 0) {
        Write-Host "        $bad problem(s) across $($queries.Count) queries"
        $global:LASTEXITCODE = 1
    } else {
        $global:LASTEXITCODE = 0
    }
}

Write-Host "`noracles — our answers checked against tools that are not ours" -ForegroundColor Cyan

# The oracles themselves, checked by injecting the fault they exist to catch.
#
# This gate has now shipped six checks that were green by construction, every
# one found by reading rather than by a red gate: the bench step that reported
# ok for a score of zero; validate_digest.py exiting 0 on both mismatches and an
# empty run; "wasm module vs native binary" running with no corpus and comparing
# nothing; test_roundtrip.py counting 344 problems and exiting 0; xcheck_eps.py
# claiming the BoundingBox contained every coordinate while dropping every line
# with ` show` on it, which is every label. The sixth was inside the meta-check
# written for the fourth: it asserted `rc != 0`, and the broken main() returned
# None, so it passed against the code it was written to catch.
#
# So this step injects the break and demands the oracle notice, with a control
# beside each one. No fixtures, no corpus, no build -- it runs on a bare
# checkout, which is the point: the cheapest step here should be the one that
# says the expensive ones can fail.
Step 'the oracles can fail' {
    python reference/python/tests/xcheck_oracles.py
} { Have python }

Step 'SEGUID vs the reference' {
    if ([string]::IsNullOrWhiteSpace($Corpus)) {
        python reference/python/tests/xcheck_seguid.py target/release/pl.exe
    } else {
        python reference/python/tests/xcheck_seguid.py target/release/pl.exe "$Corpus\**\*.dna"
    }
} { HavePy 'seguid' }
Step 'digest + PCR vs pydna' {
    python reference/python/tests/xcheck_clone.py target/release/pl.exe
} { (HavePy 'pydna') -and (HavePy 'Bio') }
# Degenerate, both-strand, origin-wrapping motif search vs Biopython.
#
# Every other oracle here uses restriction sites, and every site in the shipped
# table is a non-degenerate palindrome -- so before this step nothing compared a
# degenerate pattern or a minus-strand hit against an outside implementation,
# and the library's headline query had an oracle for none of its interesting
# cases. Verified the way this file's header asks: disabling the palindrome
# collapse produced 107 disagreements, restoring it produced 0.
Step 'motif vs Biopython (degenerate, both strands)' {
    python reference/python/tests/xcheck_motif.py target/release/pl.exe
} { HavePy 'Bio' }
# Are our zlib streams streams anybody else can read?
#
# `pl-draw` hand-writes DEFLATE, because `crates/` take no dependencies and a
# PNG needs one. Its own tests round-trip against an `inflate` written from
# RFC 1951 -- which catches nearly everything, and cannot catch the two of them
# misreading the spec the same way, since one author read it once.
#
# `zlib` is the reference implementation, by other people, and is what will open
# these files. Checked one-shot AND a byte at a time: a stream can decode
# correctly in one call and still be malformed for an incremental reader, which
# is what browsers and image libraries are. `eof` and `unused_data` are asserted
# too, so a missing final-block bit cannot pass.
#
# Verified the way this file insists, with three deliberate breakages:
#
#   - Huffman codes written low-bit-first instead of high-bit-first: zlib
#     refused 8 of the 10 streams. (The 2 that passed are the empty and
#     single-byte cases, whose trees are too degenerate for bit order to show.)
#   - the final-block bit never set: zlib refused 9 of 10.
#   - four trailing bytes appended: one-shot decoding accepted ALL 10, and the
#     byte-at-a-time pass rejected all 10 on `unused_data`.
#
# The third is why the streaming half is here, and it is the only one that
# needed it -- the first two were caught by the one-shot decode on its own.
# Trailing rubbish is the realistic version: a buffer written twice, or a length
# computed once too often.
#
# `Have python`, not `HavePy`, and that is the whole reason this precondition
# was missing: the three siblings below each need a package (`PIL`,
# `fontTools`, `resvg_py`) and `HavePy` is what asks for one, while
# xcheck_deflate.py imports `os`, `sys` and `zlib` and nothing else. So there
# was no module to name -- and the interpreter went unasked-for with it. A
# Rust-only machine then got FAIL from `Step`'s catch on the
# CommandNotFoundException, which is precisely the outcome this file's header
# says the skip mechanism exists to prevent. Reproduced with PATH cut to
# `.cargo\bin` and System32: bare, `FAIL ... The term 'python' is not
# recognized`; with this precondition, `SKIP`.
Step 'zlib streams vs zlib' {
    # THE EXIT CODE, not just the artifacts. `Step` judges on $LASTEXITCODE
    # after the whole body, which would be Python's -- and the checkers only
    # require the files to EXIST. So a failing Rust test left the previous
    # run's output in target/tmp and this step read it and said ok. Same
    # hazard the header of this file documents three earlier versions of.
    cargo test -q -p pl-draw --test zstream 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Output 'cargo test --test zstream failed; re-running it visibly'
        cargo test -p pl-draw --test zstream
        $global:LASTEXITCODE = 1
        return
    }
    python reference/python/tests/xcheck_deflate.py .
} { Have python }
# Are our PNGs pictures anybody else sees the same way?
#
# `src/png/tests.rs` parses the file with the same understanding that wrote it:
# it knows where IHDR is because it put IHDR there. PIL is a different decoder
# by other people, and stands in for every program that will open these figures.
# Every pixel is compared against the raw RGB buffer written beside the file --
# never against a re-encode, so no second encoder of ours is in the loop -- and
# `info["dpi"]` against the resolution asked for, because "at a specified
# physical width and dpi" is the roadmap row and a figure whose dpi does not
# survive arrives in a manuscript at a size nobody chose.
#
# Verified the way this file insists, and the fourth line is the useful one:
#
#   - channels written B,G,R instead of R,G,B: all 4 files failed, the flat one
#     on 5120 of 7680 bytes.
#   - IHDR CRC corrupted: PIL refused the file outright.
#   - IHDR lying about its colour type, CRC recomputed to match: PIL refused.
#   - **IDAT CRC corrupted: PIL opened it and every pixel still matched.**
#
# So PIL does not verify IDAT's CRC, and this step cannot see a wrong one. The
# unit test `every_chunk_carries_the_right_crc` is the only thing covering that,
# which is worth knowing before anyone decides it is redundant with this.
Step 'PNGs vs PIL' {
    # THE EXIT CODE, not just the artifacts. `Step` judges on $LASTEXITCODE
    # after the whole body, which would be Python's -- and the checkers only
    # require the files to EXIST. So a failing Rust test left the previous
    # run's output in target/tmp and this step read it and said ok. Same
    # hazard the header of this file documents three earlier versions of.
    cargo test -q -p pl-draw --test pngfile 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Output 'cargo test --test pngfile failed; re-running it visibly'
        cargo test -p pl-draw --test pngfile
        $global:LASTEXITCODE = 1
        return
    }
    python reference/python/tests/xcheck_png.py .
} { HavePy 'PIL' }
# Glyph outlines vs fontTools.
#
# `crates/pl-draw/src/font.rs` walks the `glyf` table by hand, because
# `crates/` take no dependencies. NOTHING IN THIS REPOSITORY CAN CHECK THAT
# WALK: a test that reads `glyf` the way `font.rs` reads `glyf` agrees with
# itself by construction. fontTools is another implementation, in another
# language, by other people.
#
# The implied on-curve point -- where two consecutive off-curve points imply an
# on-curve point at their midpoint -- is NOT re-derived on the Python side. The
# checker subclasses `BasePen`, whose own `qCurveTo` performs that expansion,
# so the rule under test is fontTools' statement of it and not a second copy of
# ours. An earlier design proposed a glyph bounding box for this, which is
# blind to ignoring the on-curve flag entirely.
#
# It earned its keep on the first run, before it ever passed: it found this
# reader emitting an extra closing `Line` per contour (131 of 190 glyph-face
# pairs) and then a second off-by-one where the loop revisited the contour's
# start. Both are invisible in a rendered glyph. It now compares 8,657 outline
# commands over 382 glyph-face pairs, 117 of them composites, with 0
# disagreements.
#
# That said 3,504 until 2026-08-04, which is the ASCII-only total -- 95
# codepoints across 2 faces, with NO composite among them. It was the figure
# from before this session widened the range to Latin-1, and the reason for
# widening it was precisely that ASCII judged `Face::composite` not at all. So
# the one line in the repository that says how far the only independent oracle
# reaches was quoting the reach it had before the composite branch was brought
# inside it. `xcheck_glyphs.py` now re-derives all four numbers and fails this
# step when they drift, so the figure cannot decay that way twice. (The 131 of
# 190 above is past tense about the first run, and stays as it was.)
Step 'glyph outlines vs fontTools' {
    # THE EXIT CODE, not just the artifacts. `Step` judges on $LASTEXITCODE
    # after the whole body, which would be Python's -- and the checkers only
    # require the files to EXIST. So a failing Rust test left the previous
    # run's output in target/tmp and this step read it and said ok. Same
    # hazard the header of this file documents three earlier versions of.
    cargo test -q -p pl-draw --test glyphs 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Output 'cargo test --test glyphs failed; re-running it visibly'
        cargo test -p pl-draw --test glyphs
        $global:LASTEXITCODE = 1
        return
    }
    python reference/python/tests/xcheck_glyphs.py .
} { HavePy 'fontTools' }
# The whole raster, against an independent SVG renderer.
#
# Every other check on the rasterizer is a property -- this pixel is dark, that
# area is right, this colour parses. None of them can say the PICTURE is right.
# resvg is handed the SVG this crate already emits and forced onto the same two
# font files this crate fills outlines from, so a text disagreement is about
# placement rather than about which face got picked. One comparison then covers
# arc flattening, winding, stroke construction, antialiasing, glyph decoding,
# glyph placement, the baseline constant, anchors and colour at once.
#
# THE DISCRIMINATOR IS *WHERE* THE PIXELS DIFFER, not how many. Two correct
# antialiasing implementations disagree slightly along edges and nowhere else,
# so every grossly differing pixel must sit on a gradient; a wrong arc, a wrong
# winding, a misplaced glyph or a hole in a stroke all put differing pixels in
# FLAT regions, where two correct renderers must agree exactly. A count-based
# trend was tried first and discarded: at 1x the figure has ONE grossly
# differing pixel, so any ratio against it is noise.
#
# Currently 98.3% of pixels identical at 1x and 99.5% at 4x, with 100% of the
# residue on an edge at both.
Step 'the raster vs resvg' {
    # THE EXIT CODE, not just the artifacts. `Step` judges on $LASTEXITCODE
    # after the whole body, which would be Python's -- and the checkers only
    # require the files to EXIST. So a failing Rust test left the previous
    # run's output in target/tmp and this step read it and said ok. Same
    # hazard the header of this file documents three earlier versions of.
    cargo test -q -p pl-draw --test render 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Output 'cargo test --test render failed; re-running it visibly'
        cargo test -p pl-draw --test render
        $global:LASTEXITCODE = 1
        return
    }
    python reference/python/tests/xcheck_render.py .
} { (HavePy 'resvg_py') -and (HavePy 'PIL') -and (HavePy 'numpy') }
# The window icon is the .ico's own 64 px frame, and both are polylinker.svg.
#
# THE TASKBAR BUTTON OF A RUNNING WINDOW DOES NOT COME FROM THE .EXE'S ICON.
# winit never reads the executable's resource directory; a window's icon is an
# `HICON` set with `WM_SETICON` on Windows and a `_NET_WM_ICON` property on X11,
# both fed from `egui::ViewportBuilder::with_icon`, which takes raw RGBA and
# cannot take a `.ico`. So `bins/pl-gui/icon/` holds two generated artefacts from
# one master, and two artefacts from one master is the shape that goes stale --
# this repository has produced that exact defect several times in one week.
#
# `bins/pl-gui/src/main.rs` holds a sha256 of each, which proves they came out of
# ONE run of `build-icon.py`. It cannot compare a pixel: the `.ico`'s frames are
# PNG and this project has no PNG decoder, only an encoder. THIS step is the one
# that decodes the 64 px frame and compares all 16,384 bytes against the blob the
# window gets, and then compares every frame against a fresh resvg render of
# `polylinker.svg`.
#
# THAT LAST PART CLOSES THE GAP the step 'the built binaries carry their icon and
# version resource' documents on itself. That step proves the .exe carries the
# .ico's bytes and is blind to the .ico being a stale rasterisation of an edited
# master, because both sides of its comparison move together. It stays
# dependency-free and this step, which needs Python, resvg and PIL, is where the
# master enters the comparison.
#
# Verified the way this file insists, on 2026-08-05, three injections:
#
#   * one byte of `polylinker-64.rgba` moved from 178 to 180 -> `the .ico's
#     64x64 frame vs polylinker-64.rgba: 1 of 16384 bytes differ, first at byte
#     5398 (pixel 1349, channel B): 178 became 180`;
#   * the master's short blue bar moved 4 units and ONLY the .ico regenerated ->
#     2 disagreements, 59 of 16384 bytes, which is the drift this exists for;
#   * the same edit with NEITHER artefact regenerated -> 10 disagreements, every
#     one of the nine frames and the blob. That is the gap the release-section
#     step cannot see, and it is now red.
#
# `xcheck_oracles.py` pins the comparator itself, including the case where it
# compares nothing.
Step 'the window icon is the .ico''s own frame' {
    python reference/python/tests/xcheck_icon.py .
} { (HavePy 'resvg_py') -and (HavePy 'PIL') }
# Is the PDF a PDF, and is it the same picture as the SVG?
#
# `pl-draw` builds one Scene and renders it twice, so they ought to match --
# but the PDF back end flips the coordinate system, turns every arc into
# Beziers, and places text by *measuring* it, since PDF has no text-anchor.
# Each of those can be plausibly wrong. pypdf and PyMuPDF open the file (a bad
# xref offset gives a file that greps fine and opens in nothing), every string
# the SVG draws is checked present, and every single-word string's position is
# compared against the SVG's with the anchor resolved.
#
# Verified the way this file insists: shifting the Helvetica width table by one
# entry produced 2 disagreements, removing the y flip produced 41.
Step 'PDF is a PDF, and matches the SVG' {
    python reference/python/tests/xcheck_pdf.py target/release/pl.exe
} { (HavePy 'fitz') -and (HavePy 'pypdf') }
# Melting temperature vs Biopython.
#
# Biopython implements the same published nearest-neighbour model
# independently, and docs/PLAN.md names its tables as the licence-clean source
# for the parameters (Primer3's oligotm.c is GPL-2.0). The numbers were taken
# from it, so this checks the arithmetic written here -- and it earned its keep
# immediately: the plan's own formula had the concentration convention
# backwards (x = 4 for self-complementary, 1 otherwise, where SantaLucia is the
# reverse), which put every palindrome out by ~8 C. Nothing hand-written here
# noticed; 480 comparisons against Biopython did, on the first run.
Step 'melting temperature vs Biopython' {
    python reference/python/tests/xcheck_tm.py target/release/pl.exe
} { HavePy 'Bio' }
# Primer binding sites vs pydna.
#
# docs/PLAN.md §7.3 names pydna's `limit` as the reference for the seed length,
# so pydna is the natural oracle. Run through `pl primers --exact`, since pydna
# stops a footprint at the first mismatch and our default walks through
# isolated ones -- a real difference that showed up as six disagreements on
# tailed primers before the modes were separated.
#
# The tailed cases are the point: the footprint must come back *without* the
# tail, or the Tm printed beside it belongs to a different oligo.
Step 'primer binding sites vs pydna' {
    python reference/python/tests/xcheck_primers.py target/release/pl.exe
} { HavePy 'pydna' }
# All 27 NCBI genetic codes, and the ORFs read with them, against Biopython.
#
# Four checks of deliberately different kinds, because the failure modes are
# different in kind. Biopython supplies the tables; every reported ORF is
# re-translated from its own coordinates (which catches a span off by three, on
# the wrong strand, or wrapped the wrong way -- all of which still produce a
# plausible-looking protein); linear ORF sets are enumerated a second time in
# protein space; and circular ones must survive rotation.
#
# The rotation check earned its place immediately. The scan used to begin at
# the origin, which is an arbitrary cut, so the first start codon it met was
# whichever one happened to sit nearest that cut -- rotating a plasmid changed
# its ORFs. Synchronising to a stop first removed the origin from the answer.
#
# Verified it can fail three ways: reverting the origin sync gives 22
# disagreements; flipping one residue in table 24 is caught and named exactly
# ('GTG', 'A', 'V'); and reading stops off the amino-acid line instead of the
# Starts line breaks tables 27, 28 and 31, where a codon is both a stop and an
# amino acid.
Step 'genetic codes and ORFs vs Biopython' {
    python reference/python/tests/xcheck_translate.py target/release/pl.exe
} { HavePy 'Bio' }
# Sanger read placement vs Biopython's PairwiseAligner.
#
# The *score* is compared, not the traceback. An optimal alignment is usually
# not unique -- when the bases flanking a deletion repeat, two gap placements
# score identically and both are right -- so comparing columns would be
# comparing tie-breaks. The score is well defined and moves for any error in
# the recurrence, the initialisation, the affine bookkeeping or the k-mer
# windowing. 60 reads, 10,601 read bases, circular and reverse-primer cases
# included; the windowing has never yet cost a point against the full-reference
# optimum.
#
# Verified it can fail: aligning only the forward orientation gives 17
# disagreements, and charging a gap opening without its first extension -- an
# off-by-one invisible on any read without an indel -- gives 31.
Step 'Sanger placement vs Biopython' {
    python reference/python/tests/xcheck_sanger.py target/release/pl.exe
} { HavePy 'Bio' }
# The gel's calibration spline vs SciPy's PchipInterpolator.
#
# SciPy's PCHIP *is* Fritsch-Carlson monotone cubic Hermite -- the same 1980
# algorithm, written by other people -- so this is a genuine second
# implementation rather than a transcription of ours.
#
# It matters because a gel calibration must be monotone: a longer fragment
# cannot run further than a shorter one. An ordinary cubic through real,
# unevenly spaced ladder points (3, 4, 6, 10 kb) overshoots by enough to swap
# two bands, which is not a rounding error but the wrong answer about which
# band is which. The tangent-clamping rules that prevent it are exactly the
# fiddly kind of thing to check against someone else, and ladder-shaped knot
# sets are included because the one-sided end-tangent rule applies at a
# ladder's largest and smallest bands.
#
# 4,312 points across 44 knot sets, worst relative difference 4.9e-13.
# EPS against the PDF, and against PostScript's own rules.
#
# **No PostScript interpreter is installed here** -- no Ghostscript, and MuPDF
# refuses EPS -- so this does NOT prove a renderer draws the file. Saying so
# matters, because "the EPS oracle passes" would otherwise sound like more than
# it is. What it does prove: the geometry is point-for-point the PDF's after the
# y-flip (284 path operators), gsave/grestore and every string literal balance,
# only Level 2 operators appear, and the BoundingBox contains the artwork --
# paths and all 79 label ink boxes.
#
# "and the labels" is new, and the old wording was false. The bbox arm compared
# the box against `eps_tokens`, which skips every line containing ` show` -- and
# the emitter writes each label as one `X Y moveto (text) show` line, so NO text
# coordinate was ever tested against the box. The gap is live, not theoretical:
# pl-draw's label gutter is capped at 30% of the canvas, so a feature name over
# roughly 28 characters overflows it. A two-CDS GenBank file carrying
# "aph(3')-Ia aminoglycoside phosphotransferase gene" makes the shipped binary
# emit a right-column label running from x=530 to x=806.4 under
# `%%BoundingBox: 0 0 720 720` -- 86 points of gene name cropped off the plate,
# while this oracle printed `problems : 0`. That one is a LAYOUT bug in pl-draw
# and is NOT fixed here; the check that finds it is. Every fixture in the gate
# has names short enough to stay inside the box, which is precisely why the hole
# went unnoticed: a check whose fixtures cannot exercise it proves nothing.
#
# Labels are measured as ink boxes -- origin, plus the advance width of the
# decoded bytes, plus Helvetica's 0.718 ascent and 0.207 descent. The origin
# alone would not do: right-column labels are `Anchor::Start`, so the `moveto`
# stays inside the box while the string runs out of it, which is exactly the
# case above.
#
# The circle is drawn as four Beziers rather than PostScript's native `arc`, on
# purpose: PDF has no arc operator, and two formats that approximate a circle
# differently are two slightly different figures.
#
# Verified it can fail four ways: not flipping the y axis (an upside-down but
# otherwise perfect figure) is caught at operator 0; a BoundingBox 20% too small
# is caught as "the figure would be cropped"; a label outside the box is caught
# as "the label would be cropped", proved on the long-name file above and pinned
# without a fixture in `the oracles can fail`; and leaving ')' unescaped drops a
# label. That third one needed a fixture -- `tests/export-fixture/hostile-names.gb`
# exists because none of the other fixtures has a parenthesis in a feature name,
# so the check could not fail and therefore proved nothing. The name in it is
# real: aph(3')-Ia is what this project's own database calls KanR.
Step 'EPS agrees with the PDF, and is valid PostScript' {
    python reference/python/tests/xcheck_eps.py target/release/pl.exe (Get-ChildItem tests/library-fixture/*.gb, tests/export-fixture/*.gb | ForEach-Object { $_.FullName })
} { Have python }

# The Python bindings, checked from Python, against Biopython.
#
# The bindings exist so a script already using Biopython can call Polylinker for
# the parts that are hard to get right without rewriting the pipeline, and that
# argument only holds if the two agree where they overlap. The Rust side is
# already cross-validated, so this checks the *boundary*: arguments passed
# through unchanged, coordinates unshifted, and failures raised rather than
# returned as a number. 1,268 checks.
#
# Verified it can fail -- and the first version could NOT. Dropping the circular
# flag at the FFI edge changed nothing, because random sequence almost never
# carries a recognition site across the origin, which is the only case where
# circular and linear must differ. The check now plants sites split across the
# join for every unambiguous enzyme; with those, the same injection gives 616
# disagreements. A case that cannot distinguish the two answers proves nothing,
# and the file says so where the cases are built.
Step 'Python bindings vs Biopython' {
    cargo build --release -p pl-py 2>&1 | Out-Null
    $ext = if ($IsWindows -or $env:OS -eq 'Windows_NT') { 'dll' } else { 'so' }
    $src = Join-Path 'target/release' ("$(if ($ext -eq 'dll') { '' } else { 'lib' })polylinker.$ext")
    $dst = Join-Path $env:TEMP 'polylinker.pyd'
    Copy-Item $src $dst -Force
    python reference/python/tests/xcheck_pybindings.py $dst
} { HavePy 'Bio' }
# The MCP server answers JSON-RPC, and keeps its caveats across the boundary.
Step 'MCP server' {
    cargo test -p pl-mcp --quiet
}

Step 'gel calibration spline vs SciPy' {
    cargo build --release -p pl-gel --example dump_spline 2>&1 | Out-Null
    python reference/python/tests/xcheck_spline.py target/release/examples/dump_spline.exe
} { HavePy 'scipy' }
# The rendered chromatogram agrees with the file it came from.
#
# Nothing external renders ABIF, so the SVG is read back and asserted to have
# the property it must have: at each called base, the curve in that base's
# colour is the tallest of the four. 40 real traces, 23,057 bases above Q30,
# 99.94%. Bases below Q30 are skipped and counted -- overlapping peaks at the
# end of a read are the chemistry, not the renderer.
#
# Verified it can fail: swapping two channels scores 49.08%.
#
# Verified it CANNOT see one thing, which is why the note is here: every one of
# the 374 ABIF files on this drive carries FWO_=GATC, so hard-coding that order
# rather than reading FWO_ is invisible to this step -- injecting exactly that
# changed the number by nothing. The unit test
# `every_base_is_drawn_in_the_colour_its_own_channel_says` builds synthetic
# traces in three channel orders and is the actual guard.
Step 'rendered chromatograms agree with their files' {
    python reference/python/tests/xcheck_trace_render.py target/release/pl.exe "$Corpus\**\*.ab1" $env:TEMP
} { (HavePy 'Bio') -and -not [string]::IsNullOrWhiteSpace($Corpus) -and (Test-Path $Corpus) }
# Chromatograms vs Biopython, on real traces.
#
# Corpus-gated: 394 .ab1 files live on the lab drive and none may live here.
# The numbers that make this worth running: 336,268 base calls compared, and
# **20 of the 394 files are not ABIF at all** (4 SCF, 16 ZTR wearing an .ab1
# name), which both implementations must refuse -- counted rather than skipped,
# because "we agreed to fail" is a result and dropping them silently would
# compare less than the summary claims.
#
# Verified it can fail: preferring PBAS1 (the human's edit) over PBAS2 (the
# basecaller's) gives 217 disagreements, which is exactly the number of files
# where a human had edited the read.
Step 'chromatograms vs Biopython' {
    python reference/python/tests/xcheck_abif.py target/release/pl.exe "$Corpus\**\*.ab1"
} { (HavePy 'Bio') -and -not [string]::IsNullOrWhiteSpace($Corpus) -and (Test-Path $Corpus) }
# A second, independent digest oracle over real plasmids.
#
# `xcheck_clone.py` above compares fragments against pydna on synthetic cases;
# this compares cut *positions* against Biopython on the corpus, and runs the
# Python transcription alongside the Rust so a divergence says which one moved.
# It guards `cut_positions`, which is now a thin wrapper over
# `pl_core::iupac::find_all` shared with the library's motif search.
#
# Corpus-gated because no `.dna` may live in this repo (see the header) and the
# script now — correctly — fails rather than passes when it compares nothing.
Step 'digest vs Biopython (real plasmids)' {
    python reference/python/tests/validate_digest.py target/release/pl.exe "$Corpus\**\*.dna"
} { (HavePy 'Bio') -and -not [string]::IsNullOrWhiteSpace($Corpus) -and (Test-Path $Corpus) }
Step 'rust reader vs python reader' {
    python reference/python/tests/xcheck_rust.py target/release/pl.exe "$Corpus\**\*.dna"
} { (HavePy 'Bio') -and -not [string]::IsNullOrWhiteSpace($Corpus) -and (Test-Path $Corpus) }

Write-Host "`nannotation database" -ForegroundColor Cyan
Step 'features.tsv satisfies its own schema' {
    cargo test -p pl-features --test corpus the_shipped_database 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { cargo test -p pl-features --test corpus the_shipped_database }
} { Test-Path 'features/features.tsv' }
Step 'every coding record translates to its protein' {
    cargo test -p pl-features --test corpus every_coding_record 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { cargo test -p pl-features --test corpus every_coding_record }
} { Test-Path 'features/features.tsv' }
# `docs/PLAN.md` §v1.0 item 5 claims "under 200 ms for a 10 kb plasmid". It
# claimed that for months with nothing computing it, which is precisely how
# `rust-version` sat at a wrong 1.82 -- and a performance budget is the worse of
# the two, because it is cited by everybody and checked by nobody.
#
# RELEASE, and the profile is the whole point of the step. The budget describes
# what a user's machine does and no user runs an unoptimised build: the same two
# molecules are 106 ms and 1,075 ms in debug against 11 ms and 103 ms in
# release, so a debug run would report a tenfold miss that means nothing.
# `budget.rs` accordingly asserts nothing at all under `debug_assertions`, and
# would be a green tick forever if it were run the way `unit tests` runs
# everything else.
#
# Named explicitly because it has to be: `unit tests` above is
# `--workspace --lib --bins` and reaches no integration target. That is the
# omission this file has already documented finding twice -- pl-design's six
# suites had never been run by any gate at all, and pl-features' own corpus
# tests are named one function at a time immediately above for the same reason.
Step 'annotation is inside the 200 ms budget the plan claims' {
    cargo test --release -p pl-features --test budget 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { cargo test --release -p pl-features --test budget -- --nocapture }
}
Step 'no SnapGene sseqid fingerprint in our ids' {
    # PLAN 8.3 rule 2: `CmR_(2)` / `KanR_(3)` is a copying fingerprint, and
    # 21.5% of their rows carry it. Ours must never look like that.
    $bad = Select-String -Path 'features/features.tsv' -Pattern '^\S*_\(\d+\)	' -AllMatches
    if ($bad) { Write-Output "sseqid-style ids found: $($bad.Count)"; $global:LASTEXITCODE = 1 }
    else { $global:LASTEXITCODE = 0 }
} { Test-Path 'features/features.tsv' }

Write-Host "`ncircular-map (TypeScript)" -ForegroundColor Cyan
Step 'typecheck' {
    Push-Location packages/circular-map; npx --no-install tsc -p tsconfig.json --noEmit; Pop-Location
} { (Have node) -and (Test-Path 'packages/circular-map/node_modules') }
Step 'circular-map tests' {
    Push-Location packages/circular-map
    node --test --experimental-strip-types test/*.test.ts
    Pop-Location
} { Have node }

# Do the two renderers still agree?
#
# The desktop app draws with egui and the browser tool is TypeScript, so there
# are two implementations of one specification. Checking one against the other
# is the only thing that makes the second one worth its cost -- it has already
# caught three divergences, including JavaScript's Math.round breaking ties
# toward +Inf where Rust's breaks them away from zero.
#
# Regenerating first is the point. A fixture that is only ever read records what
# the reference used to say, and agrees with nothing.
Step 'renderers agree (fixture is current)' {
    # Compared by content, not by `git diff --exit-code`.
    #
    # git only reports on *tracked* files, so the obvious version of this step
    # passed silently for a brand-new fixture -- checking nothing while printing
    # ok, which is precisely how the bench step once reported a score of zero as
    # a pass. Comparing bytes works whether or not the file is in the index.
    $fixture = 'crates/pl-draw/tests/agreement.json'
    $fresh = Join-Path ([System.IO.Path]::GetTempPath()) 'pl-agreement.json'
    # The generator writes the file itself. Redirecting it here would re-encode
    # stdout as CRLF, so the check would compare a bash-written fixture against
    # a PowerShell-written one and always disagree.
    node --experimental-strip-types tools/gen-agreement.mjs $fresh
    if ($LASTEXITCODE -ne 0) { return }
    $a = if (Test-Path $fixture) { Get-Content -Raw $fixture } else { '' }
    $b = Get-Content -Raw $fresh
    if ($a -ceq $b) {
        Remove-Item $fresh
        $global:LASTEXITCODE = 0
    } else {
        Copy-Item $fresh $fixture -Force
        Remove-Item $fresh
        Write-Host '        the TypeScript renderer changed and the fixture was stale.'
        Write-Host '        it has been regenerated -- review the diff, then re-run.'
        $global:LASTEXITCODE = 1
    }
} { Have node }
Step 'renderers agree (rust replays it)' {
    # Explicitly, because `unit tests` runs --lib --bins and would never reach
    # an integration test.
    cargo test -p pl-draw --test agreement
} { Have node }

# This heading said "benchmark" while the two steps under it were the release
# script and its manifest, because the benchmark used to be the only thing here.
Write-Host "`nrelease" -ForegroundColor Cyan
# The release script runs, and its manifest verifies.
#
# A checksum file is the integrity guarantee an unsigned build leans on, so it
# has to actually verify on the machine of whoever is checking it: LF endings,
# no BOM, and the exact two-space format `sha256sum -c` expects. A file that
# looks right in an editor and fails at the other end is worse than none.
#
# It said "the ONLY integrity guarantee" until 2026-08-06, and that stopped
# being true on 2026-08-05: the release workflow now signs the combined
# SHA256SUMS.txt with the Ed25519 release key, so a download can be traced to
# whoever holds that key and not merely to whoever served the page. The two
# guarantees are different and both matter. What release.ps1 writes here is
# still the unsigned per-archive manifest, because the signature is made in the
# publish job over the cross-platform one, after all three legs have finished.
#
# CODE signing still cannot be checked here — see docs/RELEASING.md — because it
# needs credentials issued to a person. The manifest signature can be and is,
# but in the release workflow rather than in this gate, for the same reason:
# the private half is a GitHub Actions secret and is on no machine that runs
# this file.
#
# NO LONGER GATED ON PYTHON. This step used to carry `{ Have python }`, and its
# own comment recorded the cost: on a Rust-only machine the whole thing SKIPPED,
# so the manifest was never checked at all, because one probe at the end of it
# needed an interpreter. That probe is now a step of its own with its own
# precondition, which is what it should always have been — the release output is
# built once here and the steps that follow reuse it, so splitting costs one
# variable and no build time.
$script:release = $null
$script:releaseFiles = @()

# EVERY FILE release.ps1 READS MUST BE TRACKED BY GIT.
#
# A release is cut from a fresh checkout on a runner, so a file that exists on
# this machine but is not in the repository is a file that will not be there.
# `release.ps1` throws when a notice source is missing -- which is right, and
# which happens twenty minutes after a tag that cannot be un-pushed.
#
# This is not hypothetical. `README-LINUX.txt` and `README-MACOS.txt` were first
# written to `tools/dist/`, and `.gitignore:11` is a bare `dist/` -- meant for a
# Node build directory, with no leading slash, so it matches at EVERY depth.
# `git check-ignore` confirmed both files were ignored: they would have been
# invisible to `git add`, absent from the runner's checkout, and the Linux and
# macOS jobs would have died on that throw while Windows -- whose readme lives
# under `tools/installer/` -- sailed through. Moving them to `tools/readme/`
# fixed those two files. This stops the next one.
#
# The paths are READ OUT OF release.ps1 rather than repeated here. Any string
# literal in that script naming a file that exists is a file the release depends
# on, so this check has no list of its own to drift.
#
# PARSED WITH THE POWERSHELL PARSER, NOT WITH A REGEX. The obvious
# `'([^']+)'` finds nine strings in a file that has a hundred and twenty,
# because release.ps1's prose is full of apostrophes -- "cargo's", "the user's",
# "PowerShell's" -- and every one of them shifts the pairing, so most of the
# real literals end up inside a "string" that started at a comment. The floor
# below is what caught that; without it this step would have reported ok while
# checking one file. A language has a parser; use it.
Step 'every file the release reads is committed' {
    $errs = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(
        (Join-Path $PSScriptRoot 'release.ps1'), [ref]$null, [ref]$errs)
    if ($errs) { throw "release.ps1 does not parse: $($errs[0])" }
    $paths = @($ast.FindAll(
            { param($n) $n -is [System.Management.Automation.Language.StringConstantExpressionAst] }, $true) |
        ForEach-Object { $_.Value } |
        Sort-Object -Unique |
        Where-Object { $_ -match '^[\w][\w./-]*$' } |
        # -PathType Leaf, so 'dist' and 'target/release' fall out rather than
        # being asked whether a directory is tracked.
        Where-Object { Test-Path -LiteralPath (Join-Path $repo $_) -PathType Leaf })

    # A floor, because a regex that stopped matching would enumerate nothing and
    # report success. Twelve notices, three installer files, the icon, two
    # readmes and Cargo.toml is nineteen.
    if ($paths.Count -lt 12) {
        throw "only $($paths.Count) input file(s) found in release.ps1; this step parsed almost nothing and proved nothing"
    }

    $untracked = @()
    foreach ($p in $paths) {
        git ls-files --error-unmatch -- $p 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { $untracked += $p }
    }
    if ($untracked) {
        throw ("the release reads files git is not tracking, so they will not exist in a fresh checkout:`n        " +
               ($untracked -join "`n        ") +
               "`n        Check .gitignore -- a pattern with no leading slash matches at every depth.")
    }
    Write-Host "        $($paths.Count) input file(s), all tracked" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
} { Test-Path (Join-Path $repo '.git') }

Step 'release script and its manifest' {
    $out = Join-Path $env:TEMP ('pl-release-check-' + $PID)
    Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
    & "$PSScriptRoot/release.ps1" -Out $out -Quiet 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'release.ps1 failed' }
    $script:release = $out

    $m = Join-Path $out 'SHA256SUMS.txt'
    if (-not (Test-Path $m)) { throw 'no manifest was written' }
    $bytes = [System.IO.File]::ReadAllBytes($m)
    if ($bytes[0] -eq 0xEF) { throw 'the manifest has a BOM and will not verify' }
    if ($bytes -contains 0x0D) { throw 'the manifest has CRLF and will not verify' }
    # Pure ASCII. Windows PowerShell 5.1 reads a BOM-less script as ANSI, so
    # a non-ASCII character in one of the script's strings comes back out
    # double-encoded -- an em-dash in the dirty-tree warning reached the
    # manifest as three mojibake bytes, and nothing else here would have
    # noticed. A checksum file needs nothing ASCII cannot spell.
    foreach ($b in $bytes) {
        if ($b -gt 0x7F) { throw 'the manifest is not pure ASCII' }
    }
    $text = [System.Text.Encoding]::UTF8.GetString($bytes)
    $lines = $text -split "`n"
    $sep = [Array]::IndexOf($lines, '--')
    if ($sep -lt 4) { throw 'the manifest has no header' }
    $listed = @()
    foreach ($line in $lines[($sep + 1)..($lines.Length - 1)]) {
        if (-not $line) { continue }
        if ($line -notmatch '^[0-9a-f]{64}  \S+$') { throw "not a checksum line: $line" }
        $parts = $line -split '  ', 2
        $f = Join-Path $out $parts[1]
        if (-not (Test-Path -LiteralPath $f)) { throw "the manifest lists $($parts[1]), which is not in the release" }
        $actual = (Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $parts[0]) { throw "$($parts[1]) does not match its recorded hash" }
        $listed += $parts[1]
    }
    $script:releaseFiles = $listed

    # SET EQUALITY, not a floor and not a hardcoded list.
    #
    # This is the check that makes the licence obligation structural. Until
    # 2026-08-05 the manifest covered four binaries out of sixteen files, and
    # the eleven notice texts that four licences require to travel with every
    # copy had no integrity record and no gate. A second list here — "and these
    # eleven files must be present" — would be a fourth copy of `$notices` and
    # would drift from it exactly as `dist/` did, twice, on 2026-08-03 and
    # 2026-08-04. So nothing is enumerated: what is on disk must be what the
    # manifest says, in both directions, and `release.ps1` already refuses to
    # build a copy whose notice SOURCES are missing.
    #
    # The zip and its checksum sidecar are the two files that cannot be in the
    # manifest, because they are built from it.
    $onDisk = Get-ChildItem -LiteralPath $out -Recurse -File |
        ForEach-Object { $_.FullName.Substring($out.Length + 1).Replace('\', '/') } |
        Where-Object { $_ -ne 'SHA256SUMS.txt' -and $_ -notlike '*.zip' -and $_ -notlike '*.zip.sha256' }
    $unhashed = @($onDisk | Where-Object { $listed -notcontains $_ })
    if ($unhashed) { throw "shipped but not in the manifest: $($unhashed -join ', ')" }

    # A floor as well, because set equality alone is satisfied by a release of
    # nothing agreeing with a manifest of nothing. Twenty is what the current
    # `$artifacts` + `$notices` + installer produce; raise it when they grow.
    #
    # It read sixteen until 2026-08-05, by which time the release had been
    # eighteen files for a while: the floor had become two files of slack rather
    # than a floor, which is the same drift it exists to catch. Nineteen was
    # those eighteen plus polylinker.ico, which `release.ps1` began shipping that
    # day; twenty is nineteen plus LICENSE-MIT.txt, added on 2026-08-06 because
    # the repository had been offering `MIT OR Apache-2.0` while shipping only
    # the Apache half.
    if ($listed.Count -lt 20) { throw "only $($listed.Count) file(s) in the manifest; at least 20 are expected" }

    # And the specific obligation named in NOTICE, spelled out once here because
    # this is the assertion whose failure is a licence violation rather than a
    # papercut. Counted, not listed, for the reason above.
    $lic = @($listed | Where-Object { $_ -like 'licences/*' })
    if ($lic.Count -lt 7) { throw "only $($lic.Count) font licence text(s) shipped; NOTICE requires 7" }
    foreach ($required in 'NOTICE.txt', 'LICENSE.txt', 'LICENSE-MIT.txt', 'features/NOTICE.txt') {
        if ($listed -notcontains $required) { throw "$required did not ship" }
    }

    $zip = Get-ChildItem -LiteralPath $out -Filter '*.zip'
    if (-not $zip) { throw 'no Windows zip was produced' }
    if (-not (Test-Path "$($zip.FullName).sha256")) { throw 'the zip has no checksum sidecar' }

    Write-Host "        $($listed.Count) file(s) hashed, $($lic.Count) licence texts, manifest verified" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

# Read a PE image's resource directory straight out of the bytes on disk:
# MZ -> PE signature -> optional header (PE32 or PE32+) -> data directory entry
# 2 -> RVA translated through the section table -> the three-level
# type/name/language tree. One object per leaf, with its bytes.
#
# A BYTE SCAN, NOT `Add-Type`, `System.Drawing` or `dumpbin`, for the reason the
# CRT step above gives in full: it needs nothing installed, so it runs on a
# Rust-only machine and on a CI runner. It also reads what the LINKER put in the
# file rather than what Windows would make of it, which is the property the step
# below is about -- `LoadImage` succeeding proves the icon is loadable, not that
# it is the icon in this repository.
function Get-PeResources($Path) {
    $b = [System.IO.File]::ReadAllBytes($Path)
    $u16 = { param($o) [BitConverter]::ToUInt16($b, $o) }
    $u32 = { param($o) [BitConverter]::ToUInt32($b, $o) }

    if ((& $u16 0) -ne 0x5A4D) { throw "$Path is not a PE image (no MZ)" }
    $peOff = & $u32 0x3C
    if ((& $u32 $peOff) -ne 0x00004550) { throw "$Path has no PE signature" }
    $coff = $peOff + 4
    $nSections = & $u16 ($coff + 2)
    $optSize = & $u16 ($coff + 16)
    $opt = $coff + 20
    # 0x20B = PE32+; the data directory sits 16 bytes further in, because
    # ImageBase and three of the size fields are 8 bytes rather than 4.
    $dirOff = if ((& $u16 $opt) -eq 0x20B) { $opt + 112 } else { $opt + 96 }
    $resRva = & $u32 ($dirOff + 2 * 8)
    if ($resRva -eq 0) { throw "$Path carries no resource directory at all" }

    $sections = @()
    for ($i = 0; $i -lt $nSections; $i++) {
        $s = $opt + $optSize + $i * 40
        $sections += [pscustomobject]@{
            VA = & $u32 ($s + 12); VSz = & $u32 ($s + 8)
            Raw = & $u32 ($s + 20); RSz = & $u32 ($s + 16)
        }
    }
    $toFile = {
        param($rva)
        foreach ($s in $sections) {
            if ($rva -ge $s.VA -and $rva -lt ($s.VA + [Math]::Max($s.VSz, $s.RSz))) {
                return $s.Raw + ($rva - $s.VA)
            }
        }
        throw "RVA 0x$('{0:X}' -f $rva) is in no section of $Path"
    }

    $resBase = & $toFile $resRva
    # A List rather than an array, and `.Add` rather than `+=`: `&` runs a
    # scriptblock in a CHILD scope, so `$found += ...` inside `$walk` would
    # create a local `$found` and discard every leaf it found. Mutating an
    # object has no such problem.
    $found = [System.Collections.Generic.List[object]]::new()
    $walk = {
        param($off, $level, $type, $name)
        $n = (& $u16 ($off + 12)) + (& $u16 ($off + 14))   # named + id entries
        for ($i = 0; $i -lt $n; $i++) {
            $e = $off + 16 + $i * 8
            $id = & $u32 $e
            $ptr = & $u32 ($e + 4)
            $child = $resBase + ($ptr -band 0x7FFFFFFF)
            if (($ptr -band 0x80000000) -ne 0) {
                # Level 0 is the type, level 1 the name, level 2 the language.
                if ($level -eq 0) { & $walk $child 1 $id $null }
                else { & $walk $child 2 $type $id }
            } else {
                $size = & $u32 ($child + 4)
                $start = & $toFile (& $u32 $child)
                $found.Add([pscustomobject]@{
                    Type = $type; Name = $name; Size = $size
                    Bytes = $b[$start..($start + $size - 1)]
                })
            }
        }
    }
    & $walk $resBase 0 $null $null
    , $found.ToArray()
}

# The shipped binaries must carry the version block and the icon.
#
# WHY THIS IS ASSERTED AGAINST THE .EXE AND NOT AGAINST THE BUILD SCRIPT
#
# `bins/winres.rs` writes a `.res` by hand and hands it to `link.exe` through
# `cargo:rustc-link-arg-bin=<name>=...`. Cargo does not verify that `<name>`
# names a real binary target: a typo makes the whole thing a silent no-op, the
# build stays green, and the only symptom is an .exe with no version -- which is
# precisely the state this repository was in until 2026-08-05, when
# `(Get-Item polylinker.exe).VersionInfo` was empty on all three binaries and
# `dist\polylinker.exe` had no resource directory at all. So nothing here asks
# whether a build script ran. It reads the resource back out of the linked file.
#
# The version is read out of Cargo.toml with the same regex `release.ps1` uses,
# rather than typed here, because a literal in the gate would be the second copy
# of a number that is supposed to have exactly one.
#
# WHAT THIS DOES NOT CATCH: `polylinker.svg` being edited without rerunning
# `bins/pl-gui/icon/build-icon.py`. The .ico would be stale and the .exe would
# faithfully carry the stale frames -- both sides of every comparison below move
# together. Closing that means rasterising SVG in the gate, which needs Python
# and resvg, and this step is deliberately dependency-free. It is closed by the
# step 'the window icon is the .ico's own frame' in the oracles section, which
# renders the master and compares every frame against it; this one stays as it
# is so that a Rust-and-PowerShell-only machine still proves the .exe carries the
# .ico in the repository.
#
# NOR DOES IT SAY ANYTHING ABOUT THE RUNNING WINDOW. The resource this asserts is
# read by Explorer, the Start Menu shortcut and Add/Remove Programs; winit does
# not read it, and the taskbar button of a live window comes from
# `window_icon()` in bins/pl-gui/src/main.rs. That is the other step's subject
# too.
Step 'the built binaries carry their icon and version resource' {
    $repoIco = Join-Path $repo 'bins/pl-gui/icon/polylinker.ico'
    if (-not (Test-Path $repoIco)) {
        throw ("$repoIco is missing. " +
               'Run: python bins/pl-gui/icon/build-icon.py -- it regenerates the .ico from polylinker.svg.')
    }

    # One copy of the version, and it is Cargo.toml's -- see release.ps1.
    $version = ''
    foreach ($line in (Get-Content (Join-Path $repo 'Cargo.toml'))) {
        if ($line -match '^\s*version\s*=\s*"([^"]+)"') { $version = $Matches[1]; break }
    }
    if (-not $version) { throw 'could not read the version out of Cargo.toml' }

    # The seven fields Add/Remove Programs, the shell property sheet and any
    # inventory tool read. Empty is what they all were before.
    $fields = 'CompanyName', 'FileDescription', 'FileVersion',
              'LegalCopyright', 'OriginalFilename', 'ProductName', 'ProductVersion'
    foreach ($name in 'polylinker.exe', 'pl.exe') {
        $f = Join-Path $script:release $name
        if (-not (Test-Path $f)) { throw "$name is not in the release" }
        $vi = (Get-Item $f).VersionInfo
        foreach ($field in $fields) {
            if (-not $vi.$field) { throw "$name has an empty $field; the version resource did not reach it" }
        }
        if ($vi.OriginalFilename -ne $name) {
            throw "$name says its OriginalFilename is $($vi.OriginalFilename); the two build scripts have been crossed"
        }
        if ($vi.FileVersion -ne $version) {
            throw "$name reports FileVersion $($vi.FileVersion) but Cargo.toml says $version"
        }
        if ($vi.ProductVersion -ne $version) {
            throw "$name reports ProductVersion $($vi.ProductVersion) but Cargo.toml says $version"
        }
    }

    # The icon, frame by frame. The group must describe the same set the .ico
    # does, and each RT_ICON payload must be the .ico's own bytes: a group that
    # merely has the right number of entries would survive an icon rebuilt from
    # a different drawing.
    $res = Get-PeResources (Join-Path $script:release 'polylinker.exe')
    $grp = @($res | Where-Object { $_.Type -eq 14 })          # RT_GROUP_ICON
    if ($grp.Count -ne 1) { throw "polylinker.exe has $($grp.Count) RT_GROUP_ICON resource(s); exactly 1 is expected" }
    if (-not ($res | Where-Object { $_.Type -eq 16 })) { throw 'polylinker.exe carries no RT_VERSION' }

    $ico = [System.IO.File]::ReadAllBytes($repoIco)
    $gb = $grp[0].Bytes
    $count = [BitConverter]::ToUInt16($gb, 4)
    $icoCount = [BitConverter]::ToUInt16($ico, 4)
    if ($count -ne $icoCount) {
        throw "the group icon in polylinker.exe declares $count frame(s) but polylinker.ico has $icoCount"
    }
    for ($i = 0; $i -lt $count; $i++) {
        # GRPICONDIRENTRY is 14 bytes; ICONDIRENTRY is 16. The difference is the
        # 4-byte file offset becoming a 2-byte resource id.
        $id = [BitConverter]::ToUInt16($gb, 6 + $i * 14 + 12)
        $off = [BitConverter]::ToUInt32($ico, 6 + $i * 16 + 12)
        $size = [BitConverter]::ToUInt32($ico, 6 + $i * 16 + 8)
        $want = $ico[$off..($off + $size - 1)]
        $got = @($res | Where-Object { $_.Type -eq 3 -and $_.Name -eq $id })
        if ($got.Count -ne 1) { throw "the group names RT_ICON $id, and polylinker.exe has $($got.Count) of them" }
        if (Compare-Object $want $got[0].Bytes -SyncWindow 0) {
            throw "RT_ICON $id in polylinker.exe is not the corresponding frame of polylinker.ico"
        }
    }

    # And the shipped copy is the same file the binary was built from, so the
    # Start Menu shortcut and the running window cannot show two pictures.
    $shipped = Join-Path $script:release 'polylinker.ico'
    if (-not (Test-Path $shipped)) { throw 'polylinker.ico is not in the release, so the installer has no shortcut icon' }
    if (Compare-Object $ico ([System.IO.File]::ReadAllBytes($shipped)) -SyncWindow 0) {
        throw 'the polylinker.ico in the release differs from bins/pl-gui/icon/polylinker.ico'
    }

    # `pl` is a console tool. An icon on it would mean `bins/pl/build.rs` had
    # been handed the GUI's arguments.
    $plRes = Get-PeResources (Join-Path $script:release 'pl.exe')
    if (-not ($plRes | Where-Object { $_.Type -eq 16 })) { throw 'pl.exe carries no RT_VERSION' }
    if ($plRes | Where-Object { $_.Type -eq 14 }) {
        throw 'pl.exe carries a group icon; the two build scripts have been crossed'
    }

    Write-Host "        version $version on 2 binaries, $count icon frames byte-identical to polylinker.ico" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
} { $script:release }

# The Python extension must be shipped under a name CPython will load.
# A correctly built `polylinker.dll` cannot be imported on Windows at
# all, and the failure reads as "the wheel is broken" rather than as a
# naming problem — which is exactly how it presented when a smoke test
# first tried to import one.
Step 'shipped Python extension imports' {
    $py = Get-ChildItem $script:release -Filter 'polylinker.*' |
          Where-Object { $_.Extension -in '.pyd', '.so' }
    if (-not $py) { throw 'no importable Python extension in the release' }
    $probe = python -c "import importlib.util as u, sys; s = u.spec_from_file_location('polylinker', sys.argv[1]); m = u.module_from_spec(s); s.loader.exec_module(m); print(len(m.enzymes()))" $py.FullName 2>&1
    if ($LASTEXITCODE -ne 0) { throw "the shipped extension does not import: $probe" }
    Write-Host "        $probe enzymes" -ForegroundColor DarkGray
} { (Have python) -and $script:release }

# Nothing shipped may need a C runtime the user is not allowed to install.
#
# All four artifacts imported VCRUNTIME140.dll until 2026-08-05. That DLL is not
# part of Windows; it comes from the VC++ redistributable, whose installer needs
# administrator rights. `docs/PLAN.md:120` describes the primary user as someone
# who has none, so on a freshly imaged machine the app was a missing-DLL dialog.
# `.cargo/config.toml` links the CRT statically to fix it, at a measured cost of
# 521,216 bytes across all four binaries.
#
# This is exactly the kind of property that regresses invisibly: nothing behaves
# differently on a developer machine, which has the redistributable, and the
# failure only appears on a machine nobody here owns. So it is asserted.
#
# THE CHECK IS A BYTE SCAN, NOT `dumpbin`. An imported DLL's name is stored as a
# literal ASCII string in the PE import directory, so if the import exists the
# string is certainly there. The check asserts ABSENCE, so the only error this
# method can make is a false FAILURE from an incidental occurrence of the name —
# the safe direction, and one that would be investigated rather than ignored. It
# also needs nothing installed, so unlike `dumpbin` it runs on a Rust-only
# machine and on a CI runner. `dumpbin /dependents` was used to establish the
# baseline by hand and agrees: after the change, the imports are stock Windows
# DLLs plus python3.dll for the extension.
Step 'no C runtime redistributable is needed' {
    $banned = 'VCRUNTIME140.dll', 'VCRUNTIME140_1.dll', 'MSVCP140.dll', 'api-ms-win-crt-runtime-l1-1-0.dll'
    $checked = 0
    foreach ($f in (Get-ChildItem -LiteralPath $script:release -File |
                    Where-Object { $_.Extension -in '.exe', '.pyd', '.dll' })) {
        $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
        $ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
        foreach ($b in $banned) {
            if ($ascii.IndexOf($b, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
                throw "$($f.Name) references $b, which is not part of Windows and whose redistributable needs admin rights. Check .cargo/config.toml still sets +crt-static."
            }
        }
        $checked++
    }
    if ($checked -lt 4) { throw "only $checked binary(ies) checked; expected at least 4" }
    Write-Host "        $checked binaries, none needs the VC++ redistributable" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
} { $script:release }

Write-Host "`ninstaller" -ForegroundColor Cyan

# The PATH-editing functions, tested where they live.
#
# They are the only pure logic in the installer and the only part that can
# damage something that was not ours: a PATH is a shell accumulated over years,
# and the failure mode is not "the install failed" but "half my tools stopped
# resolving". The cases include the one that actually occurs on this machine —
# a user PATH ending in an unexpanded %USERPROFILE%, which a naive read/write
# round trip silently bakes into an absolute path.
Step 'installer PATH edits' {
    & "$PSScriptRoot/installer/Install-Polylinker.ps1" -SelfTest
}

# The installer's plan must name every file the release produced, and must not
# name a path outside the prefix it was given.
#
# The first half is the licence obligation again, at the third and last place it
# can be dropped: `release.ps1` builds the notices, the manifest hashes them,
# and this proves the installer actually copies them rather than a curated
# subset of them.
Step 'installer plan covers the whole release' {
    $prefix = Join-Path $env:TEMP ('pl-install-plan-' + $PID)
    # A CHILD PROCESS, not `& script.ps1`.
    #
    # Two reasons, both of which bit. The plan is printed with `Write-Host`,
    # which writes to the host rather than to the pipeline, so an in-process
    # call captured an empty string and this step cheerfully reported that the
    # installer would not install any of eighteen files. And `Out-String` wraps
    # at the console width, so even once captured, every long temp path arrived
    # split across two lines and `features\NOTICE.txt` could not be found in it.
    #
    # A child process with a wide `-Width` fixes both, and it is what a user
    # actually runs, so the exit code means what it says.
    $host_ = (Get-Process -Id $PID).Path
    $out = & $host_ -NoProfile -File "$PSScriptRoot/installer/Install-Polylinker.ps1" `
        -DryRun -Prefix $prefix -Source $script:release `
        -RegistryRoot "HKCU\Software\Polylinker-CI-$PID" `
        -StateDir (Join-Path $env:TEMP "pl-state-$PID") `
        -StartMenuDir (Join-Path $env:TEMP "pl-startmenu-$PID") `
        -AddToPath -Associate 2>&1 | Out-String -Width 4096
    if ($LASTEXITCODE -ne 0) { throw "the dry run failed:`n$out" }
    if (-not $out.Trim()) { throw 'the dry run printed nothing, so this step would assert nothing' }
    if (Test-Path $prefix) { throw 'a dry run created the install directory' }

    foreach ($f in $script:releaseFiles) {
        $leaf = Split-Path -Leaf $f
        if ($out -notmatch [regex]::Escape($leaf)) {
            throw "the installer plan never mentions $f, so it would not be installed"
        }
    }
    # Every destination the plan writes to must be inside the prefix. The Start
    # Menu shortcut is the one deliberate exception, and it prints as `shortcut`
    # rather than as one of these three verbs.
    #
    # `\s{2,}` after the verb, not `\s+`: the plan pads its action column, but
    # the prose in the "It will NOT" section is single-spaced, and a looser
    # pattern read the sentence "will not write outside the paths listed above"
    # as a plan line writing to a directory called "outside the paths listed
    # above". The prose was reworded too -- a test that depends on prose not
    # containing a keyword is a test waiting to break -- but the column format
    # is the real discriminator and this is what should have keyed on it.
    foreach ($line in ($out -split "`r?`n")) {
        if ($line -match '^\s{2}(copy|write|create dir)\s{2,}(\S.*?)(\s+\(|$)') {
            $dest = $Matches[2].Trim()
            if (-not $dest.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
                throw "the plan writes outside the prefix: $dest"
            }
        }
    }
    # And the promises the product is built on.
    foreach ($promise in 'contact the network', 'install an updater') {
        if ($out -notmatch [regex]::Escape($promise)) { throw "the plan no longer states that it will not $promise" }
    }
    Write-Host "        $($script:releaseFiles.Count) file(s) in the plan, none outside the prefix" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
} { $script:release }

# A real install, into a scratch prefix and a scratch registry root, then a real
# uninstall — with a sentinel planted in the state directory that must survive.
#
# The sentinel is the point. `recovery\*.recover` is unsaved user work rescued
# from a crash, and an uninstaller that removes it has destroyed the only copy
# of somebody's afternoon. That is a promise in prose in three files; this is
# the only thing that makes it a promise in fact.
Step 'installer round trip leaves user state alone' {
    $tag = "pl-rt-$PID"
    $prefix = Join-Path $env:TEMP "$tag-prefix"
    $state  = Join-Path $env:TEMP "$tag-state"
    $menu   = Join-Path $env:TEMP "$tag-menu"
    $regRoot = "HKCU\Software\Polylinker-CI-$PID"
    $regProv = "HKCU:\Software\Polylinker-CI-$PID"
    try {
        Remove-Item -Recurse -Force $prefix, $state, $menu -ErrorAction SilentlyContinue
        Remove-Item -Recurse -Force $regProv -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Force (Join-Path $state 'recovery') | Out-Null
        New-Item -ItemType Directory -Force (Join-Path $state 'index') | Out-Null
        'unsaved work'    | Set-Content (Join-Path $state 'recovery\9999-0.recover')
        'panel_width: 300' | Set-Content (Join-Path $state 'layout')
        'cache'            | Set-Content (Join-Path $state 'index\demo.plx')

        # A handler that was already there, exactly as an installed SnapGene
        # would be. `docs/PLAN.md:212` says taking this is what enrages the user
        # the project is courting, so it is asserted rather than trusted.
        $dna = "$regProv\Classes\.dna"
        New-Item -Path "$dna\OpenWithProgids" -Force | Out-Null
        New-ItemProperty -Path "$dna\OpenWithProgids" -Name 'SnapGene.Document' -PropertyType String -Value '' -Force | Out-Null
        New-ItemProperty -Path $dna -Name '(default)' -PropertyType String -Value 'SnapGene.Document' -Force | Out-Null

        # A HASHTABLE, not an array. Splatting an array passes its elements
        # positionally, and this script has no positional parameters, so
        # `@common` as an array died with "a positional parameter cannot be
        # found that accepts argument 'HKCU\Software\...'". Named splatting is
        # what was meant.
        $common = @{
            Prefix = $prefix; Source = $script:release; RegistryRoot = $regRoot
            StateDir = $state; StartMenuDir = $menu; Yes = $true
        }
        & "$PSScriptRoot/installer/Install-Polylinker.ps1" @common -AddToPath -Associate 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'the install failed' }

        foreach ($f in $script:releaseFiles) {
            $p = Join-Path $prefix ($f.Replace('/', '\'))
            if (-not (Test-Path -LiteralPath $p)) { throw "$f was not installed" }
        }
        if (-not (Test-Path (Join-Path $prefix 'install-receipt.txt'))) { throw 'no receipt was written' }
        if (-not (Test-Path (Join-Path $menu 'Polylinker.lnk')))        { throw 'no Start Menu shortcut' }
        if (-not (Test-Path "$regProv\Uninstall\Polylinker"))           { throw 'no Add/Remove Programs entry' }
        $stillSnapGene = (Get-ItemProperty -LiteralPath $dna).'(default)'
        if ($stillSnapGene -ne 'SnapGene.Document') { throw "the install took the .dna default: it is now '$stillSnapGene'" }

        & "$PSScriptRoot/installer/Install-Polylinker.ps1" -Uninstall -NoRelaunch @common 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'the uninstall failed' }

        if (Test-Path $prefix)                              { throw 'the install directory survived the uninstall' }
        if (Test-Path (Join-Path $menu 'Polylinker.lnk'))    { throw 'the Start Menu shortcut survived' }
        if (Test-Path "$regProv\Uninstall\Polylinker")       { throw 'the Add/Remove Programs entry survived' }
        if (Test-Path "$regProv\Classes\Polylinker_dna.1")   { throw 'a ProgId survived' }
        $left = (Get-Item -LiteralPath "$dna\OpenWithProgids" -ErrorAction SilentlyContinue).Property
        if ($left -contains 'Polylinker_dna.1') { throw 'a dangling OpenWithProgids entry survived' }
        if ($left -notcontains 'SnapGene.Document') { throw "the uninstall removed somebody else's registration" }

        # THE ONE THAT MATTERS.
        if (-not (Test-Path (Join-Path $state 'recovery\9999-0.recover'))) {
            throw 'THE UNINSTALL DELETED AN UNSAVED CRASH DRAFT'
        }
        if (-not (Test-Path (Join-Path $state 'layout')))        { throw 'the uninstall deleted the settings' }
        if (-not (Test-Path (Join-Path $state 'index\demo.plx'))) { throw 'the uninstall deleted the cache without -RemoveCache' }

        Write-Host '        installed, verified, uninstalled; user state intact' -ForegroundColor DarkGray
        $global:LASTEXITCODE = 0
    } finally {
        Remove-Item -Recurse -Force $prefix, $state, $menu -ErrorAction SilentlyContinue
        Remove-Item -Recurse -Force $regProv -ErrorAction SilentlyContinue
    }
} { $script:release }

# The product's central claim, enforced mechanically.
#
# README.md's first line is "Never sends a sequence anywhere"; RELEASING.md's
# "There is no auto-updater, on purpose" lists the four bars any updater here
# must clear. An installer is part of the product, and prose in a doc does not
# stop anybody adding a version check to a script. This does.
#
# UNCHANGED BY the update check added on 2026-08-06, and that is the point of
# keeping it. `pl update` and the editor's off-by-default switch are things a
# person invokes; an installer reaching the network is a thing that happens to
# them. This gate is what keeps the second from arriving on the coat-tails of
# the first, so it got stricter in spirit rather than looser.
Step 'the installer contacts nothing' {
    $banned = @(
        'Invoke-WebRequest', 'Invoke-RestMethod', 'Start-BitsTransfer', 'System.Net',
        'WebClient', 'HttpClient', 'curl.exe', 'schtasks', 'Register-ScheduledTask',
        'New-Service', 'DownloadFile', 'DownloadString'
    )
    $hits = @()
    foreach ($f in (Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'installer') -File)) {
        $text = [System.IO.File]::ReadAllText($f.FullName)
        foreach ($b in $banned) {
            if ($text -match [regex]::Escape($b)) { $hits += "$($f.Name): $b" }
        }
    }
    if ($hits) {
        throw "the installer can reach the network or schedule work:`n    $($hits -join "`n    ")`nSee docs/RELEASING.md, 'There is no auto-updater, on purpose' -- the installer inherits that rule."
    }
    Write-Host "        $($banned.Count) forbidden facilities, none present" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

# The zip must be a deterministic function of the directory it was built from.
#
# NOT "two builds of the same commit produce the same zip". They cannot, and
# docs/RELEASING.md, "Reproducibility", declines to claim it and gives one
# reason: the build embeds absolute paths. There is a second reason it does not
# give -- a PE file carries a link timestamp, so a second `cargo build` relinks
# `polylinker.pyd` into different bytes. Asserting that would be asserting a
# property the project explicitly does not claim, and the first version of this
# step did exactly that and failed on the `.pyd` — correctly.
#
# What IS in this project's gift is the packaging step: given the same bytes on
# disk, the zip must come out the same. That reduces to three properties, and
# `Compress-Archive` fails the first two, which is why the zip is written by
# hand in release.ps1.
Step 'the zip is a deterministic function of dist/' {
    $zip = Get-ChildItem -LiteralPath $script:release -Filter '*.zip'
    if (-not $zip) { throw 'no zip was produced' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem | Out-Null
    $z = [System.IO.Compression.ZipFile]::OpenRead($zip.FullName)
    try {
        $names = @($z.Entries | ForEach-Object { $_.FullName })
        if ($names.Count -lt 16) { throw "the zip has only $($names.Count) entries" }

        # 1. Entry order is sorted, so it does not depend on the order the
        #    filesystem happened to enumerate.
        $sorted = @($names | Sort-Object)
        for ($i = 0; $i -lt $names.Count; $i++) {
            if ($names[$i] -ne $sorted[$i]) { throw "the zip entries are not in sorted order (first difference: $($names[$i]))" }
        }

        # 2. Every timestamp is pinned, so the same bytes on two days produce the
        #    same zip. This is the one Compress-Archive cannot satisfy: it stores
        #    each file's current mtime.
        #    Compared as a WALL CLOCK, not as an instant. A zip stores an MS-DOS
        #    date and time with no timezone at all, so `LastWriteTime` reads
        #    back as that same wall clock wearing the reader's local offset:
        #    on this machine the pinned midnight comes back as
        #    `2000-01-01 00:00:00 +02:00`, whose UtcDateTime is the 31st of
        #    December. Comparing the instant therefore fails everywhere except
        #    UTC, which is a bug in the test and not in the zip -- the bytes on
        #    disk are identical either way.
        $pinned = [DateTime]::new(2000, 1, 1, 0, 0, 0)
        foreach ($e in $z.Entries) {
            if ($e.LastWriteTime.DateTime -ne $pinned) {
                throw "$($e.FullName) carries a live timestamp ($($e.LastWriteTime)), so the zip hash would change on every build"
            }
        }

        # 3. Everything is under one top-level directory named for the version,
        #    so extracting does not scatter sixteen files into Downloads.
        $roots = @($names | ForEach-Object { ($_ -split '/')[0] } | Sort-Object -Unique)
        if ($roots.Count -ne 1) { throw "the zip has $($roots.Count) top-level entries: $($roots -join ', ')" }

        # And the contents really are the release, byte for byte.
        foreach ($e in $z.Entries) {
            $rel = $e.FullName.Substring($roots[0].Length + 1)
            $onDisk = Join-Path $script:release ($rel.Replace('/', '\'))
            if (-not (Test-Path -LiteralPath $onDisk)) { throw "the zip contains $rel, which is not in the release directory" }
            if ((Get-Item -LiteralPath $onDisk).Length -ne $e.Length) { throw "$rel differs between the zip and the release directory" }
        }
        Write-Host "        $($names.Count) entries, sorted, pinned timestamps, one root '$($roots[0])'" -ForegroundColor DarkGray
        $global:LASTEXITCODE = 0
    } finally { $z.Dispose() }
} { $script:release }

# The archive, checked the way a downloader receives it rather than the way this
# machine built it.
#
# The step above proves the zip is a deterministic function of dist/. This proves
# the zip's own SHA256SUMS.txt verifies against the bytes inside the zip, and
# that the licence set is in there by name and by count. Between those two
# statements sits the packaging step, and the packaging step is where seven
# licence texts went missing on 2026-07-30.
#
# The same script runs on all three runners in .github/workflows/release.yml,
# against the archive that will actually be attached to the release, so this is
# a local rehearsal of a check that also happens where it matters.
Step 'the zip verifies against its own manifest' {
    $zip = Get-ChildItem -LiteralPath $script:release -Filter '*.zip'
    if (-not $zip) { throw 'no zip was produced' }
    & "$PSScriptRoot/check-archive.ps1" -Archive $zip.FullName
} { $script:release }

if ($script:release) { Remove-Item -Recurse -Force $script:release -ErrorAction SilentlyContinue }

Write-Host "`nrelease workflow" -ForegroundColor Cyan

# THE UNIX ARCHIVE, BUILT AND CHECKED ON THIS MACHINE.
#
# `release.ps1` writes a tar.gz on Linux and macOS -- ustar headers and gzip,
# both hand-written, for the same determinism reason the zip is hand-written and
# additionally because a zip has no portable place to record a file mode, so
# every Unix user would begin with `chmod +x`.
#
# None of that code would otherwise run anywhere anybody looks. This gate is the
# only thing that runs on a developer machine, this machine is Windows, and a
# tar writer whose only exercise is a green job on a runner is a tar writer
# nobody has read the output of. So `-ArchiveFormat` exists and this step forces
# it: the payload is the Windows one, which is not the point -- the container is.
#
# Checked twice. `check-archive.ps1` reads the tar with this project's own
# reader, and then `tar.exe` reads it with somebody else's, which is the same
# argument the oracle steps above make. Windows 11 ships bsdtar as
# System32\tar.exe and Git for Windows ships GNU tar; either satisfies the
# precondition, and between them they disagree about enough of the format to be
# worth having.
Step 'the tar.gz writer produces an archive two other tools can read' {
    $out = Join-Path $env:TEMP ('pl-tar-check-' + $PID)
    Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
    try {
        & "$PSScriptRoot/release.ps1" -Out $out -ArchiveFormat tar.gz -Quiet 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'release.ps1 -ArchiveFormat tar.gz failed' }
        $tar = Get-ChildItem -LiteralPath $out -Filter '*.tar.gz'
        if (-not $tar) { throw 'no tar.gz was produced' }

        & "$PSScriptRoot/check-archive.ps1" -Archive $tar.FullName
        if ($LASTEXITCODE -ne 0) { throw 'the tar.gz failed its own archive check' }

        # An independent reader. `-t` alone would accept a header this project
        # and that tool happen to misread the same way, so the payload is
        # extracted and three files are compared byte for byte against dist/.
        #
        # RELATIVE PATHS, from inside $out. GNU tar reads `host:path` as an rsh
        # target, so handing it `C:\Users\...\x.tar.gz` makes it try to resolve a
        # machine called C and fail with "Cannot connect to C: resolve failed" --
        # which is what it did here, while bsdtar in System32 accepted the same
        # archive happily. Two readers disagreeing about the ARGUMENT is not
        # evidence about the format, so neither gets a drive letter.
        $tool = (Get-Command tar.exe -ErrorAction SilentlyContinue).Source
        New-Item -ItemType Directory -Force (Join-Path $out 'extracted') | Out-Null
        Push-Location $out
        try {
            & $tool -xzf $tar.Name -C extracted
        } finally { Pop-Location }
        if ($LASTEXITCODE -ne 0) { throw "$tool refused the archive" }
        $root = Get-ChildItem -LiteralPath (Join-Path $out 'extracted') -Directory
        if ($root.Count -ne 1) { throw "extracting produced $($root.Count) top-level directories" }
        foreach ($probe in 'SHA256SUMS.txt', 'licences/Phosphor-MIT.txt', 'features/NOTICE.txt') {
            $a = Join-Path $out ($probe.Replace('/', '\'))
            $b = Join-Path $root[0].FullName ($probe.Replace('/', '\'))
            if (-not (Test-Path -LiteralPath $b)) { throw "$probe did not survive a round trip through $tool" }
            $ha = (Get-FileHash -LiteralPath $a -Algorithm SHA256).Hash
            $hb = (Get-FileHash -LiteralPath $b -Algorithm SHA256).Hash
            if ($ha -ne $hb) { throw "$probe came back out of the tar with different bytes" }
        }
        Write-Host "        tar.gz read by check-archive.ps1 and by $tool, three files compared byte for byte" -ForegroundColor DarkGray
        $global:LASTEXITCODE = 0
    } finally {
        Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
    }
} { Have tar.exe }

# The release workflow, checked without running it.
#
# Nothing here proves the workflow WORKS -- only a pushed tag does that, and the
# first one will be the first time these three jobs have ever executed. What can
# be established from a checkout is that the file is well-formed YAML, that it
# covers three operating systems rather than the one this machine is, and that
# every path it names exists. A typo in a runner label is a green two-platform
# release; a typo in a path is a red job twenty minutes after a tag that cannot
# be taken back.
#
# PyYAML, because PowerShell has no YAML parser and a regex is not a parse. The
# step below does the content assertions with no interpreter at all, so a
# Rust-only machine still checks something rather than skipping the subject.
Step 'no private key material is anywhere in the tree' {
    # WHY THIS LOOKS AT CONTENT AND NOT AT FILENAMES.
    #
    # .gitignore now lists *.pem, *.key and /scratchpad/, and that is worth
    # having, but it is the weaker half. An ignore rule stops a file being
    # STAGED; it does nothing about one already tracked, and it is keyed on a
    # name the author chooses. The release signing key does not have to be
    # called anything in particular to be a release signing key.
    #
    # This exists because of a real near-miss on 2026-08-05: the Ed25519 release
    # private key was written into scratchpad/ for about thirty seconds during a
    # signing experiment, in a session where `git add -A` ran repeatedly.
    # Nothing was committed only because that directory happened to be younger
    # than the last such run.
    #
    # So every tracked file is read and matched against what key material
    # actually looks like, whatever it is called.
    $problems = @()
    # THE MARKERS ARE ASSEMBLED, NOT WRITTEN OUT, and that is not squeamishness.
    #
    # The first version spelled them literally, and the first thing it caught was
    # tools/ci.ps1 -- itself. A scanner that contains the string it scans for is
    # a scanner that fails on a clean tree, and the way that gets resolved in
    # practice is by excluding the scanner from the scan, which is how the one
    # file most likely to be edited carelessly becomes the one file nobody
    # checks. This project already learned the same lesson on the
    # release-workflow checker, which had to be taught to strip comments before
    # it stopped failing on the sentence describing the rule it enforces.
    #
    # Splitting the delimiter means no file is exempt, including this one, while
    # the assembled pattern still matches a genuine key block anywhere.
    $b = '-----' + 'BEGIN '
    $e = '-----'
    $markers = @(
        @{ Pattern = $b + '[A-Z ]*PRIVATE KEY' + $e;    What = 'a PEM private key block' }
        @{ Pattern = $b + 'OPENSSH PRIVATE KEY' + $e;   What = 'an OpenSSH private key' }
        @{ Pattern = $b + 'PGP PRIVATE KEY BLOCK' + $e; What = 'a PGP private key' }
    )
    # Tracked files only: an untracked scratch file is the author's business,
    # and ignoring it is what .gitignore is for.
    $tracked = & git -C $repo ls-files
    foreach ($rel in $tracked) {
        $p = Join-Path $repo $rel
        if (-not (Test-Path -LiteralPath $p)) { continue }
        $fi = Get-Item -LiteralPath $p
        if ($fi.Length -gt 2MB) { continue }   # a key is small; a corpus is not
        $text = Get-Content -LiteralPath $p -Raw -ErrorAction SilentlyContinue
        if (-not $text) { continue }
        foreach ($m in $markers) {
            if ($text -match $m.Pattern) { $problems += "$rel contains $($m.What)" }
        }
    }

    # scratchpad/ must stay untracked. It is where oracle virtualenvs and
    # downloaded vector suites live, and where key material passes through
    # during signing experiments. The .gitignore entry is what keeps it out;
    # this is what notices if it ever got in anyway.
    $inScratch = @($tracked | Where-Object { $_ -like 'scratchpad/*' -or $_ -eq 'scratchpad' })
    if ($inScratch.Count) {
        $problems += "scratchpad/ is tracked by git ($($inScratch.Count) file(s)); it holds oracle virtualenvs and, transiently, key material"
    }

    if ($problems) { throw ($problems -join "`n        ") }
    Write-Host "        $($tracked.Count) tracked files scanned; no private key material, scratchpad untracked" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

Step 'the MSI is generated from the manifest and not from a second file list' {
    # THE ONE CHECK THAT JUSTIFIES SHIPPING AN MSI AT ALL.
    #
    # docs/RELEASING.md refused a compiled installer on the grounds that every
    # one of them carries a second list of files that drifts from the real
    # payload -- and that is not hypothetical here: the notices list in
    # release.ps1 drifted twice in one week, on 2026-08-03 and 2026-08-04, and a
    # licence text stopped shipping each time.
    #
    # So tools/installer/Polylinker.wxs contains no files, and this step proves
    # it: it regenerates the payload fragment and asserts the component set is
    # exactly the manifest minus the three deliberate exclusions.
    # FIRST, THE ASSERTION THIS STEP'S NAME ACTUALLY PROMISES.
    #
    # The comparison below was, in its first version, the whole step -- and it
    # could not fail. It compared the manifest against a fragment that
    # build-msi.ps1 had generated FROM the manifest four lines earlier, using the
    # same parser. Two lists derived from one source agree by construction. It
    # would have passed just as happily against a Polylinker.wxs stuffed with
    # hand-written <File> elements, which is the exact thing the step exists to
    # forbid. So the authoring file is checked first, and directly.
    $wxsRaw = Get-Content -LiteralPath "$repo/tools/installer/Polylinker.wxs" -Raw
    $wxsCode = [regex]::Replace($wxsRaw, '(?s)<!--.*?-->', '')
    if ($wxsCode -match '<File\b') {
        throw 'Polylinker.wxs contains a <File> element. The MSI payload must come from dist/SHA256SUMS.txt through build-msi.ps1, so that a licence text cannot stop shipping without the manifest saying so.'
    }
    if ($wxsCode -match '<ComponentGroup\b(?!Ref)') {
        throw 'Polylinker.wxs defines a <ComponentGroup>. It may only reference the generated one with <ComponentGroupRef Id="PayloadComponents" />.'
    }
    if ($wxsCode -notmatch '<ComponentGroupRef\s+Id="PayloadComponents"') {
        throw 'Polylinker.wxs does not reference the generated PayloadComponents group, so the MSI would ship no payload at all'
    }

    $dist = "$repo/dist"
    $out = Join-Path ([IO.Path]::GetTempPath()) ("pl-msi-gen-" + [IO.Path]::GetRandomFileName())
    try {
        & "$PSScriptRoot/build-msi.ps1" -Dist $dist -Out $out -GenerateOnly -KeepIntermediate | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'build-msi.ps1 -GenerateOnly failed' }

        $frag = Get-Content -LiteralPath (Join-Path $out 'Payload.wxs') -Raw
        $inWxs = [regex]::Matches($frag, '!\(bindpath\.payload\)([^"]+)"') |
                 ForEach-Object { $_.Groups[1].Value -replace '\\', '/' } | Sort-Object

        $exclude = @('SHA256SUMS.txt', 'Install-Polylinker.ps1', 'Install.cmd')
        $inManifest = @()
        $past = $false
        foreach ($line in Get-Content -LiteralPath (Join-Path $dist 'SHA256SUMS.txt')) {
            if (-not $past) { if ($line.Trim() -eq '--') { $past = $true }; continue }
            if ($line -match '^[0-9a-f]{64}\s\s(.+)$') {
                $rel = $Matches[1].Trim()
                if ($exclude -notcontains $rel) { $inManifest += $rel }
            }
        }
        $inManifest = $inManifest | Sort-Object

        # EVERY GENERATED <File> MUST CARRY AN EXPLICIT Name.
        #
        # Without it WiX names the installed file after the last segment of
        # @Source, and @Source begins with !(bindpath.payload) with no separator
        # after it -- so the whole string is one segment and the file lands on
        # disk called "!(bindpath.payload)polylinker.exe". That shipped once. It
        # installed and uninstalled cleanly and passed every registry assertion,
        # because nothing about the registry is wrong in that state.
        $files = [regex]::Matches($frag, '<File\b[^>]*/>')
        foreach ($f in $files) {
            $t = $f.Value
            if ($t -notmatch 'Name="([^"]+)"') {
                throw "a generated <File> has no Name attribute, so WiX would name it after the unresolved bindpath: $t"
            }
            $name = $Matches[1]
            if ($t -notmatch 'Source="[^"]*[\\)]' + [regex]::Escape($name) + '"') {
                throw "a generated <File> has Name='$name' that does not match the leaf of its Source: $t"
            }
        }
        if ($files.Count -ne $inWxs.Count) {
            throw "$($files.Count) <File> elements but $($inWxs.Count) sources parsed; the fragment is malformed"
        }

        $onlyWxs = $inWxs | Where-Object { $inManifest -notcontains $_ }
        $onlyMan = $inManifest | Where-Object { $inWxs -notcontains $_ }
        if ($onlyWxs) { throw "the MSI would install files the manifest does not list: $($onlyWxs -join ', ')" }
        if ($onlyMan) { throw "the manifest lists files the MSI would not install: $($onlyMan -join ', ')" }
        if ($inWxs.Count -lt 12) { throw "only $($inWxs.Count) files in the generated payload; that is too few to be the real one" }

        # NEGATIVE CONTROL. The comparison above passes whenever two lists agree,
        # including when both are wrong in the same way, so the comparison itself
        # is exercised against a list known to differ. Without this the step
        # would be one more check that cannot fail.
        $doctored = $inManifest | Where-Object { $_ -ne 'polylinker.exe' }
        $shouldDiffer = $inWxs | Where-Object { $doctored -notcontains $_ }
        if (-not $shouldDiffer) {
            throw 'the set comparison did not notice a deliberately removed polylinker.exe, so it proves nothing'
        }

        Write-Host "        $($inWxs.Count) components, equal to the manifest; comparison verified against a doctored list" -ForegroundColor DarkGray
    } finally {
        Remove-Item -LiteralPath $out -Recurse -Force -ErrorAction SilentlyContinue
    }
    $global:LASTEXITCODE = 0
} { Test-Path "$repo/dist/SHA256SUMS.txt" }

Step 'the MSI takes no file type away from a program the reader already uses' {
    # WiX's <Extension> element writes an extension's DEFAULT value, which is
    # how an installer silently becomes the owner of .dna on a machine that has
    # SnapGene on it. Polylinker.wxs writes OpenWithProgids entries by hand
    # instead, and this step is what stops someone "simplifying" that later into
    # the element that looks tidier and behaves worse.
    $wxsPath = "$repo/tools/installer/Polylinker.wxs"
    if (-not (Test-Path $wxsPath)) { throw 'tools/installer/Polylinker.wxs is missing' }
    $raw = Get-Content -LiteralPath $wxsPath -Raw

    # WELL-FORMED XML FIRST, because wix will not tell you until CI does.
    #
    # This caught a real one on the day it was written: the file used ' -- ' as
    # an em-dash in its comments, following the house style of every .rs and
    # .ps1 file in the tree, and XML forbids '--' inside a comment. Ten of them.
    # The whole package would have failed to compile on the runner, several
    # minutes into a release, for a punctuation habit.
    try { [xml]$raw | Out-Null }
    catch { throw "Polylinker.wxs is not well-formed XML, so wix build cannot read it: $($_.Exception.Message)" }
    # Comments stripped first: this file explains at length why it does not use
    # <Extension>, and a checker that cannot tell code from prose gets silenced
    # rather than satisfied -- which the release-workflow checker already had to
    # learn once.
    $wxs = [regex]::Replace($raw, '(?s)<!--.*?-->', '')
    $problems = @()

    if ($wxs -match '<Extension\b') {
        $problems += 'Polylinker.wxs uses <Extension>, which writes the extension default and takes the file type from whatever the reader already has'
    }
    if ($wxs -match '<ProgId\b') {
        $problems += 'Polylinker.wxs uses <ProgId>, which writes the extension default for its child extensions'
    }
    foreach ($banned in 'ServiceInstall', 'ServiceControl', 'RemoteFile', 'DownloadUrl', 'ScheduledTask') {
        if ($wxs -match [regex]::Escape($banned)) {
            $problems += "Polylinker.wxs declares $banned; this installer copies files and writes registry values, nothing else"
        }
    }
    # The UpgradeCode is the product's permanent identity. A new one makes the
    # next release install ALONGSIDE this one instead of replacing it, and the
    # damage is invisible until that release ships.
    if ($wxs -notmatch 'UpgradeCode="78205503-D26C-4A6B-82DE-E0F834220A6D"') {
        $problems += 'the UpgradeCode is not the one minted on 2026-08-05; changing it makes future releases install alongside this one rather than upgrading it'
    }
    # .plproj must not be claimed. The GUI decides format by content through
    # pl_fileio, and nothing under crates/ knows the .plproj format -- it is a
    # bench file read by bins/pl-gui/src/session.rs from a menu. A double-click
    # reaches load_as() and fails.
    if ($wxs -match '\.plproj') {
        $problems += '.plproj is associated in Polylinker.wxs, but double-clicking one cannot work: load_as decides on content and no crate knows that format'
    }
    # And the associations that ARE there must be the additive kind.
    $owp = ([regex]::Matches($wxs, 'OpenWithProgids')).Count
    if ($owp -lt 8) { $problems += "only $owp OpenWithProgids entries; the installer associates eight extensions additively" }

    if ($problems) { throw ($problems -join "`n        ") }
    Write-Host "        no <Extension>, no default-handler theft, $owp additive associations, UpgradeCode pinned" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

Step 'the MSI installs, does what it says, uninstalls, and leaves nothing' {
    # The real oracle: msiexec against this machine, then the disk and the
    # registry are looked at.
    #
    # The per-USER pass is the one that always runs, because it needs no
    # elevation and because it is the scope readers get by default. The
    # per-machine pass is added only when the session happens to be elevated.
    # The step is skipped entirely without wix, since a workstation with no .NET
    # SDK cannot build an MSI to test and the other 62 steps are still worth
    # running there.
    $dist = "$repo/dist"
    $out = Join-Path ([IO.Path]::GetTempPath()) ("pl-msi-" + [IO.Path]::GetRandomFileName())
    try {
        & "$PSScriptRoot/build-msi.ps1" -Dist $dist -Out $out | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'build-msi.ps1 failed' }
        $msi = Get-ChildItem $out -Filter *.msi
        if ($msi.Count -ne 1) { throw "expected one .msi, found $($msi.Count)" }

        & "$PSScriptRoot/check-msi.ps1" -Msi $msi[0].FullName -Dist $dist -PerUser
        if ($LASTEXITCODE -ne 0) { throw 'the MSI failed its check in the per-user scope, which is the default one' }

        $elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
                    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
        if ($elevated) {
            & "$PSScriptRoot/check-msi.ps1" -Msi $msi[0].FullName -Dist $dist
            if ($LASTEXITCODE -ne 0) { throw 'the MSI failed its check in the per-machine scope' }
        } else {
            Write-Host '        per-machine pass skipped: this session is not elevated' -ForegroundColor DarkGray
        }
    } finally {
        Remove-Item -LiteralPath $out -Recurse -Force -ErrorAction SilentlyContinue
    }
    $global:LASTEXITCODE = 0
} {
    (Test-Path "$repo/dist/SHA256SUMS.txt") -and
    ($null -ne (Get-Command wix -ErrorAction SilentlyContinue))
}

Step 'the release workflow parses and covers three platforms' {
    # A here-string piped to `python -`, not a shell heredoc: `<<'PY'` is bash
    # and PowerShell has no such operator. Single-quoted, so nothing in the
    # Python below is interpolated on its way there.
    $prog = @'
import sys, yaml

wf = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))

# YAML 1.1 says the bare word `on` is a boolean, and PyYAML obeys, so the
# trigger block comes back under the key True rather than under "on". Reading
# wf["on"] here raises KeyError on a perfectly valid workflow -- which is the
# kind of bug that makes a checker look broken and get deleted.
triggers = wf.get("on", wf.get(True))
problems = []

tags = (triggers or {}).get("push", {}).get("tags") or []
if "v*" not in tags:
    problems.append(f"push.tags is {tags!r}; a v* tag is what cuts a release")
if "workflow_dispatch" not in (triggers or {}):
    problems.append("there is no workflow_dispatch trigger, so a release cannot be rehearsed without tagging")

if wf.get("permissions", {}).get("contents") != "read":
    problems.append("the workflow is not read-only by default")
if "RUSTFLAGS" in (wf.get("env") or {}):
    problems.append("RUSTFLAGS is set workflow-wide; a new warning would fail a release build of code that works")

jobs = wf.get("jobs") or {}
build = jobs.get("build")
include = []
if not build:
    problems.append("there is no build job")
else:
    include = build.get("strategy", {}).get("matrix", {}).get("include") or []
    runners = sorted(e.get("os") for e in include)
    want = ["macos-latest", "ubuntu-latest", "windows-latest"]
    if runners != want:
        problems.append(f"the matrix runs on {runners}, not {want}")
    labels = sorted(e.get("label") for e in include)
    if labels != ["linux-x64", "macos-universal", "windows-x64"]:
        problems.append(f"the platform labels are {labels}")
    if build.get("strategy", {}).get("fail-fast") is not False:
        problems.append("fail-fast is not disabled; one platform failing would hide the other two")

pub = jobs.get("publish")
if not pub:
    problems.append("there is no publish job")
else:
    if pub.get("needs") != "build":
        problems.append("publish does not depend on build")
    if pub.get("permissions", {}).get("contents") != "write":
        problems.append("publish cannot write a release")
    if "refs/tags/v" not in str(pub.get("if", "")):
        problems.append("publish is not gated on a tag, so workflow_dispatch would publish")

if problems:
    for p in problems:
        print("  " + p)
    sys.exit(1)
print(f"  parses; {len(include)} platforms, publish gated on a tag")
'@
    $prog | python - "$repo/.github/workflows/release.yml"
} { (HavePy 'yaml') -and (Test-Path "$repo/.github/workflows/release.yml") }

# What the workflow must NOT contain, and every file it must be able to find.
#
# THE FIRST HALF IS THE POINT. The licence obligation survives a three-platform
# release only because no platform assembles its own archive: all three run
# `tools/release.ps1`, whose `$notices` array is the single list, and which
# `tools/ci.ps1` exercises above on this machine. The moment a job in that YAML
# builds a tarball itself -- one `tar -czf` in a run: block, for the one
# platform where the script was inconvenient -- there is a second file list,
# in a language this gate cannot run, and the failure of 2026-07-30 has a new
# place to happen. So it is forbidden by name.
#
# No interpreter, so this runs everywhere the gate does.
Step 'the release workflow assembles nothing itself' {
    $wfPath = "$repo/.github/workflows/release.yml"
    if (-not (Test-Path $wfPath)) { throw 'there is no release workflow' }
    # COMMENTS STRIPPED FIRST. The workflow's own header explains why it packs
    # nothing itself, and does so by naming `tar -cf` -- so the first run of
    # this step failed on the sentence describing the rule it enforces. A
    # checker that cannot tell code from prose will be silenced rather than
    # satisfied. YAML comments start at `#` and run to end of line; a `#` inside
    # a quoted scalar would survive, and none of the banned strings below is one.
    $wf = (Get-Content -LiteralPath $wfPath | ForEach-Object { $_ -replace '(^|\s)#.*$', '' }) -join "`n"
    $problems = @()

    # Packaging facilities. `tar -c`, not `tar`, because the workflow may
    # legitimately LIST or extract one.
    foreach ($banned in 'Compress-Archive', 'tar -c', 'tar c', 'zip -r', '7z a', 'ditto -c') {
        if ($wf -match [regex]::Escape($banned)) {
            $problems += "the workflow packs an archive itself ($banned); the archive must come from tools/release.ps1 so the licence set has one source"
        }
    }
    # And it must actually call the two scripts.
    foreach ($required in 'tools/release.ps1', 'tools/check-archive.ps1') {
        $leaf = Split-Path -Leaf $required
        if ($wf -notmatch [regex]::Escape($leaf)) { $problems += "the workflow never runs $required" }
    }
    # Nothing is fetched at BUILD time, which is this step's subject and is not
    # the same question as what the shipped product does at run time. The
    # product's answer changed on 2026-08-06 -- `pl update` and an
    # off-by-default check in the editor -- and this list did not, because a
    # build that downloads a dependency mid-release is how something nobody
    # reviewed gets into an artifact everybody trusts. Same list the installer
    # is held to; docs/RELEASING.md has the reasoning.
    foreach ($banned in 'Invoke-WebRequest', 'DownloadString', 'curl -s http', 'wget ') {
        if ($wf -match [regex]::Escape($banned)) { $problems += "the workflow fetches something at build time ($banned)" }
    }

    # Every repository path the workflow names must exist. A release workflow
    # referring to a file that was renamed fails twenty minutes after a tag.
    # `(?:\./)?` because the workflow invokes these as `./tools/release.ps1` --
    # a lookbehind that also excluded `/` matched none of them, and the floor
    # below is what caught that rather than a green step proving nothing.
    $named = [regex]::Matches($wf, '(?<![\w.-])(?:\./)?(tools/[\w./-]+)') |
             ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
    foreach ($p in $named) {
        if (-not (Test-Path (Join-Path $repo $p))) { $problems += "the workflow names $p, which does not exist" }
    }
    if ($named.Count -lt 3) { $problems += "only $($named.Count) tools/ path(s) found in the workflow; this check parsed almost nothing" }

    if ($problems) { throw ($problems -join "`n        ") }
    Write-Host "        $($named.Count) repository paths, all present; no packaging facility in the workflow" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

# The Rust version floor, in the three places it is written down.
#
# Only one of them is enforced by anything: `rust-version` in the root
# Cargo.toml, which cargo refuses to build below, and which the `msrv` job in
# `.github/workflows/ci.yml` now reads and installs so that the declaration is
# compiled against rather than asserted. The other two are prose, and they are
# the copies a user acts on -- `tools/readme/README-LINUX.txt` names the
# toolchain to the reader it has just told to build from source because the
# binaries will not start on their glibc, and `tools/release-notes.md` says the
# same thing on the release page.
#
# All three read 1.82 until 2026-08-06, and 1.82 could not parse this
# workspace's own Cargo.lock. What let that survive is that the three agreed
# with each other and with nothing else. So this step compares the prose to the
# manifest, and the workflow compares the manifest to a compiler; neither alone
# is enough, and a floor raised in the manifest without touching the prose now
# fails here rather than on somebody's laptop.
Step 'the Rust version floor is one number, and CI installs it' {
    $manifest = [System.IO.File]::ReadAllText("$repo/Cargo.toml")
    if ($manifest -notmatch '(?m)^rust-version = "(\d+\.\d+(?:\.\d+)?)"') {
        throw 'the root Cargo.toml declares no rust-version, so there is no floor to hold anything to'
    }
    $floor = $Matches[1]
    $problems = @()

    # `Rust ` followed by a digit, so "pure Rust and no system libraries" two
    # lines below the number in README-LINUX.txt is prose and not a claim.
    foreach ($f in 'tools/readme/README-LINUX.txt', 'tools/release-notes.md') {
        $body = [System.IO.File]::ReadAllText((Join-Path $repo $f))
        $said = @([regex]::Matches($body, 'Rust (\d+\.\d+(?:\.\d+)?)') |
                  ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
        if ($said.Count -eq 0) {
            $problems += "$f names no Rust version at all; it is where a reader whose machine cannot run the binaries is sent to build from source"
        }
        foreach ($v in $said) {
            if ($v -ne $floor) { $problems += "$f tells the reader Rust $v; Cargo.toml declares $floor" }
        }
    }

    # And the workflow has to compile on that line rather than on a copy of it.
    $wf = [System.IO.File]::ReadAllText("$repo/.github/workflows/ci.yml")
    if ($wf -notmatch [regex]::Escape('rust-version = ')) {
        $problems += 'ci.yml never reads rust-version out of Cargo.toml; a version typed into the workflow is a second copy of the number and drifts the same way the first one did'
    }
    if ($wf -notmatch [regex]::Escape('cargo check --workspace --locked')) {
        $problems += 'ci.yml runs no `cargo check --workspace --locked`, so nothing compiles this tree on the floor it declares'
    }
    $pinned = @([regex]::Matches($wf, 'rust-toolchain@(\d[\w.]*)') | ForEach-Object { $_.Groups[1].Value })
    foreach ($p in $pinned) {
        $problems += "ci.yml pins dtolnay/rust-toolchain@$p; the floor comes from Cargo.toml or it is two numbers"
    }

    if ($problems) { throw ($problems -join "`n        ") }
    Write-Host "        Rust $floor in Cargo.toml, README-LINUX.txt and release-notes.md; ci.yml installs the manifest's line" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

# The three honest paragraphs, which are the ones that erode.
#
# Every artifact is unsigned and stays unsigned. Code signing came off the
# roadmap on 2026-08-06, so what this step guards is permanent text and not a
# description of an interval -- which makes it likelier to erode, not less,
# because nothing is coming along later to replace it.
#
# The release notes are the only place most users will read what
# that means, and the specific remedy for macOS -- clearing the quarantine
# attribute on named files -- is the difference between explaining a security
# decision and telling somebody to click through one. The Windows counterpart is
# the ABSENCE of the click-through: `docs/RELEASING.md` states that the words
# "More info" and "Run anyway" appear nowhere in anything shipped, and until now
# nothing checked that.
Step 'the release notes still say what an unsigned build costs' {
    $notes = Join-Path $repo 'tools/release-notes.md'
    if (-not (Test-Path $notes)) { throw 'there is no release-notes template' }
    $t = [System.IO.File]::ReadAllText($notes)
    $problems = @()
    foreach ($required in @(
        'xattr -d com.apple.quarantine',   # the honest macOS remedy, spelled out
        'SmartScreen',                      # named, so the user recognises the dialog
        'glibc',                            # the Linux artifact's real limit
        # The product promise, and it is worth saying what changed on
        # 2026-08-06 and what did not. `pl update` and an off-by-default switch
        # in the app now exist, so the old required phrase -- a bare "no
        # updater" -- became false and this gate would have kept it in the
        # notes. What survives is the promise that actually mattered: nothing
        # happens on a schedule, and a new installation asks for nothing. Both
        # halves are required here, because either one alone is the half a
        # rewrite would keep.
        'no auto-updater',
        'off by default',
        'licences/'                         # where the obligation travels
    )) {
        if ($t -notmatch [regex]::Escape($required)) { $problems += "the release notes no longer mention '$required'" }
    }
    # The click-through, in the shipped text and in the notes.
    foreach ($f in @($notes, (Join-Path $repo 'tools/installer/README-WINDOWS.txt'),
                     (Join-Path $repo 'tools/readme/README-MACOS.txt'),
                     (Join-Path $repo 'tools/readme/README-LINUX.txt'))) {
        $body = [System.IO.File]::ReadAllText($f)
        foreach ($phrase in 'Run anyway', 'More info →', 'More info ->') {
            # `More info` alone is not banned: README-WINDOWS.txt names the
            # dialog in order to explain it. The instruction is the arrow
            # followed by the button, which is what a reader copies.
            if ($body -match [regex]::Escape($phrase)) {
                $problems += "$(Split-Path -Leaf $f) tells the user to '$phrase'; docs/RELEASING.md says that habit is not taught here"
            }
        }
        if ($body -notmatch 'proves nothing about who') {
            $problems += "$(Split-Path -Leaf $f) no longer says what the checksum does NOT prove"
        }
    }
    if ($problems) { throw ($problems -join "`n        ") }
    Write-Host '        quarantine remedy, SmartScreen, glibc floor and the checksum caveat all present' -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

Step 'the cross-platform scripts touch no Windows-only environment variable unguarded' {
    # WHY THIS EXISTS. The first three-platform release failed on both non-Windows
    # legs at `Join-Path $env:USERPROFILE '.cargo\bin'`. Off Windows that variable
    # is not merely empty, it is absent; `Join-Path` refuses a null `-Path`; and
    # `$ErrorActionPreference = 'Stop'` promotes the refusal to a fatal error. The
    # Linux job died in 41 seconds having compiled nothing.
    #
    # Every gate in this file runs on Windows, where the faulty line works
    # perfectly. That is the actual defect: a Windows-only assumption in a script
    # whose entire purpose is to run on three platforms cannot be caught by
    # executing it here, so it has to be caught by reading it here. It was in fact
    # read, and called harmless -- prepending an empty string is a no-op, which is
    # true of the assignment and irrelevant to the call that produces it.
    #
    # A use is accepted only inside an `if` that tests the same variable, or
    # inside one that tests the platform. Guard scope is tracked by brace depth.
    $winOnly = 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'PROGRAMFILES', 'PROGRAMDATA',
               'SYSTEMROOT', 'WINDIR', 'COMSPEC', 'USERNAME', 'HOMEDRIVE', 'HOMEPATH'
    $problems = @()

    foreach ($rel in 'tools/release.ps1', 'tools/ci.ps1', 'tools/check-archive.ps1') {
        $p = "$repo/$rel"
        if (-not (Test-Path $p)) { throw "$rel is missing" }
        $lines = Get-Content -LiteralPath $p
        $depth = 0
        $guards = @{}   # depth -> list of variable names guarded at that depth
        $platformGuard = @{}

        for ($i = 0; $i -lt $lines.Count; $i++) {
            # Strip comments and single-quoted literals. A comment naming the
            # variable is documentation, and a quoted 'USERPROFILE' is a string,
            # not a dereference -- this very step would otherwise flag the prose
            # above, which is the mistake the release-workflow checker already
            # made once and had to be taught not to repeat.
            $code = $lines[$i] -replace '(^|\s)#.*$', ''

            $opensGuard = @()
            $guardsPlatform = $false
            if ($code -match '^\s*(\}\s*)?(else)?if\s*\(') {
                foreach ($v in $winOnly) {
                    if ($code -match ('\$env:' + $v + '\b')) { $opensGuard += $v }
                }
                if ($code -match '\$(IsWindows|onWindows|IsLinux|IsMacOS|onMac|onLinux)\b') { $guardsPlatform = $true }
            }

            # Active guards are those opened at a still-open depth.
            $active = @()
            $platformActive = $false
            foreach ($d in $guards.Keys) { if ($d -le $depth) { $active += $guards[$d] } }
            foreach ($d in $platformGuard.Keys) { if ($d -le $depth -and $platformGuard[$d]) { $platformActive = $true } }

            foreach ($v in $winOnly) {
                if ($code -notmatch ('\$env:' + $v + '\b')) { continue }
                if ($opensGuard -contains $v) { continue }        # this line IS the guard
                if ($active -contains $v) { continue }            # inside its own guard
                if ($platformActive -or $guardsPlatform) { continue }  # inside a platform branch
                # A bare assignment cannot fail; only passing it onward can.
                if ($code -match ('^\s*\$env:' + $v + '\s*=')) { continue }
                $problems += ("{0}:{1} uses `$env:{2} outside any guard -- that variable does not exist off Windows: {3}" -f
                              $rel, ($i + 1), $v, $code.Trim())
            }

            $opens = ([regex]::Matches($code, '\{')).Count
            $closes = ([regex]::Matches($code, '\}')).Count
            if ($opensGuard.Count -or $guardsPlatform) {
                $inner = $depth + 1
                if ($opensGuard.Count) { $guards[$inner] = $opensGuard }
                if ($guardsPlatform)   { $platformGuard[$inner] = $true }
            }
            $depth += $opens - $closes
            if ($depth -lt 0) { $depth = 0 }
            foreach ($d in @($guards.Keys))       { if ($d -gt $depth) { $guards.Remove($d) } }
            foreach ($d in @($platformGuard.Keys)) { if ($d -gt $depth) { $platformGuard.Remove($d) } }
        }
    }

    if ($problems) { throw ($problems -join "`n        ") }
    Write-Host '        release.ps1, ci.ps1 and check-archive.ps1 are clean' -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

Write-Host "`nbenchmark" -ForegroundColor Cyan
Step 'polylinker-bench' {
    # An absolute path, and the score is asserted rather than assumed.
    #
    # This step used to pass a relative forward-slash path, which Python's
    # subprocess cannot resolve on Windows -- so the adapter never launched,
    # every case scored "unsupported", the bench printed 0.0%, and run.py still
    # exited 0. The step reported ok for a score of zero. A gate that only
    # checks an exit code cannot notice a benchmark collapsing.
    $exe = (Resolve-Path 'target/release/pl.exe').Path
    $out = python bench/run.py bench/polylinker-bench.json -- $exe bench-adapter 2>&1
    $out
    if ($LASTEXITCODE -ne 0) { return }
    $line = $out | Select-String -Pattern '^all\s' | Select-Object -Last 1
    if (-not $line) {
        Write-Output 'no scorecard line in the bench output'
        $global:LASTEXITCODE = 1
        return
    }
    # all  <pass> <fail> <unsup> <total>  <rate>%
    $f = ($line.Line -split '\s+')
    [int]$passed = $f[1]; [int]$failed = $f[2]
    if ($failed -gt 0) {
        Write-Output "bench has $failed failing case(s)"
        $global:LASTEXITCODE = 1
    } elseif ($passed -lt $BenchFloor) {
        Write-Output "bench passed only $passed cases; the floor is $BenchFloor"
        $global:LASTEXITCODE = 1
    } else {
        $global:LASTEXITCODE = 0
    }
} { Have python }
Step 'bench regenerates identically' {
    python bench/generate.py > "$env:TEMP\regen.json" 2>$null
    $a = (Get-FileHash 'bench/polylinker-bench.json' -Algorithm SHA256).Hash
    $b = (Get-FileHash "$env:TEMP\regen.json" -Algorithm SHA256).Hash
    if ($a -ne $b) { Write-Output 'generate.py is no longer deterministic'; $global:LASTEXITCODE = 1 }
    else { $global:LASTEXITCODE = 0 }
} { (HavePy 'seguid') -and (HavePy 'pydna') }

$elapsed = (Get-Date) - $started
Write-Host ''
if ($script:skipped) {
    Write-Host ("skipped: {0}" -f ($script:skipped -join ', ')) -ForegroundColor DarkGray
}
if ($script:failed.Count -eq 0) {
    Write-Host ("GATE PASSED in {0:N0}s" -f $elapsed.TotalSeconds) -ForegroundColor Green
    exit 0
} else {
    Write-Host ("GATE FAILED: {0}" -f ($script:failed -join ', ')) -ForegroundColor Red
    exit 1
}
