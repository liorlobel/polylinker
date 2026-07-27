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
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:PATH = "$cargoBin;$env:PATH" }

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
    $out = & $Body 2>&1
    if ($LASTEXITCODE -eq 0) {
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
Step 'wasm module vs native binary' {
    node crates/pl-wasm/tests/drive_wasm.mjs `
        target/wasm32-unknown-unknown/wasm/pl_wasm.wasm target/release/pl.exe
} { $hasWasm -and (Have node) }

# `pl-index` must never touch the filesystem.
#
# The split between the pure search engine and `pl-scan`, which owns all the
# I/O, is the whole reason the engine can be reasoned about -- and a convention
# nobody can check is a convention that decays. wasm32 has no filesystem, so
# this step goes red the day a storage concern leaks in. It also means the
# browser tool can search an in-memory Vec<Row> through the identical code.
Step 'pl-index stays pure (wasm32)' {
    cargo build -p pl-index --target wasm32-unknown-unknown --profile wasm
} { $hasWasm }
# `unit tests` runs --lib --bins and would never reach an integration test.
Step 'pl-index and pl-scan tests' {
    cargo test -p pl-index -p pl-scan --tests
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
    if ($hits -notmatch 'records? matched' -or $hits -match '^0 records matched') {
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
