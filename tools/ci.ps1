<#
.SYNOPSIS
    Run the whole CI gate, in the same order CI runs it.

.DESCRIPTION
    This is THE gate. `.github/workflows/ci.yml` runs this file as its `gate`
    job, on every runner in that job's matrix; running it here runs the same
    list, so "CI is green" is something you can know before you push rather
    than after.

    THAT SENTENCE USED TO NAME THE RUNNERS -- "windows-latest, ubuntu-latest
    and macos-latest" -- and naming them here was a copy of a list that lives
    in ci.yml. A copy is a thing that drifts: the day a fourth runner is added
    there, this file would have gone on describing three, in the one document
    whose entire subject is prose asserting what the tree does not do. The
    matrix is one file away and it is named above.

    That was not true until 2026-08-09. This script's own header used to say
    "the repository has no remote yet, so GitHub Actions has never executed",
    and the sentence outlived the fact by six releases: no workflow invoked it,
    it failed on a clean tree from v0.1.2, and nothing anywhere reported that.
    It then ran on windows-latest ONLY, which made "the gate passed" mean
    "passed on Windows" -- and the sentence in this header did not say so.

    Steps that need something the machine may not have -- a corpus, Python
    oracles, a wasm target -- are SKIPPED rather than failing. A gate that
    fails for missing optional tooling teaches people to ignore it. That
    leniency is right for a workstation and dangerous on a runner, which is
    what -ExpectedSkips is for.

    SEVEN STEPS CANNOT RUN OFF WINDOWS AT ALL, because their subject is a Win32
    artefact: a PE resource directory, an 8.3 short name, the registry,
    msiexec. Each declares it in its own precondition, as `WindowsOnly { ... }`
    -- the one helper that owns the string 'not windows' -- and that string is
    checked against $IsWindows rather than believed. See 'THE SKIP DISCIPLINE'
    at the foot of this file. There is no second list of which steps are
    Windows-only: the preconditions are the list.

.PARAMETER Corpus
    Directory of real .dna / .gb files. Corpus tests skip without it, exactly
    as they do in CI, where the corpus cannot legally exist.

.PARAMETER ExpectedSkips
    Path to a file naming the steps that skip HERE FOR WANT OF A CORPUS, one
    per line, `#` to end of line being a comment. The run FAILS on any
    difference in either direction: a step that skipped for want of a corpus
    and is not named, and a step that is named and did not skip for that
    reason. A name matching no step in this gate also fails, so a renamed step
    cannot drift off the list unnoticed.

    Set equality, not a count: a count is satisfied by the wrong step skipping.

    Passing this switch also turns on the two rules that need no list at all:
    every skip must carry a declared reason, and a platform reason must agree
    with the platform. See the foot of this file.

.PARAMETER Ledger
    Where to write a tab-separated record of every step and what became of it:
    name, `ran` or `skipped`, and the reason. `.github/workflows/ci.yml`
    collects one of these from each runner in the `gate` matrix and
    `tools/reconcile-ledgers.ps1` compares them, which is the only place a step
    that skipped on EVERY platform can be seen. (That script takes a fixed
    number of ledgers, one per platform, and refuses a short set -- a leg that
    did not report at all is the case it is there to catch -- so adding a
    runner to the matrix is also an edit there.)

.EXAMPLE
    .\tools\ci.ps1
    .\tools\ci.ps1 -Corpus "C:\Users\me\OneDrive\plasmids"
    .\tools\ci.ps1 -ExpectedSkips .github/ci-expected-skips.txt
#>
[CmdletBinding()]
param([string]$Corpus, [string]$ExpectedSkips, [string]$Ledger)

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

# THE PLATFORM, ONCE, IN THE SAME THREE LINES `tools/release.ps1` USES.
#
# `$IsWindows`, `$IsLinux` and `$IsMacOS` are pwsh 7 automatic variables and do
# not exist in Windows PowerShell 5.1 at all -- where they read as $null. 5.1
# only runs on Windows, so $null means Windows.
#
# `$exe` and `$tmp` are the two spellings that made this file Windows-only.
# `target/release/pl.exe` appeared at twenty-five call sites and
# `[IO.Path]::GetTempPath()` -- which is $TMPDIR off Windows, %TEMP% on it, and
# never absent -- was already used at six while `$env:TEMP` was used at
# seventeen more. `$env:TEMP` off Windows is ABSENT, not empty, and
# `Join-Path $null x` is a terminating error: the exact failure that killed
# both non-Windows legs of the first release.
$onWindows = if ($null -eq $IsWindows) { $true } else { [bool]$IsWindows }
$onMac     = [bool]$IsMacOS
$onLinux   = [bool]$IsLinux
$exe       = if ($onWindows) { '.exe' } else { '' }
$tmp       = [System.IO.Path]::GetTempPath()
# The release binaries, by the name they carry on this platform. Named once so
# that a step reads `& $pl find ...` and cannot reintroduce the suffix.
$pl        = "target/release/pl$exe"

# Guarded for the reason given at length in the step named
# 'the cross-platform scripts touch no environment variable
# unguarded' near the end of this file: off Windows this variable is absent, not
# empty, and Join-Path treats a null -Path as a terminating error. The unguarded
# twin of these two lines killed both non-Windows legs of the first release.
if ($env:USERPROFILE) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo/bin'
    if (Test-Path $cargoBin) { $env:PATH = "$cargoBin$([IO.Path]::PathSeparator)$env:PATH" }
}
# And the Unix half of the same line, which had no twin at all: cargo's default
# home is ~/.cargo on Linux and macOS and $env:USERPROFILE does not exist there,
# so a runner that had not put cargo on PATH would have failed at `cargo` with
# no explanation. $HOME is absent on Windows, hence the guard on this one too.
if (-not $onWindows -and $env:HOME) {
    $cargoBin = Join-Path $env:HOME '.cargo/bin'
    if (Test-Path $cargoBin) { $env:PATH = "$cargoBin$([IO.Path]::PathSeparator)$env:PATH" }
}

# Cases polylinker-bench must still pass. Asserted, not merely printed: the
# step used to report ok while the benchmark scored zero. Raise this when the
# score rises; never lower it without saying why in the commit message.
$BenchFloor = 176

$script:failed = @()
$script:skipped = @()
# Every step's name, whether it ran or skipped. Only -ExpectedSkips reads it,
# and only so that a name on that list which matches nothing here is an error
# rather than silence: a step renamed in this file while the list kept the old
# spelling would otherwise look exactly like a step that stopped skipping.
$script:steps = @()
# One row per step: name, ran-or-skipped, and the reason a skip gave for
# itself. This is what -Ledger writes and what the cross-platform reconciler
# reads; it is also what the rules at the foot of this file are checked over.
#
# NOT `$script:ledger`. PowerShell variable names are case-insensitive, so that
# spelling IS the -Ledger parameter: the first version of this line overwrote
# the path it had been given with an array of step records, and the write at the
# foot of the file died in Split-Path on a six-kilobyte "directory name".
$script:stepLedger = @()
$started = Get-Date

# THE REASONS A STEP MAY GIVE FOR NOT RUNNING, and there are exactly two.
#
# A precondition returns $true to run, or a string to skip WITH A REASON, or
# $false to skip with none. The string is not a label somebody attached to a
# step: it is the return value of the test that failed, so 'not windows' cannot
# be produced by anything except `-not $onWindows`. To write it falsely you have
# to hand-type `WindowsOnly` into a scriptblock sitting under the step's own
# comment, in a reviewed diff.
#
# AND THAT REVIEWED DIFF IS THE WHOLE OF THE DEFENCE, which this comment used to
# deny. It said such an edit would additionally have "to survive X2 in
# tools/reconcile-ledgers.ps1, which requires every step to have RUN on at least
# one platform" -- true as a sentence and worthless as an obstacle, because a
# portable step relabelled Windows-only GOES ON RUNNING ON WINDOWS and X2 asks
# for nothing more than that. Measured, not reasoned: wrapping the precondition
# of 'gel calibration spline vs SciPy' in `WindowsOnly` and reconciling the three
# real ledgers of run 31359657821 with that one row changed on the Linux and
# macOS legs gives "reconciled 3 legs, 73 steps each; every step ran on at least
# one platform", exit 0. L1 does not fire (a reason was declared), L2 does not
# (the platform agrees), L3 does not (it is not a corpus skip), and X1, X3 and X4
# do not either. Two legs of coverage disappear and every check reports clean.
#
# So the honest statement of the boundary is this: the guard catches a step that
# stops running for a reason NOBODY DECLARED, on any platform, which is the
# failure that actually happens -- a wheel that stops building, a `pip install`
# line edited in a hurry. It does not catch a human deciding to call a portable
# step Windows-only. Nothing here can, without a second list of which steps ought
# to run where, and `.github/ci-expected-skips.txt` sets out at length why that
# list would be worse than the disease. What closes it is that `WindowsOnly` is
# one identifier, greppable in seven places, and this file is reviewed.
#
# $false is deliberately still allowed and deliberately still fatal under
# -ExpectedSkips: a missing SciPy, a missing node, a missing WiX has no entry
# here, so on a runner it is red. That is exactly what the committed skip list
# used to buy, and it now costs no list.
$script:ReasonVocabulary = @('not windows', 'corpus')

# THE ONLY PLACE THE STRING 'not windows' IS WRITTEN, and that is the point.
# Seven steps are Windows-only; if each spelled the literal itself there would
# be seven places to hand-write a lie, and the claim above -- that the reason is
# the return value of the test that failed -- would be seven times weaker.
# `$onWindows` is resolved at call time from script scope, so the string and the
# platform test are one expression in one place and cannot drift apart.
#
# `-AndAlso` is the rest of the precondition and is evaluated ONLY on Windows.
# That ordering is deliberate: off Windows the answer is already settled, and a
# `$script:release` that was never built, or a `wix` that is not installed, must
# not be able to turn a platform skip into an undeclared one.
function WindowsOnly {
    param([scriptblock]$AndAlso)
    if (-not $onWindows) { return 'not windows' }
    if (-not $AndAlso) { return $true }
    $v = & $AndAlso
    if ($v -is [System.Array]) { $v = if ($v.Count) { $v[-1] } else { $false } }
    return [bool]$v
}
$script:haveCorpus = (-not [string]::IsNullOrWhiteSpace($Corpus)) -and (Test-Path $Corpus)
function NeedsCorpus { if ($script:haveCorpus) { $true } else { 'corpus' } }

function Step {
    param([string]$Name, [scriptblock]$Body, [scriptblock]$Precondition = { $true })
    $script:steps += $Name

    # A scriptblock's value is a pipeline, so a precondition that emits more
    # than one object arrives here as an array. The DECISION is its last value,
    # which is what `-not (& $Precondition)` used to coerce to; taking [-1]
    # rather than the whole array keeps that behaviour and makes the string
    # case work.
    $verdict = & $Precondition
    if ($verdict -is [System.Array]) { $verdict = if ($verdict.Count) { $verdict[-1] } else { $false } }
    $reason = $null
    $run = $false
    if ($verdict -is [string]) {
        # Trimmed, and an ALL-WHITESPACE string counts as no reason at all
        # rather than as a reason that prints as nothing. A blank reason would
        # otherwise satisfy the "did it declare one" test at the foot of this
        # file while telling a reader nothing, which is the shape of every
        # defect this file records.
        if ($verdict.Trim()) { $reason = $verdict.Trim() }
    } elseif ($verdict) { $run = $true }

    if (-not $run) {
        $shown = if ($reason) { "  ({0})" -f $reason } else { '' }
        Write-Host ("  SKIP  {0}{1}" -f $Name, $shown) -ForegroundColor DarkGray
        $script:skipped += $Name
        $script:stepLedger += [pscustomobject]@{ Name = $Name; State = 'skipped'; Reason = $reason }
        return
    }
    $script:stepLedger += [pscustomobject]@{ Name = $Name; State = 'ran'; Reason = '' }
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

# A DIRECTORY PATH IN THE SAME SPELLING `FileInfo.FullName` USES, with exactly
# one trailing separator. The full reasoning is in `tools/release.ps1`, where a
# copy of this function is defined and where the defect first fired; the short
# version is that `Resolve-Path` and the FileSystemProvider disagree about 8.3
# aliases and about trailing separators, and every `Substring`/`StartsWith` on a
# path in this file was silently assuming they did not.
#
# It is duplicated rather than shared because the three files that need it --
# this one, `release.ps1` and `installer/Install-Polylinker.ps1` -- are each
# invoked directly and the installer additionally SHIPS ALONE, inside the release
# zip, with nothing to dot-source. A copy in each is the cost of that.
#
# THAT THE COPIES ARE STILL THE SAME FUNCTION IS CHECKED, not asserted here. This
# comment used to call `release.ps1`'s the "identical function" and nothing
# compared them -- prose standing in for a check, which is this project's own
# recurring defect sitting on top of the bug it describes. The step
# 'Get-DirectoryPrefix is one function copied, not three functions drifting'
# finds every definition under tools/ by parsing and compares them; the step
# after it drives each one over a real 8.3 alias. Change this function and both
# go red until the other copies follow.
function Get-DirectoryPrefix([string]$Path) {
    $abs = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
    $abs = [System.IO.Path]::GetFullPath($abs)
    $sep = [System.IO.Path]::DirectorySeparatorChar
    return $abs.TrimEnd($sep, [System.IO.Path]::AltDirectorySeparatorChar) + $sep
}
function HavePy($mod) {
    if (-not (Have python)) { return $false }
    python -c "import $mod" 2>$null | Out-Null
    return $LASTEXITCODE -eq 0
}

# `cargo test`, with the number of tests it ran PRINTED and asserted non-zero.
#
# The fourth way a step here reported ok while measuring nothing, and the one
# the sentinel in `Step` cannot see. `cargo test <filter>` exits 0 when the
# filter matches nothing -- "running 0 tests / test result: ok. 0 passed;
# 0 failed; N filtered out" -- so a genuine 0 comes back from a real command and
# every check above is satisfied. Two live instances on 2026-08-09:
# 'every coding record translates to its protein' filtered on a name that had
# been renamed away and ran nothing at all, and 'the same molecule twice, from
# two processes' named 1 of the 53 tests in `bins/pl/tests/cli.rs` while the
# other 52 were run by no gate on any platform. A step that runs 0 tests and a
# step that runs 53 are the same green line in this log; that is how both
# survived. Now they are not: the count is read out of libtest's own summary,
# one line per test binary, and zero is a failure.
#
# It counts `passed`, not `filtered out`: an all-ignored target would otherwise
# look like work. Ignored tests are not run, so they are not counted, and a
# target that is entirely #[ignore]d fails here -- which is the right answer for
# a gate step whose whole claim is that it executed something.
function CargoTest {
    $out = & cargo test @args 2>&1
    $code = $LASTEXITCODE
    if ($code -ne 0) {
        $out | ForEach-Object { Write-Output $_ }
        $global:LASTEXITCODE = $code
        return
    }
    $ran = 0
    foreach ($line in $out) {
        if ("$line" -match '^test result: ok\. (\d+) passed') { $ran += [int]$Matches[1] }
    }
    if ($ran -eq 0) {
        throw ("cargo test " + ($args -join ' ') + " ran 0 tests and still exited 0, so this step proved nothing. " +
               'A name filter that matches nothing is the usual cause -- check the test has not been renamed.')
    }
    Write-Host ("        {0} test(s) ran" -f $ran) -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

# THE SAME RULE FOR `node --test`, WHICH THIS FILE DOCUMENTED FOR CARGO AND THEN
# DID NOT GENERALISE.
#
# `CargoTest` above exists because `cargo test <filter>` exits 0 having run
# nothing. `node --test test/*.test.ts` does exactly the same thing for exactly
# the same reason, and the gate's node step was written without it. PowerShell
# does not expand a wildcard for a native command, so the pattern reaches node
# intact and NODE expands it -- and node's runner exits 0 when the pattern
# matches no file. Measured here on node v24.19.0 against
# `packages/circular-map`, which is the only caller:
#
#     test/*.test.ts    -> tests 46 / pass 46 / fail 0    EXIT=0
#     test/*.spec.ts    -> tests  0 / pass  0 / fail 0    EXIT=0
#
# A step asserting nothing but the exit code cannot tell those two apart, so one
# renamed file takes its tests dark behind an unchanged green line -- and nothing
# else in the tree would notice, because `packages/circular-map/tsconfig.json`
# includes only `src/**/*.ts` so the typecheck step never looks in `test/`, and
# `package.json`'s "test" script is byte-identical to the gate's command and so
# is vacuous in the same way.
#
# The count is read out of the runner's own summary rather than counted off the
# filesystem, for the reason `CargoTest` reads libtest's: a file that exists and
# is not collected is precisely the case a directory listing calls healthy.
#
# NO SUMMARY IS ALSO A FAILURE. If a future node stops printing `pass N` this
# throws rather than falling back on "well, it exited 0" -- the fail-closed
# direction, and the one that keeps this from becoming another check that cannot
# fail. Both of node's reporters are read: the TAP one it uses when stdout is not
# a terminal prints `# pass 46`, the spec one it uses when it is prints an info
# glyph and then `pass 46`, and CI has been seen to give either.
function Assert-NodeTestSummary {
    param([string[]]$Lines, [string]$What)
    $pass = -1
    $fail = -1
    foreach ($line in $Lines) {
        # `[^0-9]*` before the word is what keeps a TEST NAME containing "pass"
        # from being read as the summary: under TAP such a line reads
        # `ok 12 - ...`, and the digits of the ordinal cannot be crossed; under
        # the spec reporter it carries a `(0.41ms)` suffix, and the anchor
        # forbids anything after the number.
        if ($line -match '^[^0-9]*\bpass\s+(\d+)\s*$') { $pass = [int]$Matches[1] }
        elseif ($line -match '^[^0-9]*\bfail\s+(\d+)\s*$') { $fail = [int]$Matches[1] }
    }
    if ($pass -lt 0 -or $fail -lt 0) { throw ($What + ' printed no test-runner summary at all, so nothing here read a count out of it and this step proved nothing.') }
    if ($pass -eq 0) { throw ($What + ' ran 0 tests and still exited 0, so this step proved nothing. A glob that matches no file is the usual cause -- node --test exits 0 for an empty match, exactly as cargo test does for a name filter that matches nothing. Check that no test file has been renamed.') }
    if ($fail -ne 0) { throw ($What + " reported $fail failing test(s) and still exited 0.") }
    return $pass
}

Write-Host "`nPolylinker CI gate" -ForegroundColor Cyan
Write-Host ("rustc {0}" -f (rustc --version))

Write-Host "`nlint" -ForegroundColor Cyan
Step 'rustfmt'  { cargo fmt --all --check }
Step 'clippy'   { cargo clippy --workspace --all-targets -- -D warnings }

Write-Host "`ntests" -ForegroundColor Cyan
Step 'unit tests'    { cargo test --workspace --lib --bins }
Step 'release build' { cargo build --workspace --release }
# pl-fileio's integration targets, ALL of them, corpus or no corpus.
#
# This step named `--test corpus` and was preconditioned on -Corpus, so on a
# machine without the lab drive it SKIPPED -- and `crates/pl-fileio/tests/
# notes_report.rs`, the only cover for the line that carries the unrepresentable-
# notes report from `Document` to `LoadReport` (lib.rs:221), was run by nothing,
# here or in `.github/workflows/ci.yml`. Its own header records the injection:
# replacing that assignment with `Vec::new()` left `cargo test --workspace`
# green, because the unit tests call `parse_notes` directly and the corpus test
# reads `Document`. `pl info` and `pl convert` would then stop telling the user
# which part of a .dna's Notes block was dropped, silently.
#
# `--tests`, not two named targets: a new file under tests/ is picked up the day
# it is written rather than the day somebody remembers this line. The corpus
# target skips itself when PL_CORPUS is unset (it prints "skipping: set
# PL_CORPUS" under --nocapture), which is why the precondition could go: running
# it unconditionally also proves it skips cleanly, the same argument
# .github/workflows/ci.yml makes for keeping it in a corpus-less CI.
#
# `--tests` also re-runs pl-fileio's lib tests, which `unit tests` above already
# ran. That is 0.04 seconds and one fewer list to keep correct.
Step 'pl-fileio tests (corpus cases skip without -Corpus)' {
    if (-not [string]::IsNullOrWhiteSpace($Corpus) -and (Test-Path $Corpus)) {
        $env:PL_CORPUS = (Resolve-Path $Corpus).Path
    }
    # `'--'` IS QUOTED ON PURPOSE. PowerShell's parameter binder treats a bare
    # `--` as its own end-of-parameters marker and removes it from $args, so
    # `CargoTest ... -- --nocapture` reaches cargo as `--nocapture` and cargo
    # refuses it ("unexpected argument '--nocapture' found"). Measured, not
    # guessed: a function printing $args gets 4 arguments for the bare form and
    # 5 for this one. The plain `cargo test` lines elsewhere in this file are a
    # native command invocation, where the rule does not apply.
    CargoTest -p pl-fileio --tests '--' --nocapture
}

Write-Host "`nwasm" -ForegroundColor Cyan
$hasWasm = (rustup target list --installed) -contains 'wasm32-unknown-unknown'
Step 'wasm32 build' {
    cargo build -p pl-wasm --target wasm32-unknown-unknown --profile wasm
} { $hasWasm }
# The wasm module's own checks: the ABI, the allocator, the string boundary.
Step 'wasm module self-checks' {
    node crates/pl-wasm/tests/drive_wasm.mjs `
        target/wasm32-unknown-unknown/wasm/pl_wasm.wasm $pl
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
        target/wasm32-unknown-unknown/wasm/pl_wasm.wasm $pl `
        (Resolve-Path $Corpus).Path
} { if (-not ($hasWasm -and (Have node))) { $false } else { NeedsCorpus } }

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

# The test names in one integration target, in declaration order.
#
# A line scan rather than one regex over the file: `#[test]` is followed by
# `#[ignore]`, `#[should_panic]` and doc comments often enough that a
# fixed-width lookahead would miss those functions and a greedy one would run
# past into the next.
function Get-TestNames($Path) {
    $names = @()
    $armed = $false
    foreach ($line in Get-Content -LiteralPath $Path) {
        if ($line -match '^\s*#\[test\]') { $armed = $true; continue }
        if ($armed -and $line -match '^\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)') {
            $names += $Matches[1]
            $armed = $false
        }
    }
    # Returned plainly and wrapped in @() by every caller, NOT with the unary
    # comma used elsewhere in this file. `, $names` survives the pipeline
    # unflattened, and `@(, $names)` is then an array holding one array -- which
    # cost an hour here: the one-test target (pl-draw's memory.rs) came back as
    # a nested array whose .Contains() is IList's exact match, silently turning
    # the substring test below into an equality test.
    $names
}

# Every `cargo test` a gate file runs, and what each one actually selects.
#
# COMMENTS AND SINGLE-QUOTED LITERALS ARE STRIPPED FIRST, because both carry the
# words this looks for: the note above `pl-index and pl-scan tests` explains the
# `--lib --bins` rule in prose, and each of the four pl-draw oracle steps below
# prints its own 'cargo test --test X failed; re-running it visibly'. A
# checker that cannot tell code from prose gets silenced rather than satisfied --
# 'the release workflow assembles nothing itself' below made exactly that
# mistake on its first run.
#
# `CargoTest` counts as `cargo test`, because it is: it is the wrapper defined at
# the top of this file that runs the same command and asserts the number of
# tests that came back.
#
# A token beginning with `$` or `@` makes the invocation Dynamic -- a splatted
# argument list cannot be read statically, so nothing is claimed about it and it
# covers nothing. `CargoTest`'s own body is the one such line here.
function Get-CargoTestInvocations($Path) {
    $found = [System.Collections.Generic.List[object]]::new()
    $lines = Get-Content -LiteralPath $Path
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $code = $lines[$i] -replace '(^|\s)#.*$', '' -replace "'[^']*'", ''
        if ($code -notmatch '(?:^|\s)(?:cargo\s+test|CargoTest)\b(.*)$') { continue }
        $tail = $Matches[1]
        # Where the command ends and the shell resumes.
        foreach ($stop in '|', '2>&1', ';', '}') {
            $k = $tail.IndexOf($stop)
            if ($k -ge 0) { $tail = $tail.Substring(0, $k) }
        }
        $inv = [pscustomobject]@{
            File = $Path; Line = $i + 1
            Packages = @(); Targets = @(); Filters = @()
            Dynamic = $false; LibBins = $false
        }
        $tok = @($tail -split '\s+' | Where-Object { $_ })
        for ($j = 0; $j -lt $tok.Count; $j++) {
            $t = $tok[$j]
            if ($t -match '^[@$]') { $inv.Dynamic = $true }
            elseif (($t -eq '-p' -or $t -eq '--package') -and $j + 1 -lt $tok.Count) { $inv.Packages += $tok[++$j] }
            elseif ($t -eq '--test' -and $j + 1 -lt $tok.Count) { $inv.Targets += $tok[++$j] }
            elseif ($t -eq '--lib' -or $t -eq '--bins' -or $t -eq '--doc') { $inv.LibBins = $true }
            elseif ($t.StartsWith('-')) { }      # any other flag, and `--` itself
            else { $inv.Filters += $t }          # a positional is a name filter
        }
        $found.Add($inv)
    }
    $found.ToArray()   # plainly, for the reason Get-TestNames gives
}

# Which integration targets a set of invocations RUNS, whole.
function Get-SuitesRun($Invocations, $Targets) {
    $runs = @{}
    foreach ($inv in $Invocations) {
        # A FILTERED invocation runs part of a target, so it does not count as
        # running the target. That is the finding this whole step exists for.
        if ($inv.Dynamic -or $inv.Filters.Count -gt 0) { continue }
        foreach ($t in $Targets) {
            if ($inv.Packages.Count -gt 0 -and $inv.Packages -notcontains $t.Package) { continue }
            if ($inv.Targets.Count -gt 0) {
                if ($inv.Targets -notcontains $t.Target) { continue }
            } elseif ($inv.LibBins) {
                continue   # --lib/--bins builds no tests/ target
            }
            $runs["$($t.Package)/$($t.Target)"] = "$(Split-Path -Leaf $inv.File):$($inv.Line)"
        }
    }
    $runs
}

# IS EVERY INTEGRATION SUITE IN THIS TREE ACTUALLY RUN BY A GATE?
#
# Four times now the answer has been no, and every time it was found by an audit
# rather than by a red gate, because there is nothing to see: `unit tests` is
# `--workspace --lib --bins`, which builds no `tests/` target at all, so a suite
# nobody names simply never runs and no line in the log says so.
#
#   * pl-index / pl-scan  -- fixed by naming them, above.
#   * pl-design           -- six suites, 45 tests, run by no gate on any
#                            platform from the day they were written.
#   * bins/pl/tests/cli.rs -- 52 of 53, including the only enforcement of
#                            "exactly one verb reaches the network".
#   * pl-fileio/tests/notes_report.rs and pl-draw/tests/memory.rs -- never named
#                            anywhere in tools/ or .github/.
#
# Each fix was a line naming one more suite, which is the same list to forget
# the next one from. So this reads the tree instead: one target per `.rs`
# directly under a crate's `tests/` (no manifest here sets `autotests = false`
# or declares a `[[test]]` section, so the files ARE the targets), and every one
# of them must be run, whole, by tools/ci.ps1 or by .github/workflows/ci.yml.
#
# EITHER GATE, NOT BOTH -- and note that "either" is no longer "one of two
# unrelated lists". Since 2026-08-09 ci.yml RUNS this script, as the `gate` job,
# so what is left in ci.yml alongside it is the part that is deliberately not
# here: `pl-features/tests/schema_pin.rs` is in ci.yml by design and in no step
# here, and `pl-features/tests/budget.rs` is here (it must run --release, and
# asserts nothing under debug_assertions) and deliberately not there. Requiring
# both would be requiring a duplication the project has reasoned its way out of.
#
# The sentence this comment used to quote from CONTRIBUTING.md -- that ci.yml
# "does not invoke this script, so it is a second list of steps rather than the
# same one" -- was doubly wrong by the time it was read: ci.yml does invoke it,
# and `git log -S` over the whole history finds that string never having been in
# CONTRIBUTING.md, or anywhere else in this repository, at all. CONTRIBUTING.md
# says the opposite today, under 'Run the gate before you submit': "CI runs this
# same file".
#
# WHAT IT DOES NOT PROVE: that a step's preconditions hold on the machine you
# are reading this on. Four of pl-draw's suites are named inside Python-gated
# steps here, and are unconditional in ci.yml. This says a suite is RUN BY A
# GATE, not that it ran today -- for which the count `CargoTest` prints is the
# evidence.
#
# It also refuses a name filter that matches no test, which is how
# 'every coding record translates to its protein' spent weeks running zero tests
# and printing ok after the test it named was renamed.
Step 'every integration suite is run by a gate' {
    $targets = @()
    foreach ($dir in 'crates', 'bins') {
        foreach ($crate in Get-ChildItem -LiteralPath (Join-Path $repo $dir) -Directory) {
            $manifest = Join-Path $crate.FullName 'Cargo.toml'
            $tdir = Join-Path $crate.FullName 'tests'
            if (-not (Test-Path $manifest) -or -not (Test-Path $tdir)) { continue }
            # The package name out of [package], not the directory name: cargo
            # is given `-p <name>`, and the two need not agree.
            $pkg = $null
            $section = ''
            foreach ($line in Get-Content -LiteralPath $manifest) {
                if ($line -match '^\s*\[([^\]]+)\]') { $section = $Matches[1]; continue }
                if ($section -eq 'package' -and $line -match '^\s*name\s*=\s*"([^"]+)"') { $pkg = $Matches[1]; break }
            }
            if (-not $pkg) { throw "$dir/$($crate.Name)/Cargo.toml declares no [package] name" }
            foreach ($f in Get-ChildItem -LiteralPath $tdir -Filter '*.rs' -File) {
                $targets += [pscustomobject]@{ Package = $pkg; Target = $f.BaseName; Path = $f.FullName }
            }
        }
    }
    # A floor, because a glob that stopped matching would enumerate nothing and
    # report success -- the failure mode three other steps in this file record.
    if ($targets.Count -lt 20) {
        throw "only $($targets.Count) integration target(s) found under crates/*/tests and bins/*/tests; this step enumerated almost nothing and proved nothing"
    }

    $gates = 'tools/ci.ps1', '.github/workflows/ci.yml'
    $invocations = @()
    foreach ($g in $gates) {
        $p = Join-Path $repo $g
        if (-not (Test-Path $p)) { throw "$g is missing" }
        $invocations += @(Get-CargoTestInvocations $p)
    }
    if ($invocations.Count -lt 12) {
        throw "only $($invocations.Count) cargo test invocation(s) parsed out of the two gate files; the parser is broken and this step proved nothing"
    }

    $problems = @()
    $runs = Get-SuitesRun $invocations $targets
    foreach ($t in $targets) {
        $key = "$($t.Package)/$($t.Target)"
        if (-not $runs.ContainsKey($key)) {
            $problems += ("crates or bins: $key is run by no gate. Add it to tools/ci.ps1 or " +
                          ".github/workflows/ci.yml -- --lib --bins does not build a tests/ target, so nothing else will reach it.")
        }
    }

    # And no step may filter on a name no test carries.
    foreach ($inv in $invocations) {
        if ($inv.Dynamic -or $inv.Filters.Count -eq 0 -or $inv.Targets.Count -eq 0) { continue }
        foreach ($f in $inv.Filters) {
            $hit = $false
            foreach ($t in $targets) {
                if ($inv.Targets -notcontains $t.Target) { continue }
                if ($inv.Packages.Count -gt 0 -and $inv.Packages -notcontains $t.Package) { continue }
                foreach ($n in @(Get-TestNames $t.Path)) {
                    if ($n.Contains($f)) { $hit = $true }
                }
            }
            if (-not $hit) {
                $problems += "$(Split-Path -Leaf $inv.File):$($inv.Line) filters on '$f', which is a substring of no test name in $($inv.Targets -join ', '); cargo exits 0 having run nothing"
            }
        }
    }

    # THIS CHECKER CAN FAIL, shown on planted input rather than asserted -- the
    # rule the oracles in this file are held to. Both halves are green above
    # only because both were fixed; without a probe there would be nothing left
    # to distinguish a working checker from one whose regex stopped matching.
    #
    # `cargo test` is not written as one literal here, because tools/ci.ps1 is
    # one of the two files the scanner reads and a complete invocation in this
    # string would be collected as a real one -- carrying, by design, a filter
    # no test has.
    $verb = 'cargo ' + 'test'
    $probe = Join-Path ([System.IO.Path]::GetTempPath()) "pl-gate-probe-$PID.txt"
    Set-Content -LiteralPath $probe -Value @(
        "$verb -p pl --test cli a_name_no_test_carries",
        "$verb -p pl-fileio --tests"
    )
    $planted = @(Get-CargoTestInvocations $probe)
    Remove-Item -LiteralPath $probe -Force
    if ($planted.Count -ne 2) { throw "the invocation parser found $($planted.Count) of 2 planted commands" }
    if ($planted[0].Filters -notcontains 'a_name_no_test_carries') { throw 'the parser did not see a planted name filter' }
    foreach ($n in @(Get-TestNames ($targets | Where-Object { $_.Package -eq 'pl' -and $_.Target -eq 'cli' }).Path)) {
        if ($n.Contains('a_name_no_test_carries')) { throw 'the planted filter matched a real test; pick another' }
    }
    $plantedRuns = Get-SuitesRun $planted $targets
    if ($plantedRuns.ContainsKey('pl/cli')) { throw 'a filtered invocation was counted as running its whole target' }
    if (-not $plantedRuns.ContainsKey('pl-fileio/notes_report')) { throw 'an unfiltered --tests invocation was not counted as running the suite' }

    if ($problems) { throw ($problems -join "`n        ") }
    Write-Host ("        $($targets.Count) integration suite(s), all run; $($invocations.Count) cargo test invocation(s) in $($gates.Count) gate files") -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
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

    & $pl index $lab --index-at $idx 2>$null | Out-Null
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
        $indexed = & $pl find $lab @q --index-at $idx 2>$null | Out-String
        $direct  = & $pl find $lab @q --no-index 2>$null | Out-String
        if ($indexed -cne $direct) {
            $bad++
            Write-Host "        DIFFER: $($q -join ' ')"
            Write-Host "        indexed: $($indexed -replace "`r?`n",' | ')"
            Write-Host "        direct : $($direct  -replace "`r?`n",' | ')"
        }
    }
    # A query set that matches nothing would agree trivially, so assert that
    # the fixture actually produces hits.
    $hits = & $pl find $lab --motif GAATTC --index-at $idx 2>$null | Out-String
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
        python reference/python/tests/xcheck_seguid.py $pl
    } else {
        python reference/python/tests/xcheck_seguid.py $pl "$Corpus/**/*.dna"
    }
} { HavePy 'seguid' }
Step 'digest + PCR vs pydna' {
    python reference/python/tests/xcheck_clone.py $pl
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
    python reference/python/tests/xcheck_motif.py $pl
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
# antialiasing implementations disagree slightly along edges and nowhere else.
# Away from every edge -- no edge in the eight neighbours -- they must agree
# EXACTLY, so the bar there is zero differing pixels rather than a percentage,
# and a wrong winding, a misplaced glyph or a hole in a stroke lands squarely on
# it. Currently 0 of 71,009 to 8,151,997 flat pixels differ on each figure at
# each scale, with 100% of the residue on an edge.
#
# A GLOBAL "95% OF ALL PIXELS IDENTICAL" WAS THE FIRST BAR AND MEASURED THE
# WRONG THING -- how much of the canvas is blank. The ring is 720x720, mostly
# white, and passed at 98.3%; the linear figure is 720x123 and almost entirely
# ink, and failed at 92.2% with ZERO gross differences and 100% of its residue
# on an edge. A threshold a correct renderer fails for being densely drawn is a
# threshold that gets widened until it means nothing.
#
# BOTH FIGURES, named by `FIGURES` in the test's output directory. The ring and
# the track are different rasterizer workloads -- arcs, thick strokes and sparse
# text against long thin boxes, concave pentagons, hairlines and dense small
# text -- and the ring exercises almost none of the second set. It is an oracle
# for the RASTERIZER and not for the geometry: both images come from the same
# SVG, so a scene that is wrong is wrong in both. Moving an arrowhead's tip by
# 2 units leaves it clean; half a unit on the baseline constant fails 8 checks.
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
# What the BINARY does when you run it: all 53 tests in bins/pl/tests/cli.rs.
#
# THIS STEP RAN ONE OF THEM until 2026-08-09. It was called 'the same molecule
# twice, from two processes' and passed
# `two_processes_render_the_same_molecule_to_the_same_bytes` as a name filter,
# and it was the only line in tools/ or .github/ that reached the target at all:
# `unit tests` above is `--workspace --lib --bins`, which builds no tests/
# target, and .github/workflows/ci.yml never tested package `pl` at all. So 52
# tests were run by no gate, on any platform.
#
# WHAT WAS UNENFORCED, and it is the reason this is not a tidying commit.
# `bins/pl/Cargo.toml` names `only_the_update_verb_can_reach_the_network`
# (cli.rs:2383) as the mechanism holding the promise that exactly one verb
# touches the network: it reads this binary's own source and fails if a call
# into `pl_update` appears outside `cmd_update`. It is the only thing anywhere
# that holds it -- `crates/pl-update/tests/handoff.rs` reads only its own
# crate's sources, and Step 'the installer contacts nothing' below scopes to
# tools/installer/. Adding `let _ = pl_update::curl_available();` to `cmd_info`
# compiles, clippy is silent, the release build is silent, and both gates stayed
# green. cli.rs is also the regression suite for the previous audit's data-loss
# findings -- `converting_never_writes_over_a_file_that_is_still_an_input`
# (cli.rs:70) and `a_raster_too_big_to_hold_is_refused_before_it_is_allocated`
# (cli.rs:2229) among them -- every one of which was reproduced against the
# release binary before it was fixed.
#
# THE TWO-PROCESS TEST is still the reason this runs on every machine rather
# than behind a Python precondition, so its argument is kept here.
# "Byte-identical on every platform" is the sentence on the front of this
# project, and nothing else in the gate compares two separate runs of the
# renderer: `pl-draw`'s own determinism tests render eight times inside ONE
# process, which holds constant every single thing that varies between
# processes -- the allocator's state, `RandomState`'s per-process seed, the
# environment, the locale. A `HashSet` in the label path is invisible to a loop
# and shows up here immediately. Demonstrated, not assumed: adding
# `(std::process::id() % 3) as f64` to the linear figure's height leaves
# `the_linear_figure_is_byte_identical_for_identical_input` GREEN -- the
# perturbation is constant inside a run -- and turns this red on the first
# comparison.
#
# NO PYTHON, no resvg, no corpus, 13 seconds. It runs everywhere the gate runs
# -- which since 2026-08-09 includes windows-latest, where the `gate` job runs
# this file -- and .github/workflows/ci.yml runs it on ubuntu-latest and
# macos-latest. Between the two that is all three platforms.
Step 'the pl binary, from the outside (bins/pl/tests/cli.rs)' {
    CargoTest -p pl --test cli
}
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
    python reference/python/tests/xcheck_pdf.py $pl
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
    python reference/python/tests/xcheck_tm.py $pl
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
    python reference/python/tests/xcheck_primers.py $pl
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
    python reference/python/tests/xcheck_translate.py $pl
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
    python reference/python/tests/xcheck_sanger.py $pl
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
    python reference/python/tests/xcheck_eps.py $pl (Get-ChildItem tests/library-fixture/*.gb, tests/export-fixture/*.gb | ForEach-Object { $_.FullName })
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
#
# THE NAME CPYTHON WILL LOAD IT UNDER IS NOT THE NAME CARGO WROTE, and the two
# differ differently on each platform. This step half-knew that until
# 2026-08-10: it branched on Windows for the BUILT name and then hardcoded
# `polylinker.pyd` for the SHIPPED one, which is wrong everywhere except
# Windows -- CPython off Windows loads `.so` -- and it chose `libpolylinker.so`
# for everything non-Windows, which is wrong on macOS, where cargo emits
# `libpolylinker.dylib`. So the macOS leg's first red here would have been a
# real defect rather than a portability nuisance, and the tempting fix -- make
# the step Windows-only -- would have deleted the check on two platforms
# instead of fixing it.
#
# The table is `tools/release.ps1`'s, verbatim and for the same reason it gives
# there: `.dylib` is "the one intuition to distrust here". Two copies rather
# than a shared module, because release.ps1 is invoked directly and this file
# runs before it; they are three lines and the step 'shipped Python extension
# imports' below drives release.ps1's copy over a real release, so a divergence
# between them shows up as a failure and not as silence.
Step 'Python bindings vs Biopython' {
    cargo build --release -p pl-py 2>&1 | Out-Null
    $pyMap = if ($onWindows) { @{ Built = 'polylinker.dll';       Shipped = 'polylinker.pyd' } }
             elseif ($onMac)  { @{ Built = 'libpolylinker.dylib'; Shipped = 'polylinker.so' } }
             else             { @{ Built = 'libpolylinker.so';    Shipped = 'polylinker.so' } }
    $src = Join-Path 'target/release' $pyMap.Built
    if (-not (Test-Path $src)) { throw "cargo built no $($pyMap.Built) in target/release, so there is nothing to import" }
    $dst = Join-Path $tmp $pyMap.Shipped
    Copy-Item $src $dst -Force
    python reference/python/tests/xcheck_pybindings.py $dst
} { HavePy 'Bio' }
# The MCP server answers JSON-RPC, and keeps its caveats across the boundary.
Step 'MCP server' {
    cargo test -p pl-mcp --quiet
}

Step 'gel calibration spline vs SciPy' {
    cargo build --release -p pl-gel --example dump_spline 2>&1 | Out-Null
    python reference/python/tests/xcheck_spline.py "target/release/examples/dump_spline$exe"
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
    python reference/python/tests/xcheck_trace_render.py $pl "$Corpus/**/*.ab1" $tmp
} { if (-not (HavePy 'Bio')) { $false } else { NeedsCorpus } }
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
    python reference/python/tests/xcheck_abif.py $pl "$Corpus/**/*.ab1"
} { if (-not (HavePy 'Bio')) { $false } else { NeedsCorpus } }
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
    python reference/python/tests/validate_digest.py $pl "$Corpus/**/*.dna"
} { if (-not (HavePy 'Bio')) { $false } else { NeedsCorpus } }
Step 'rust reader vs python reader' {
    python reference/python/tests/xcheck_rust.py $pl "$Corpus/**/*.dna"
} { if (-not (HavePy 'Bio')) { $false } else { NeedsCorpus } }

Write-Host "`nannotation database" -ForegroundColor Cyan
# The shipped table, against its own schema and its own translations.
#
# ONE STEP, NO NAME FILTER, where there were two filtered ones. The second was
# called 'every coding record translates to its protein' and filtered on
# `every_coding_record`, which is the name the test had before
# crates/pl-features/tests/corpus.rs:125 records renaming it to
# `every_record_carrying_both_sequences_translates_from_one_to_the_other`. A
# filter that matches nothing is not an error to cargo -- "0 passed; 0 failed;
# 5 filtered out", exit 0 -- so the step printed ok having executed no
# assertion, and the retry on the next line could never fire. It had been dead
# since the rename.
#
# Two filters are also two runs of one test binary for less coverage than one
# unfiltered run: the target holds 5 tests, the pair selected 1. This now runs
# all of them, which is what .github/workflows/ci.yml's step 'Feature database,
# against the shipped table' does, and `CargoTest` prints the number so a
# future rename cannot quietly shrink it again.
# Two of the five need a corpus and skip cleanly without one; PL_CORPUS is
# already set for this process if -Corpus was given.
Step 'the shipped feature database (schema, provenance, nt->aa)' {
    CargoTest -p pl-features --test corpus
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
# THE 46 PROPERTY TESTS, WITH A FLOOR UNDER THEM.
#
# This step used to be the three lines below with nothing but the exit code
# asserted, which made it the node-shaped twin of the defect `CargoTest` was
# written for: rename `render.test.ts` to `render.tests.ts` and 23 of the 46 go
# dark; rename all three and the runner reports `tests 0 / pass 0 / fail 0` and
# exits 0, and this log prints the same word it printed yesterday. What a dark
# `test/` would cost is the hostile-input and label-geometry property tests --
# not the cross-implementation diff, which 'renderers agree (fixture is
# current)' below covers byte for byte and separately.
#
# The pattern is QUOTED now. It made no difference to what node received --
# PowerShell passes a wildcard through to a native command untouched either way
# -- and it says out loud that the glob is node's to expand, which is the whole
# reason a zero match is possible here at all.
Step 'circular-map tests' {
    Push-Location packages/circular-map
    try {
        $out = & node --test --experimental-strip-types 'test/*.test.ts' 2>&1
        $code = $LASTEXITCODE
    } finally { Pop-Location }
    if ($code -ne 0) { $out | ForEach-Object { Write-Output $_ }; $global:LASTEXITCODE = $code; return }
    $ran = Assert-NodeTestSummary $out 'node --test packages/circular-map/test/*.test.ts'

    # THE FLOOR IS SHOWN TO FIRE, on planted input rather than asserted -- the
    # rule every oracle in this file is held to, because a floor over a healthy
    # corpus is indistinguishable from no floor at all. Delete the zero-test
    # throw in `Assert-NodeTestSummary` and this probe catches nothing, and this
    # step goes red rather than quietly returning to reporting ok over 0 tests.
    $fired = $false
    try { Assert-NodeTestSummary @('# tests 0', '# suites 0', '# pass 0', '# fail 0') 'a planted zero-test transcript' | Out-Null }
    catch { $fired = $true }
    if (-not $fired) { throw 'the zero-test floor did not fire on a planted "pass 0" summary, so this step could still report ok having run nothing -- which is the defect the floor was added to remove' }

    Write-Host ("        {0} test(s) ran" -f $ran) -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
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

Write-Host "`nbrowser prototype (the single-file page)" -ForegroundColor Cyan

# WHY THIS SECTION EXISTS AT ALL, WHICH IS THE POINT OF IT.
#
# Until 2026-08-14 the string `prototype` did not appear ONCE in this file's
# 4,019 lines, and neither did `dna-reader`, `build-web` or `check_page`.
# `prototype/dna-reader.template.html` is 1,257 lines, `README.md` calls the page
# it builds "Usable today", `tools/build-web.ps1` builds it and
# `prototype/check_page.js` drives it -- and no gate on any platform ran any of
# the three. The first audit that ever opened that file raised five findings and
# all five survived refutation, one of them a page telling a user that 58 of the
# 58 enzymes in its set do not cut a molecule that all 58 cut. Yield tracks first
# contact with a surface; this section is the first contact.
#
# THE PLACEHOLDER CONTRACT BETWEEN THE TEMPLATE AND ITS BUILDER, as a function
# so it can be run over planted input as well as over the real pair.
#
# The template is not the artefact. It carries `{{...}}` placeholders that
# `tools/build-web.ps1` substitutes -- the base64 wasm module, the demo GenBank
# record, a build stamp -- and the page a reader opens is the result. Rename one
# on either side and NOTHING TODAY SAYS SO: build-web.ps1 throws for
# `{{WASM_BASE64}}` alone (its line 58) and its other two substitutions are
# `String.Replace` calls, which are silent no-ops when the needle is absent. A
# template that spelled the stamp differently would ship the literal text
# `{{BUILD_STAMP}}` to a reader inside a comment about provenance; a builder
# substituting one the template no longer has would ship a page carrying no
# provenance at all -- which is how a four-commit-stale build sat on the author's
# disk, missing a fifteen-line null check, while `check_page.js` printed ALL
# CHECKS PASSED against it.
function Compare-WebPlaceholders {
    param([string[]]$InTemplate, [string[]]$InBuilder)
    $found = @()
    foreach ($p in $InTemplate) { if ($InBuilder -notcontains $p) { $found += "the template uses $p and tools/build-web.ps1 substitutes nothing for it, so the built page would hand that placeholder to a reader as literal text" } }
    foreach ($p in $InBuilder) { if ($InTemplate -notcontains $p) { $found += "tools/build-web.ps1 substitutes $p and the template no longer contains it, so that substitution is a silent no-op and whatever it carried is now missing from the page" } }
    return $found
}

# NO PRECONDITION, DELIBERATELY, and the step is written so that it is honest
# about which of its three halves ran. The first half needs nothing but this
# interpreter and runs on all three legs; the second needs a builder that is not
# portable today; the third needs a library no runner has. A step that skipped
# whenever the third was missing would be a step that never ran anywhere, which
# is the state this section was written to leave.
Step 'the browser prototype: template and builder agree, and the page is built and driven where it can be' {
    $tplPath = "$repo/prototype/dna-reader.template.html"
    $pagePath = "$repo/prototype/dna-reader.html"
    $harnessPath = "$repo/prototype/check_page.js"
    $builderPath = "$repo/tools/build-web.ps1"
    foreach ($p in $tplPath, $builderPath, $harnessPath, "$repo/prototype/demo-construct.gb") {
        if (-not (Test-Path -LiteralPath $p)) { throw "$p is missing, so the browser prototype can be neither built nor checked" }
    }

    # FLOORS ON THE INPUTS FIRST, because every corpus-taking oracle in this file
    # refuses an empty input set rather than reporting ok over one. A truncated
    # template and a harness somebody gutted are exactly the two inputs the
    # comparisons below would call healthy.
    $tplRaw = Get-Content -LiteralPath $tplPath -Raw
    $tplLines = @(Get-Content -LiteralPath $tplPath).Count
    if ($tplLines -lt 900) { throw "prototype/dna-reader.template.html is $tplLines line(s); it was 1,257 when this floor was written and a file this short is not that page" }
    $harness = Get-Content -LiteralPath $harnessPath -Raw
    $assertions = ([regex]::Matches($harness, '(?m)^\s*T\(')).Count
    if ($assertions -lt 15) { throw "prototype/check_page.js makes $assertions assertion(s); it made 24 when this floor was written, and a harness with most of them deleted still ends by printing ALL CHECKS PASSED" }

    # The builder's placeholders come from its AST, not from a grep over its
    # text. A comment naming `{{BUILD_STAMP}}` is not a substitution, and a
    # checker that cannot tell code from prose gets silenced rather than
    # satisfied -- which the release-workflow checker further down this file
    # already had to learn once.
    $toks = $null
    $errs = $null
    $bast = [System.Management.Automation.Language.Parser]::ParseFile($builderPath, [ref]$toks, [ref]$errs)
    if ($errs) { throw "tools/build-web.ps1 does not parse: $($errs[0])" }
    $inBuilder = @($bast.FindAll({ param($n)
                $n -is [System.Management.Automation.Language.StringConstantExpressionAst] -and
                $n.Value -match '^\{\{[A-Z0-9_]+\}\}$' }, $true) |
        ForEach-Object { $_.Value } | Sort-Object -Unique)
    $inTemplate = @([regex]::Matches($tplRaw, '\{\{[A-Z0-9_]+\}\}') |
        ForEach-Object { $_.Value } | Sort-Object -Unique)
    if ($inBuilder.Count -lt 3) { throw "only $($inBuilder.Count) placeholder(s) parsed out of tools/build-web.ps1; it substitutes three, so the parser is broken and this comparison would agree with anything" }
    if ($inTemplate.Count -lt 3) { throw "only $($inTemplate.Count) placeholder(s) found in the template; it carries three, so this comparison would agree with anything" }
    $problems = @(Compare-WebPlaceholders $inTemplate $inBuilder)

    # PLANTED INPUT. Two sets agree whenever they agree, including when both are
    # wrong in the same way and including when the comparison has stopped
    # comparing, so it is run once over a pair known to differ IN BOTH
    # DIRECTIONS. Delete either loop in `Compare-WebPlaceholders` and this counts
    # one where it must count two, and the step goes red.
    $planted = @(Compare-WebPlaceholders @('{{WASM_BASE64}}', '{{BUILD_STAMP_RENAMED}}') @('{{WASM_BASE64}}', '{{BUILD_STAMP}}'))
    if ($planted.Count -ne 2) { throw "the placeholder comparison reported $($planted.Count) of 2 planted disagreements, so it does not notice a renamed placeholder in both directions and proves nothing" }
    if ($problems) { throw ($problems -join "`n        ") }
    Write-Host ("        {0} placeholder(s), template and builder agree; comparison verified against a renamed one" -f $inTemplate.Count) -ForegroundColor DarkGray

    # THE BUILD, WHERE IT CAN RUN -- AND WHY THAT IS NOT EVERYWHERE.
    #
    # `tools/build-web.ps1` is Windows-only, and NOT because its subject is. Its
    # line 28 is `Join-Path $env:USERPROFILE '.cargo\bin'`, and off Windows that
    # variable is ABSENT rather than empty, which makes `Join-Path` a terminating
    # error -- the exact failure this file's own header records as having killed
    # both non-Windows legs of the first release. Its four paths are joined with
    # backslashes besides (lines 31 to 33 and 64), which name nothing on Linux.
    # So the limit is a scripting defect in the builder, not the artefact's:
    # a single HTML file with a wasm module inlined into it is as portable as
    # anything in this tree.
    #
    # It is guarded HERE rather than declared `WindowsOnly` on purpose.
    # `WindowsOnly` is this file's one spelling for "the subject is a Win32
    # artefact -- a PE resource directory, an 8.3 name, the registry, msiexec",
    # seven steps say it and the header counts them; an eighth saying it for a
    # fixable `Join-Path` would make the word mean two things and quietly retire
    # the header sentence that gives it meaning. When build-web.ps1 is made
    # portable, delete the `if ($onWindows)` and this builds on all three legs
    # with nothing else changed.
    $built = $false
    $wasmPath = "$repo/target/wasm32-unknown-unknown/wasm/pl_wasm.wasm"
    if (-not $onWindows) {
        Write-Host '        not built here: tools/build-web.ps1 is not portable (see the comment above this line)' -ForegroundColor DarkGray
    } elseif (-not (Test-Path -LiteralPath $wasmPath)) {
        Write-Host '        not built here: the wasm module is absent, so the wasm32 step above did not run' -ForegroundColor DarkGray
    } else {
        $buildLog = & $builderPath -SkipBuild *>&1
        if (-not (Test-Path -LiteralPath $pagePath)) { throw ('tools/build-web.ps1 wrote no prototype/dna-reader.html: ' + ($buildLog -join ' | ')) }
        $page = Get-Content -LiteralPath $pagePath -Raw
        # Rebuilding rather than reading whatever is on disk IS the check.
        # `prototype/dna-reader.html` is gitignored, so there is no committed
        # artefact to diff against and nothing in the tree says how old the local
        # one is. The copy found by audit on this machine was four commits behind
        # its template, and the harness passed against it. Built here, every run,
        # the page the harness drives is the page this template describes.
        foreach ($p in $inTemplate) { if ($page.Contains($p)) { throw "the built page still contains $p, so build-web.ps1 did not substitute it" } }
        if ($page.Length -le $tplRaw.Length) { throw "the built page is $($page.Length) bytes against a $($tplRaw.Length)-byte template, so the wasm module was not inlined into it" }
        $built = $true
        Write-Host ("        built prototype/dna-reader.html, {0:N0} bytes, no placeholder left in it" -f $page.Length) -ForegroundColor DarkGray
    }

    # THE BEHAVIOURAL HALF, AND ONLY OVER A PAGE THIS RUN BUILT.
    #
    # Driving whatever `dna-reader.html` happens to be lying on the disk is the
    # defect and not the check, for the reason given above, so `$built` gates it
    # rather than `Test-Path`.
    #
    # `prototype/check_page.js` needs jsdom, and JSDOM IS ON NO RUNNER: both
    # `prototype/package.json` and `node_modules/` are gitignored, so a fresh
    # checkout has neither and `require("jsdom")` throws. That is reported here
    # rather than assumed, and what would close it is one line in
    # `.github/workflows/ci.yml` -- `npm install --no-save jsdom` in `prototype/`
    # on all three legs, or a committed package.json and lockfile there plus
    # `npm ci`, the way `packages/circular-map` is already provisioned. Until
    # that lands, what runs on a runner is the contract above, which is not
    # nothing: it has floors on both inputs and a planted control.
    $haveJsdom = $false
    if (Have node) {
        Push-Location prototype
        try { node -e 'require.resolve("jsdom")' *>&1 | Out-Null; $haveJsdom = ($LASTEXITCODE -eq 0) } finally { Pop-Location }
    }
    if ($built -and $haveJsdom) {
        Push-Location prototype
        try {
            $chk = & node check_page.js 2>&1
            $chkCode = $LASTEXITCODE
        } finally { Pop-Location }
        $passes = @($chk | Where-Object { "$_" -match '^\s+PASS\s' }).Count
        if ($chkCode -ne 0) { $chk | ForEach-Object { Write-Output $_ }; throw "prototype/check_page.js failed against the page this step has just built ($passes assertion(s) passed before it stopped)" }
        # A FLOOR ON WHAT IT REACHED, not on what it printed. `check_page.js`
        # ends with ALL CHECKS PASSED whenever nothing failed, INCLUDING when it
        # was handed no corpus and skipped its own comparisons -- the shape this
        # whole file exists to refuse. Driven with no corpus, as here, 20 of its
        # 24 assertions are reached and pass; a run that reaches fewer than
        # fifteen has stopped reaching them for some other reason.
        if ($passes -lt 15) { throw "prototype/check_page.js exited 0 having passed only $passes assertion(s); it carries $assertions, and a harness that stopped reaching them exits 0 in exactly the same way" }
        Write-Host ("        page driven in jsdom: {0} assertion(s) passed" -f $passes) -ForegroundColor DarkGray
    } elseif ($built) {
        Write-Host '        not driven here: jsdom does not resolve from prototype/ (npm install --no-save jsdom there)' -ForegroundColor DarkGray
    }
    $global:LASTEXITCODE = 0
}

Write-Host "`npath arithmetic (the helper three scripts each carry a copy of)" -ForegroundColor Cyan

# THE COPIES OF `Get-DirectoryPrefix`, FOUND BY PARSING RATHER THAN BY PATH.
#
# `tools/ci.ps1`, `tools/release.ps1` and `tools/installer/Install-Polylinker.ps1`
# each define this function, byte for byte. That duplication is deliberate and is
# explained where each copy sits; the short version is that the installer SHIPS
# ALONE inside the release zip -- `release.ps1` copies it to the archive root as a
# single flat file, with nothing from `tools/` beside it -- so it cannot dot-source
# a shared module that will not be there. A copy in each is the cost of that.
#
# Nothing scans by path. A fourth copy in a fifth script is exactly the thing this
# is for, and a list of three known files would not see it. Line numbers are no
# better: the three moved three times on 2026-08-10 alone.
#
# `-Recurse -Include` over the whole tools tree, and `git ls-files` says every
# .ps1 and .psm1 in this repository is under it, so the tree is the whole search
# space rather than a convenient subset of it.
function Find-PrefixHelperCopies {
    param([string]$ToolsDir)
    $found = @()
    foreach ($f in (Get-ChildItem -Path $ToolsDir -Recurse -File -Include *.ps1, *.psm1 | Sort-Object FullName)) {
        $toks = $null
        $errs = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$toks, [ref]$errs)
        if ($errs) { throw "$($f.FullName) does not parse: $($errs[0])" }

        # CALLERS, so that a copy DELETED is caught as what it is. The rot this
        # section exists to stop is somebody tidying the duplication away into a
        # dot-sourced module: the call sites stay, the definition goes, and the
        # shipped installer dies on a user's machine at a command it no longer
        # has. The floor below would notice the missing copy, but would report it
        # as a count; this reports the actual mistake, and it also catches the case
        # the floor cannot -- a NEW file that calls the helper without defining it,
        # where all three original copies are still present and the count is fine.
        # A CommandAst is a call; the mention in `build-msi.ps1` is inside a
        # comment and is correctly not one.
        $calls = @($ast.FindAll({ param($n)
                    $n -is [System.Management.Automation.Language.CommandAst] -and
                    $n.GetCommandName() -eq 'Get-DirectoryPrefix' }, $true))

        $defs = @($ast.FindAll({ param($n)
                    $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                    $n.Name -eq 'Get-DirectoryPrefix' }, $true))

        if ($calls.Count -and -not $defs.Count) {
            throw ("$($f.FullName) calls Get-DirectoryPrefix $($calls.Count) time(s) and does not define it. " +
                   'If the definition has been moved to a shared file, note that the installer ships alone ' +
                   'inside the release zip and can dot-source nothing.')
        }

        foreach ($d in $defs) {
            # NORMALISED BY TOKENISING, not by hashing lines at fixed offsets.
            # Dropping the Comment and NewLine tokens and joining the rest with a
            # single space makes indentation, blank lines, CRLF-vs-LF-vs-CR and an
            # added explanatory comment all invisible, while a changed argument or
            # a dropped `TrimEnd` is not. Measured, all six of those perturbations:
            # the first four compare equal and the last two do not.
            #
            # The whole definition is compared, not just the brace body: the
            # parameter list is behaviour too, and `[string[]]$Path` instead of
            # `[string]$Path` is drift a body-only comparison would wave through.
            $inside = @($toks | Where-Object {
                    $_.Extent.StartOffset -ge $d.Extent.StartOffset -and
                    $_.Extent.EndOffset -le $d.Extent.EndOffset -and
                    $_.Kind -ne [System.Management.Automation.Language.TokenKind]::Comment -and
                    $_.Kind -ne [System.Management.Automation.Language.TokenKind]::NewLine })
            $found += [pscustomobject]@{
                Path   = $f.FullName.Substring($repo.Length + 1).Replace('\', '/')
                Line   = $d.Extent.StartLineNumber
                Text   = $d.Extent.Text
                Tokens = @($inside | ForEach-Object { $_.Text })
                Norm   = (@($inside | ForEach-Object { $_.Text }) -join ' ')
            }
        }
    }
    return $found
}

Step 'Get-DirectoryPrefix is one function copied, not three functions drifting' {
    $copies = @(Find-PrefixHelperCopies (Join-Path $repo 'tools'))

    # A FLOOR, because a scan that matched nothing would report ok. This is the
    # same shape as the parse floor in 'every file the release reads is
    # committed': the failure mode of a finder is finding nothing quietly.
    if ($copies.Count -lt 3) {
        throw ("only $($copies.Count) definition(s) of Get-DirectoryPrefix found under tools/; there are three " +
               '(ci.ps1, release.ps1, installer/Install-Polylinker.ps1). One has been renamed or deleted, and ' +
               'this guard has been protecting nothing since.')
    }

    # Grouped rather than compared pairwise, so the message can name the odd one
    # out instead of reporting that two unnamed files differ.
    $groups = @($copies | Group-Object -Property Norm | Sort-Object -Property @{ E = 'Count'; D = $true })
    if ($groups.Count -gt 1) {
        $ref = $groups[0]
        $lines = @('the copies of Get-DirectoryPrefix have drifted apart.',
                   "  agreed on by $($ref.Count): $(($ref.Group | ForEach-Object { "$($_.Path):$($_.Line)" }) -join ', ')")
        foreach ($g in $groups[1..($groups.Count - 1)]) {
            foreach ($c in $g.Group) {
                # WHERE it disagrees, not merely that it does. The first differing
                # token is what a reader needs; a dump of six lines they can
                # already open in an editor is not.
                $a = $ref.Group[0].Tokens
                $b = $c.Tokens
                $i = 0
                while ($i -lt [Math]::Min($a.Count, $b.Count) -and $a[$i] -ceq $b[$i]) { $i++ }
                $expect = if ($i -lt $a.Count) { "'$($a[$i])'" } else { '<end of function>' }
                $actual = if ($i -lt $b.Count) { "'$($b[$i])'" } else { '<end of function>' }
                $ctx = ($b[[Math]::Max(0, $i - 4)..[Math]::Min($b.Count - 1, $i + 4)]) -join ' '
                $lines += "  $($c.Path):$($c.Line) differs at token $($i + 1): expected $expect, found $actual"
                $lines += "      near: ... $ctx ..."
            }
        }
        $lines += ('  The duplication is deliberate -- the installer ships alone in the release zip and can ' +
                   'dot-source nothing -- so the fix is to bring the copies back into line, not to share them.')
        throw ($lines -join "`n        ")
    }

    Write-Host ("        $($copies.Count) identical copies: " +
                "$(($copies | ForEach-Object { "$($_.Path):$($_.Line)" }) -join ', ')") -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

# A DIRECTORY REACHED THROUGH A REAL 8.3 ALIAS, DISCOVERED RATHER THAN CREATED.
#
# 8.3 alias creation is a per-volume setting and it is off on this machine:
# measured, a freshly created `...\pl-mint-attempt-longname-23076` has a ShortPath
# equal to its long path, so the case cannot be minted on demand. What CAN be done
# is to ask the OS for the short spelling of a directory that already has one.
# Measured here: `C:\ProgramData` -> `C:\PROGRA~3`, `C:\Program Files` ->
# `C:\PROGRA~1`. ProgramData is also writable without elevation, which is what
# makes a self-contained tree with known names possible; Program Files is not, and
# is deliberately not a candidate.
#
# Two ways in, because either alone has a hole. A path may ALREADY be spelled with
# an alias -- the real-world case, and the one that matters most: a GitHub
# runner's `$env:TEMP` is `C:\Users\RUNNER~1\AppData\Local\Temp` over a real
# `runneradmin`. Or it may have an alias that nothing has spelled, which is
# ProgramData here and needs `Scripting.FileSystemObject` to reveal.
#
# THE INCIDENTAL COVERAGE THIS REPLACES. Because a runner's `$env:TEMP` carries an
# alias, 'release script and its manifest' has been exercising this defect for
# real on every CI run -- that is how run 31325886841 found it. But that coverage
# is an accident of how GitHub names its build account. Rename `runneradmin` to
# something that fits in 8.3 and the alias case stops being tested, every gate
# stays green, and nothing reports the loss. This step is what reports it: it
# names the case it needs, and if it cannot find one it SKIPs -- and on the runner
# a skip is a build failure, because the skip list is set equality in both
# directions and this step is deliberately not on it.
$script:aliasSubject = $null
$script:aliasWhy = @()
$aliasFso = $null
$aliasCandidates = @()
# Guarded, each on one line with its own test, for the reason the step named 'the
# cross-platform scripts touch no environment variable unguarded' gives at
# length: off Windows these are absent, not empty.
if ($env:TEMP) { $aliasCandidates += $env:TEMP }
if ($env:ProgramData) { $aliasCandidates += $env:ProgramData }
if ($env:LOCALAPPDATA) { $aliasCandidates += $env:LOCALAPPDATA }
foreach ($cand in $aliasCandidates) {
    if (-not (Test-Path -LiteralPath $cand)) { $script:aliasWhy += "$cand does not exist"; continue }
    $spellings = @($cand)
    if ($null -eq $aliasFso) {
        try { $aliasFso = New-Object -ComObject Scripting.FileSystemObject } catch { $aliasFso = $false }
    }
    if ($aliasFso) { try { $spellings += $aliasFso.GetFolder($cand).ShortPath } catch { } }

    # The test for "this spelling carries an alias" is that GetFullPath CHANGES
    # it -- not that the string contains a `~`. A directory may legitimately be
    # named with one, and would then be mistaken for an alias that expands to
    # itself: a subject on which old and new arithmetic agree, which is precisely
    # the subject that proves nothing.
    $aliased = @($spellings | Where-Object { $_ -and [System.IO.Path]::GetFullPath($_) -cne $_ })[0]
    if (-not $aliased) { $script:aliasWhy += "$cand has no 8.3 alias in any component"; continue }

    $probe = Join-Path $aliased ('pl-83-check-' + $PID)
    try {
        New-Item -ItemType Directory -Force -Path (Join-Path $probe 'features') -ErrorAction Stop | Out-Null
        Set-Content -LiteralPath (Join-Path $probe 'features/NOTICE.txt') -Value 'notice' -ErrorAction Stop
        Set-Content -LiteralPath (Join-Path $probe 'top.txt') -Value 'top' -ErrorAction Stop
        $script:aliasSubject = $probe
        break
    } catch {
        Remove-Item -Recurse -Force $probe -ErrorAction SilentlyContinue
        $script:aliasWhy += "$aliased carries an alias but is not writable ($($_.Exception.GetType().Name))"
    }
}
if ($aliasFso) { [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($aliasFso) }

Step 'the helper unmangles a real 8.3 alias and the arithmetic it replaced does not' {
    try {
        $sep = [System.IO.Path]::DirectorySeparatorChar
        $subject = $script:aliasSubject
        $longRoot = [System.IO.Path]::GetFullPath($subject)
        # `features/NOTICE.txt` on purpose: run 31325886841 died reporting
        # `pl-release-check-6584\84\features\NOTICE.txt`, and a subject shaped like
        # the original failure makes the old arithmetic's output recognisable.
        $expected = 'features/NOTICE.txt', 'top.txt'
        $problems = @()

        # EVERY COPY IS DRIVEN, not just this file's. The drift guard above proves
        # the three agree; this proves what they agree ON, and extracting each
        # definition means neither step has to be believed on the other's word. It
        # is also the only way the shipped installer's copy -- the user-facing one,
        # and the likeliest to be forgotten -- gets its behaviour tested without
        # running an installer.
        #
        # WHAT THIS STEP DOES NOT COVER, so that a green line here is not read as
        # more than it is: it drives the DEFINITIONS. A CALL SITE that stops using
        # the helper and goes back to doing the arithmetic inline is invisible
        # here, because there is then no definition carrying the defect to
        # extract. Measured rather than reasoned: reverting `release.ps1`'s
        # `$outFull` to `(Resolve-Path $Out).Path` with
        # `Substring($outFull.Length + 1)` left both steps in this section green,
        # and was caught one section down by 'release script and its manifest',
        # which died on `pl-release-check-44516\EADME-WINDOWS.txt`. That division
        # of labour is deliberate -- that step hands `release.ps1` an `-Out` with
        # a trailing separator precisely so a call site cannot regress unwatched
        # -- but it is why this step's name says "the helper" and not "the
        # repository's path arithmetic".
        $copies = @(Find-PrefixHelperCopies (Join-Path $repo 'tools'))

        # THE SAME FLOOR THE DRIFT GUARD HAS. The first draft of this step did not
        # have one: with `Find-PrefixHelperCopies` returning nothing the loop below
        # runs zero times, `$problems` stays empty and the step reports ok -- a
        # step that silently passes when it could not test anything, inside the
        # step whose whole purpose is to stop that. It surfaced while writing the
        # injection that renames the function away, before that injection was run,
        # which is an argument for writing the injections rather than only the
        # checks. With the floor in place that injection fails here as it should.
        if ($copies.Count -lt 3) {
            throw ("only $($copies.Count) definition(s) of Get-DirectoryPrefix found under tools/; there are " +
                   'three, so this step drove nothing and proved nothing')
        }
        $demonstrated = 0

        foreach ($copy in $copies) {
            $invoke = [ScriptBlock]::Create($copy.Text + "`nGet-DirectoryPrefix `$args[0]")

            # Both spellings. The bare alias is the runner's `$env:TEMP`; the alias
            # WITH a trailing separator is what cmd's `%~dp0` hands `-Source`, which
            # the installer documents as carrying both defects at once. The helper
            # must return the identical string for the two.
            foreach ($given in @($subject, ($subject + $sep))) {
                $prefix = & $invoke $given
                $old = (Resolve-Path -LiteralPath $given).Path   # what the code did before 2026-08-09

                if ($prefix -cne ($longRoot + $sep)) {
                    $problems += "$($copy.Path): given '$given' the prefix was '$prefix', expected '$longRoot$sep'"
                    continue
                }

                # THE SUBJECT MUST ACTUALLY CARRY THE DISAGREEMENT. Without this, a
                # machine whose candidates had no alias would run the whole
                # comparison over a path where old and new agree and report ok
                # having tested nothing -- the exact failure this step exists to
                # prevent, reproduced inside the step.
                if ($old.TrimEnd($sep).Length -eq $longRoot.Length) {
                    $problems += ("$($copy.Path): '$given' and '$longRoot' are the same length, so no Substring " +
                                  'could have been wrong here and this comparison proved nothing')
                    continue
                }

                $newNames = @()
                $oldNames = @()
                foreach ($f in (Get-ChildItem -LiteralPath $given -Recurse -File)) {
                    $newNames += $f.FullName.Substring($prefix.Length).Replace('\', '/')
                    $oldNames += $f.FullName.Substring($old.Length + 1).Replace('\', '/')
                }
                $newNames = @($newNames | Sort-Object)
                $oldNames = @($oldNames | Sort-Object)

                if (($newNames -join '|') -cne ($expected -join '|')) {
                    $problems += ("$($copy.Path): given '$given' the names came back " +
                                  "'$($newNames -join ', ')', expected '$($expected -join ', ')'")
                }

                # AND THE OTHER HALF, which is what makes this a test rather than a
                # tautology: the arithmetic the helper replaced must still be WRONG
                # on this input. `(Resolve-Path).Path` with `Length + 1` is verbatim
                # what release.ps1 did until 2026-08-09. If it agrees with the
                # helper then the input carried no alias, and the assertion above
                # was satisfied by an accident rather than by the fix.
                if (($oldNames -join '|') -ceq ($newNames -join '|')) {
                    $problems += ("$($copy.Path): given '$given' the OLD Resolve-Path arithmetic agreed with the " +
                                  'helper, so this input never exercised the defect')
                } else {
                    $demonstrated++
                    if ($given -eq $subject -and $copy.Path -eq 'tools/ci.ps1') {
                        Write-Host "        old arithmetic on '$given' -> $($oldNames -join ', ')" -ForegroundColor DarkGray
                    }
                }
            }
        }
        if ($problems) { throw ($problems -join "`n        ") }

        # AND THE COUNT, so that a comparison skipped by a `continue` above cannot
        # leave the step reporting ok for work it did not do. Two spellings per
        # copy, every copy.
        if ($demonstrated -ne $copies.Count * 2) {
            throw ("$demonstrated of the expected $($copies.Count * 2) comparisons actually ran; the rest were " +
                   'skipped, so this step is reporting on less than it claims')
        }
        Write-Host ("        $demonstrated comparisons over $($copies.Count) copies; " +
                    "the helper on the same input -> $($expected -join ', ')") -ForegroundColor DarkGray
        $global:LASTEXITCODE = 0
    } finally {
        Remove-Item -Recurse -Force $script:aliasSubject -ErrorAction SilentlyContinue
    }
} {
    # An 8.3 short name is an NTFS/FAT compatibility feature and there is no
    # such thing to unmangle off Windows -- the arithmetic this step contrasts
    # cannot be wrong there, because Resolve-Path and GetFullPath agree.
    #
    # ON WINDOWS THE FALLBACK IS STILL $false, DELIBERATELY, and that is the
    # whole point of the paragraph in .github/ci-expected-skips.txt about this
    # step being absent from the list. $false is a skip with no declared
    # reason, and under -ExpectedSkips a skip with no declared reason FAILS. So
    # the day a runner's TEMP stops carrying an alias, the loss of coverage is
    # reported instead of passing silently, exactly as before -- and there is
    # no vocabulary entry anyone could add to quiet it without also making it
    # legal on all three platforms.
    WindowsOnly { $null -ne $script:aliasSubject }
}
if (-not $script:aliasSubject) {
    Write-Host ('        no directory here is reachable through an 8.3 alias, so the case cannot be exercised: ' +
                ($script:aliasWhy -join '; ')) -ForegroundColor DarkGray
}

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
# The zip the two zip steps read, and the directory it was built from. On
# Windows both are the release above; off Windows they are a second, zip-forced
# build, for the reason set out where they are assigned.
$script:zip = $null
$script:zipDir = $null

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
    $out = Join-Path $tmp ('pl-release-check-' + $PID)
    Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue

    # THE TRAILING SEPARATOR IS DELIBERATE. Do not tidy it away.
    #
    # `release.ps1` computes every name in its manifest by subtracting the output
    # directory's path from each file's `FullName`. That subtraction is only
    # correct if both strings come from the same normaliser, and until 2026-08-09
    # they did not -- which nothing on the author's machine could show, because
    # the two spellings of `%TEMP%` there happen to be identical. A GitHub runner
    # showed it instead: `C:\Users\RUNNER~1\...` against a real `runneradmin` is a
    # 3-character difference, and CI run 31325886841 died with
    # `pl-release-check-6584\84\features\NOTICE.txt`.
    #
    # An 8.3 alias cannot be conjured on demand -- whether a volume even has one
    # is a per-volume setting -- so this uses the other spelling that breaks the
    # same arithmetic and IS available everywhere. Measured: with a trailing
    # separator the old `Substring($base.Length + 1)` cut one character too many
    # and `a.txt` came back as `.txt`. So this line makes the defect reproduce on
    # every machine that runs the gate, rather than only on the one that did.
    #
    # `$script:release` keeps the plain spelling: the point is to hand the
    # SCRIPT an awkward argument, not to make every later step carry it.
    $spelling = $out + [System.IO.Path]::DirectorySeparatorChar
    & "$PSScriptRoot/release.ps1" -Out $spelling -Quiet 2>&1 | Out-Null
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
    # THE ARCHIVE AND ITS CHECKSUM SIDECAR are the two files that cannot be in
    # the manifest, because they are built from it. They are found FIRST and
    # excluded BY NAME, and both halves of that sentence are the fix for a real
    # failure rather than tidying.
    #
    # This used to read `-notlike '*.zip' -and -notlike '*.zip.sha256'`, a suffix
    # list written when the only platform the gate ran on was the only platform
    # whose default container is a zip. On the first Linux and macOS runs of this
    # step -- run 31359657821 -- the default container was
    # `polylinker-0.4.0-linux-x64.tar.gz`, no `-notlike` matched it, and the set
    # equality below reported the archive and its sidecar as "shipped but not in
    # the manifest". A second suffix in that list would have fixed the symptom
    # and left the next container format to find the same way, so the list is
    # gone: `$archive` below is what `release.ps1` actually produced, and it is
    # that name -- not a pattern that might match it -- which is excluded.
    #
    # The discovery moved UP from below the comparison rather than the exclusion
    # moving down, because `$archive` is also what the two zip steps read: with
    # the throw above them, they reported "no zip was produced" on both legs and
    # the real defect was one line further up. A step that dies before it sets
    # what three later steps need turns one failure into three.
    #
    # THE CONTAINER release.ps1 CHOOSES BY DEFAULT ON THIS PLATFORM, and the
    # default is the thing under test rather than an inconvenience to route
    # around: `.github/workflows/release.yml` runs `./tools/release.ps1 -Out
    # dist` on all three runners with no -ArchiveFormat, so the default IS what
    # ships. This line read `'*.zip'` and 'no Windows zip was produced', which
    # was true of the only platform it ever ran on and would have been a
    # confident lie on the other two.
    $wantArchive = if ($onWindows) { '*.zip' } else { '*.tar.gz' }
    $archive = @(Get-ChildItem -LiteralPath $out -Filter $wantArchive)
    if ($archive.Count -ne 1) {
        throw ("release.ps1's default archive format produced $($archive.Count) $wantArchive here, and " +
               'exactly one is expected; release.yml relies on that default')
    }
    $archive = $archive[0]
    if (-not (Test-Path "$($archive.FullName).sha256")) { throw 'the archive has no checksum sidecar' }
    $notHashed = @('SHA256SUMS.txt', $archive.Name, "$($archive.Name).sha256")

    # THE ZIP, WHICH OFF WINDOWS THE DEFAULT DID NOT PRODUCE.
    #
    # The two steps below read a zip: entry order, pinned timestamps, one root
    # directory. None of that is a Windows property -- the zip is hand-written
    # in release.ps1 precisely because Compress-Archive cannot pin a timestamp
    # -- and ENTRY ORDER in particular is the one assertion whose input differs
    # per platform, because it is the order the filesystem enumerated. So the
    # answer here is the same one `-ArchiveFormat` was invented for one section
    # below, where a Windows machine builds the Unix container: off Windows,
    # build the Windows one.
    #
    # On Windows this is the archive already built above and costs nothing; the
    # extra `release.ps1` run happens on Linux and macOS only, where the
    # workspace is already built and the run is a copy, a hash and a zip.
    #
    # UP HERE, ABOVE EVERY ASSERTION IN THIS STEP, and that placement is a fix
    # rather than a tidy. `$script:zip` is what the two zip steps read, so while
    # this sat at the foot of the step ANY throw above it left them reporting
    # "no zip was produced" -- a sentence about the zip writer, describing a
    # defect in the manifest arithmetic twenty lines earlier. Run 31359657821
    # showed one failure as three that way, and run 31360902875 showed it again
    # after the first cause was fixed and a second took its place. What the zip
    # steps check is a property of the zip; they should not be able to fail for
    # anything else. Nothing below this line is needed to build one.
    if ($onWindows) {
        $script:zipDir = $out
        $script:zip = $archive.FullName
    } else {
        $zipOut = Join-Path $tmp ('pl-release-zip-' + $PID)
        Remove-Item -Recurse -Force $zipOut -ErrorAction SilentlyContinue
        & "$PSScriptRoot/release.ps1" -Out $zipOut -ArchiveFormat zip -Quiet 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'release.ps1 -ArchiveFormat zip failed' }
        $z = @(Get-ChildItem -LiteralPath $zipOut -Filter '*.zip')
        if ($z.Count -ne 1) { throw "release.ps1 -ArchiveFormat zip produced $($z.Count) zip(s), and one is expected" }
        $script:zipDir = $zipOut
        $script:zip = $z[0].FullName
    }

    # `Get-DirectoryPrefix`, not `$out.Length + 1`. The same defect as in
    # `release.ps1`, in the check that exists to audit `release.ps1` -- so with
    # the script fixed and this line left alone, the runner would simply have
    # failed one line further down, with all 22 files reported as "shipped but
    # not in the manifest". A fix applied only where the failure happened to
    # surface is how this class recurred after ci.ps1 fixed it once already
    # (see 'installer plan covers the whole release' below).
    #
    # `$spelling`, the same awkward argument `release.ps1` was given, and not
    # `$out`. `Get-DirectoryPrefix` returns the identical string for both, so
    # this is not a behaviour change -- it is what makes the defect REACHABLE
    # here. Against plain `$out` on a machine whose `%TEMP%` holds no 8.3 alias,
    # the old `$out.Length + 1` is correct by accident and reverting this line
    # changes nothing that any local run could see; only a runner would notice.
    # Measured, and it is the whole reason the trailing separator exists above.
    $outPrefix = Get-DirectoryPrefix $spelling
    $onDisk = Get-ChildItem -LiteralPath $spelling -Recurse -File |
        ForEach-Object {
            if (-not $_.FullName.StartsWith($outPrefix, [StringComparison]::OrdinalIgnoreCase)) {
                throw "$($_.FullName) is not under $outPrefix, so its name cannot be compared with the manifest"
            }
            $_.FullName.Substring($outPrefix.Length).Replace('\', '/')
        } |
        Where-Object { $notHashed -notcontains $_ }
    $unhashed = @($onDisk | Where-Object { $listed -notcontains $_ })
    if ($unhashed) { throw "shipped but not in the manifest: $($unhashed -join ', ')" }

    # A floor as well, because set equality alone is satisfied by a release of
    # nothing agreeing with a manifest of nothing. Twenty-one is what the current
    # `$artifacts` + `$notices` + installer produce ON WINDOWS; raise it when they
    # grow.
    #
    # It read sixteen until 2026-08-05, by which time the release had been
    # eighteen files for a while: the floor had become two files of slack rather
    # than a floor, which is the same drift it exists to catch. Nineteen was
    # those eighteen plus polylinker.ico, which `release.ps1` began shipping that
    # day; twenty is nineteen plus LICENSE-MIT.txt, added on 2026-08-06 because
    # the repository had been offering `MIT OR Apache-2.0` while shipping only
    # the Apache half; twenty-one is twenty plus licences/Inter-OFL.txt, added
    # on 2026-08-09 with the heading face.
    #
    # EIGHTEEN OFF WINDOWS, AND NOT TWENTY-ONE EVERYWHERE MINUS SLACK. This was a
    # bare `21` and the macOS leg of run 31360902875 failed on it with "only 18
    # file(s) in the manifest" -- correctly, because the Windows release carries
    # three files no other platform gets: `Install-Polylinker.ps1`, `Install.cmd`
    # and `polylinker.ico`, the installer set `release.ps1` ships behind
    # `if ($onWindows)` and says at that block is deliberate rather than an
    # oversight. (The read-me is a fourth Windows-only NAME but not a fourth
    # file: README-WINDOWS.txt is swapped for README-LINUX.txt or
    # README-MACOS.txt, one either way.)
    #
    # The tempting repair is to lower the number until it holds everywhere. That
    # would buy a green macOS leg with three files of permanent slack on the
    # platform the installer actually ships to, which is the platform whose
    # release this floor exists to protect -- a weaker check running in three
    # places instead of a strong one running in one. So the floor is exact on
    # each platform and the difference between the two numbers is the list of
    # files above, the same shape as `$floor` in the tar step below.
    $minFiles = if ($onWindows) { 21 } else { 18 }
    if ($listed.Count -lt $minFiles) {
        throw "only $($listed.Count) file(s) in the manifest; at least $minFiles are expected here"
    }

    # And the specific obligation named in NOTICE, spelled out once here because
    # this is the assertion whose failure is a licence violation rather than a
    # papercut. Counted, not listed, for the reason above.
    #
    # Eight since 2026-08-09. Inter's OFL is the third copy of that licence in
    # the set and is not redundant: it is the only one of the three that
    # declares no Reserved Font Name, which is the clause the shipped face — a
    # PUA-stripped subset, and so a Modified Version — actually relies on.
    $lic = @($listed | Where-Object { $_ -like 'licences/*' })
    if ($lic.Count -lt 8) { throw "only $($lic.Count) font licence text(s) shipped; NOTICE requires 8" }
    foreach ($required in 'NOTICE.txt', 'LICENSE.txt', 'LICENSE-MIT.txt', 'features/NOTICE.txt') {
        if ($listed -notcontains $required) { throw "$required did not ship" }
    }

    Write-Host ("        $($listed.Count) file(s) hashed, $($lic.Count) licence texts, manifest verified; " +
                "default container $($archive.Name)") -ForegroundColor DarkGray
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
} {
    # A version block and an RT_GROUP_ICON are entries in a PE image's resource
    # directory. An ELF and a Mach-O have no such section, VersionInfo comes
    # back empty off Windows, and the release there ships no .ico at all
    # (release.ps1 sends a README instead of the installer set), so there is
    # nothing here to port rather than a spelling to fix.
    WindowsOnly { [bool]$script:release }
}

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
} {
    # VCRUNTIME140.dll is a Windows redistributable and the scan is over the
    # PE import directory of .exe/.pyd/.dll files. Off Windows none of those
    # four names can appear and none of those three extensions exists, so the
    # step would assert the absence of something that cannot be present -- a
    # check that cannot fail, which this file treats as worse than no check.
    WindowsOnly { [bool]$script:release }
}

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
    $prefix = Join-Path $tmp ('pl-install-plan-' + $PID)
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
    #
    # `-Source` carries a trailing separator for the same reason the release step
    # hands one to `release.ps1`: `Install-Polylinker.ps1` subtracts the source
    # directory from each file's `FullName` to decide whether the copy matches
    # its manifest, and that subtraction had the identical defect. Left alone it
    # is not a CI curiosity -- it is a user with an 8.3 component anywhere in
    # their extraction path being told "this copy is incomplete -- 21 file(s) the
    # manifest lists are not here" about a perfectly good download.
    $host_ = (Get-Process -Id $PID).Path
    $out = & $host_ -NoProfile -File "$PSScriptRoot/installer/Install-Polylinker.ps1" `
        -DryRun -Prefix $prefix -Source ($script:release + [System.IO.Path]::DirectorySeparatorChar) `
        -RegistryRoot "HKCU\Software\Polylinker-CI-$PID" `
        -StateDir (Join-Path $tmp "pl-state-$PID") `
        -StartMenuDir (Join-Path $tmp "pl-startmenu-$PID") `
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
    #
    # BOTH SIDES THROUGH THE SAME NORMALISER. `$prefix` is built from the
    # temporary directory, which on a GitHub runner is
    # `C:\Users\RUNNER~1\AppData\Local\Temp`, while the destinations
    # the installer prints are absolute paths it has normalised -- so a raw
    # `StartsWith` here compares an 8.3 alias against its expansion and reports
    # that a plan writing only inside the prefix writes outside it. The trailing
    # separator `Get-DirectoryPrefix` guarantees matters too, and separately: a
    # bare `StartsWith($prefix)` also accepts `C:\...\pl-install-plan-123-evil`
    # as being "inside" `C:\...\pl-install-plan-123`, which is the opposite
    # mistake and the one that would let a real escape through.
    $prefixDir = Get-DirectoryPrefix $prefix
    foreach ($line in ($out -split "`r?`n")) {
        if ($line -match '^\s{2}(copy|write|create dir)\s{2,}(\S.*?)(\s+\(|$)') {
            $dest = Get-DirectoryPrefix ($Matches[2].Trim())
            if (-not $dest.StartsWith($prefixDir, [StringComparison]::OrdinalIgnoreCase)) {
                throw "the plan writes outside the prefix: $($Matches[2].Trim())"
            }
        }
    }
    # And the promises the product is built on.
    foreach ($promise in 'contact the network', 'install an updater') {
        if ($out -notmatch [regex]::Escape($promise)) { throw "the plan no longer states that it will not $promise" }
    }
    Write-Host "        $($script:releaseFiles.Count) file(s) in the plan, none outside the prefix" -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
} {
    # `Install-Polylinker.ps1` is a Windows installer -- HKCU, a Start Menu
    # .lnk, ProgIds, an Add/Remove Programs row -- and `release.ps1` says so in
    # its own words where it ships a README instead of it off Windows. There is
    # no Unix installer to plan, so there is no plan to check.
    WindowsOnly { [bool]$script:release }
}

# A real install, into a scratch prefix and a scratch registry root, then a real
# uninstall — with a sentinel planted in the state directory that must survive.
#
# The sentinel is the point. `recovery\*.recover` is unsaved user work rescued
# from a crash, and an uninstaller that removes it has destroyed the only copy
# of somebody's afternoon. That is a promise in prose in three files; this is
# the only thing that makes it a promise in fact.
Step 'installer round trip leaves user state alone' {
    $tag = "pl-rt-$PID"
    $prefix = Join-Path $tmp "$tag-prefix"
    $state  = Join-Path $tmp "$tag-state"
    $menu   = Join-Path $tmp "$tag-menu"
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
} {
    # Same subject as the plan above, and the same answer: this one drives the
    # real thing, so it additionally writes to HKCU and reads a .lnk back.
    WindowsOnly { [bool]$script:release }
}

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
#
# IT RUNS ON ALL THREE PLATFORMS, and property 1 is the reason that is worth
# the second `release.ps1` run this needs off Windows. "Entry order is sorted"
# is an assertion about overriding whatever order the FILESYSTEM enumerated,
# and NTFS, ext4 and APFS enumerate differently -- so on Windows alone the
# check was being fed one of the three inputs it exists to be robust to.
Step 'the zip is a deterministic function of dist/' {
    if (-not $script:zip) { throw 'no zip was produced' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem | Out-Null
    $z = [System.IO.Compression.ZipFile]::OpenRead($script:zip)
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
        # `$rel` unconverted: a zip entry name is always `/`-separated, and
        # Join-Path is happy to be handed one on Windows. `Replace('/', '\')`
        # would have named a nonexistent file off Windows, where a backslash is
        # an ordinary character in a file name.
        foreach ($e in $z.Entries) {
            $rel = $e.FullName.Substring($roots[0].Length + 1)
            $onDisk = Join-Path $script:zipDir $rel
            if (-not (Test-Path -LiteralPath $onDisk)) { throw "the zip contains $rel, which is not in the release directory" }
            if ((Get-Item -LiteralPath $onDisk).Length -ne $e.Length) { throw "$rel differs between the zip and the release directory" }
        }
        Write-Host "        $($names.Count) entries, sorted, pinned timestamps, one root '$($roots[0])'" -ForegroundColor DarkGray
        $global:LASTEXITCODE = 0
    } finally { $z.Dispose() }
} { [bool]$script:release }

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
    if (-not $script:zip) { throw 'no zip was produced' }
    & "$PSScriptRoot/check-archive.ps1" -Archive $script:zip
} { [bool]$script:release }

if ($script:release) { Remove-Item -Recurse -Force $script:release -ErrorAction SilentlyContinue }
# The zip build is a second directory off Windows and the same one on it, so
# this is guarded on being different rather than on the platform: deleting
# $script:release twice is harmless, but saying "if not on Windows" here would
# be a third place that has to agree with the two above.
if ($script:zipDir -and $script:zipDir -ne $script:release) {
    Remove-Item -Recurse -Force $script:zipDir -ErrorAction SilentlyContinue
}

Write-Host "`nrelease workflow" -ForegroundColor Cyan

# THE UNIX ARCHIVE, BUILT AND CHECKED ON THIS MACHINE.
#
# `release.ps1` writes a tar.gz on Linux and macOS -- ustar headers and gzip,
# both hand-written, for the same determinism reason the zip is hand-written and
# additionally because a zip has no portable place to record a file mode, so
# every Unix user would begin with `chmod +x`.
#
# None of that code would otherwise run anywhere anybody looks. This gate is the
# only thing that runs on a developer machine, that machine is Windows, and a
# tar writer whose only exercise is a green job on a runner is a tar writer
# nobody has read the output of. So `-ArchiveFormat` exists and this step forces
# it on every platform: on Windows the payload is the Windows one, which is not
# the point -- the container is. Off Windows the format is also the default, so
# there the forcing is a no-op and the step checks what a user will download.
#
# Checked twice. `check-archive.ps1` reads the tar with this project's own
# reader, and then a real `tar` reads it with somebody else's, which is the same
# argument the oracle steps above make. Windows 11 ships bsdtar as
# System32\tar.exe and Git for Windows ships GNU tar, and between them they
# disagree about enough of the format to be worth having.
#
# HOW MANY READERS IS PLATFORM-DEPENDENT, AND IS ASSERTED RATHER THAN NAMED.
# This step used to be called "...two other tools can read" while running
# however many `Find-Tars` happened to return, so on a machine with one tar the
# title was simply false -- the defect this project keeps finding, prose
# asserting what the code does not do. Windows has two known homes and both the
# author's machine and the runner have both, so two is required there; Linux
# ships GNU tar and macOS ships bsdtar, one each, so one is required there. The
# floor is checked below and the banners are printed either way.
#
# EVERY TAR FOUND, EACH NAMED IN THE LOG. This used to take
# `(Get-Command tar.exe).Source` -- whichever implementation happened to be first
# on PATH -- and print it only in the success line, so the log of a passing run
# recorded which tool had been used and the log of a failing run did not. The
# comment above justified itself on TWO readers disagreeing while the code ran
# exactly one of them, picked by the machine's PATH rather than by this file. On
# the author's box that is bsdtar 3.8.4 in System32; GNU tar 1.35 is installed
# under Git for Windows and is NOT on PATH here, so the second reader the comment
# claimed credit for had never run. Which one a runner picks was unknowable from
# the logs of run 31325886841, because the step died before reaching this line.
#
# So: PATH is searched, the two known absolute homes are probed, the list is
# deduplicated by normalised path, and every survivor is run and reported with
# its version banner. A machine with neither still skips, which is what the
# precondition is for.
#
# `tar$exe`, NOT `tar.exe`. Off Windows the program is called `tar` and this
# function found none, so the step would have skipped on the two platforms
# whose users are the reason the tar.gz exists at all. Linux ships GNU tar and
# macOS ships bsdtar, so each of those runners contributes one reader; Windows
# contributes two, and the loop below reports the count and the version banners
# either way.
function Find-Tars {
    $found = @()
    foreach ($c in @(Get-Command "tar$exe" -All -ErrorAction SilentlyContinue)) { $found += $c.Source }

    # Each guarded by the variable it uses, which is the rule the step 'the
    # cross-platform scripts touch no environment variable unguarded'
    # enforces over this file -- and it enforced it: the first draft
    # of this function read all four unguarded and that step failed the gate,
    # naming three of the four lines. Off Windows these are absent rather than
    # empty and `Join-Path` treats a null `-Path` as a terminating error.
    #
    # All four are Windows install locations, so off Windows every one of these
    # guards is false and the PATH search above is the whole of it -- which is
    # correct: there is no second tar to find on a Unix runner.
    $candidates = @()
    if ($env:SystemRoot)   { $candidates += (Join-Path $env:SystemRoot 'System32\tar.exe') }
    if ($env:ProgramFiles) { $candidates += (Join-Path $env:ProgramFiles 'Git\usr\bin\tar.exe') }
    if ($env:LOCALAPPDATA) { $candidates += (Join-Path $env:LOCALAPPDATA 'Programs\Git\usr\bin\tar.exe') }
    # The one variable whose name is not a legal bare identifier, so it needs
    # `${...}`. That step could NOT see this spelling when this line was written
    # -- it matched `\$env:NAME` only -- so this was the one of the four it let
    # through. The step now reads both spellings; this line is guarded because it
    # should be, not because it was caught.
    if (${env:ProgramFiles(x86)}) { $candidates += (Join-Path ${env:ProgramFiles(x86)} 'Git\usr\bin\tar.exe') }

    foreach ($p in $candidates) {
        if (Test-Path -LiteralPath $p) { $found += $p }
    }
    # Deduplicated through the same normaliser everything else in this file uses,
    # so `C:\PROGRA~1\...` and `C:\Program Files\...` are not counted as two tars.
    $seen = @{}
    $out = @()
    foreach ($f in $found) {
        $k = [System.IO.Path]::GetFullPath($f).ToLowerInvariant()
        if (-not $seen.Contains($k)) { $seen[$k] = $true; $out += $f }
    }
    # `,$out`, AND THE COMMA IS THE WHOLE FIX. PowerShell unrolls a returned
    # array, so a one-element array comes back as the bare STRING it held -- and
    # a string has a .Count of 1 and indexes by character. That is not a
    # theoretical hazard: on macOS `Get-Command tar -All` finds exactly
    # `/usr/bin/tar`, the four Windows candidate paths below are all absent, and
    # `$tools[$i]` in the step below evaluated to `/`. Run 31359657821 failed
    # with "The term '/' is not recognized as a name of a cmdlet".
    #
    # It could not fire anywhere it had ever run. Windows has two tars -- System32's
    # bsdtar and Git for Windows' GNU tar -- so this always returned a real array
    # there, and Ubuntu has two paths to one GNU tar (`/usr/bin/tar` and
    # `/bin/tar`, which this does not dedup because `GetFullPath` does not resolve
    # the `/bin` -> `/usr/bin` symlink). macOS, with one, was the first machine to
    # take the branch. The comma wraps the array in an outer array, which is what
    # gets unrolled instead.
    return ,$out
}

Step 'the tar.gz writer produces an archive other tar implementations can read' {
    $out = Join-Path $tmp ('pl-tar-check-' + $PID)
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
        $tools = Find-Tars
        # The precondition already established there is at least one. If that
        # ever stops being true this must say so rather than pass having read
        # the archive with nothing.
        if (-not $tools) { throw 'no tar was found, so no independent reader checked this archive' }
        # AND THE FLOOR THE OLD STEP NAME ONLY CLAIMED. Two on Windows, where
        # both System32's bsdtar and Git for Windows' GNU tar are present on the
        # runner and on the author's machine; one elsewhere, where the platform
        # ships exactly one. Without this, a PATH change that hid one of the two
        # would halve the coverage silently and the log line would still read
        # like a pass.
        $floor = if ($onWindows) { 2 } else { 1 }
        if ($tools.Count -lt $floor) {
            throw ("only $($tools.Count) tar implementation(s) found and $floor are expected here: " +
                   "$($tools -join ', ')")
        }

        $tested = @()
        for ($i = 0; $i -lt $tools.Count; $i++) {
            $tool = $tools[$i]
            # WHICH TOOL, PRINTED BEFORE IT RUNS. The version banner is the whole
            # point of naming them: "tar.exe" says nothing, `bsdtar 3.8.4` and
            # `tar (GNU tar) 1.35` are the two different readers this step claims
            # to be checking against.
            $banner = (& $tool --version 2>&1 | Select-Object -First 1)
            Write-Host "`n        reader $($i + 1)/$($tools.Count): $tool -- $banner" -ForegroundColor DarkGray

            $dir = "extracted-$i"
            New-Item -ItemType Directory -Force (Join-Path $out $dir) | Out-Null
            Push-Location $out
            try {
                & $tool -xzf $tar.Name -C $dir
            } finally { Pop-Location }
            if ($LASTEXITCODE -ne 0) { throw "$tool ($banner) refused the archive" }

            $root = Get-ChildItem -LiteralPath (Join-Path $out $dir) -Directory
            if ($root.Count -ne 1) { throw "extracting with $tool produced $($root.Count) top-level directories" }
            # FORWARD SLASHES, unconverted. `Replace('/', '\')` was fine on
            # Windows and wrong off it, where a backslash is an ordinary
            # character in a file name rather than a separator, so
            # `licences\Phosphor-MIT.txt` would have named a file that does not
            # exist and the step would have reported the archive as broken.
            # Windows accepts `/` in a path everywhere this uses one.
            foreach ($probe in 'SHA256SUMS.txt', 'licences/Phosphor-MIT.txt', 'features/NOTICE.txt') {
                $a = Join-Path $out $probe
                $b = Join-Path $root[0].FullName $probe
                if (-not (Test-Path -LiteralPath $b)) { throw "$probe did not survive a round trip through $tool ($banner)" }
                $ha = (Get-FileHash -LiteralPath $a -Algorithm SHA256).Hash
                $hb = (Get-FileHash -LiteralPath $b -Algorithm SHA256).Hash
                if ($ha -ne $hb) { throw "$probe came back out of the tar with different bytes through $tool ($banner)" }
            }
            $tested += $banner
        }
        Write-Host "        tar.gz read by check-archive.ps1 and by $($tested.Count) tar(s): $($tested -join '; ')" -ForegroundColor DarkGray
        $global:LASTEXITCODE = 0
    } finally {
        Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
    }
    # `Find-Tars`, not `Have tar`: a machine with GNU tar installed under Git
    # for Windows but not on PATH does have an independent reader, and used to
    # skip as though it had none.
} { (Find-Tars).Count -gt 0 }

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
    $x = '-----' + 'END '
    $e = '-----'
    # THE BODY IS WHAT IS MATCHED, NOT THE HEADER, and that is the difference
    # between this step and a string search.
    #
    # Matching the BEGIN delimiter alone failed on .github/workflows/release.yml,
    # which holds no key and never has: it assembles a PEM at signing time out of
    # the POLYLINKER_RELEASE_KEY secret, so it *writes* the header
    # (`echo '<BEGIN>'`, then `echo "$der_b64"`) and the bytes exist only inside a
    # runner. A header is a label. The material is the base64 between the two
    # delimiters, and this step is named for the material.
    #
    # So a match now needs BEGIN, then at least 40 characters of nothing but
    # base64 and whitespace, then END. A real leaked key -- including the one in
    # the near-miss above, an Ed25519 key written to a file as PEM -- is exactly
    # that shape. release.yml is not: the characters immediately before its END
    # delimiter are `echo '`, and a quote is not base64.
    #
    # `$armor` is why the run does not have to start at the delimiter: a PGP
    # block carries `Version:` headers and a blank line before its body, and a
    # pattern demanding base64 immediately would have quietly stopped matching
    # the one key format that has them. It cannot span a `-`, so it can never
    # reach past an END delimiter into the next block.
    #
    # WHAT THIS NO LONGER CATCHES, stated rather than left to be discovered: a
    # key pasted as a run of shell `echo` lines, where quoting interrupts the
    # body on every line. Nothing catches that without special-casing shell
    # syntax, and the shapes that matter -- a .pem, a .key, an id_ed25519, a
    # secret pasted into a config or a test fixture -- are all whole blocks.
    $armor = '[^-]{0,200}'
    $body = '[\sA-Za-z0-9+/=]{40,}'
    $markers = @(
        @{ Pattern = $b + '[A-Z ]*PRIVATE KEY' + $e + $armor + $body + $x + '[A-Z ]*PRIVATE KEY' + $e
           What = 'a PEM private key block' }
        @{ Pattern = $b + 'OPENSSH PRIVATE KEY' + $e + $armor + $body + $x + 'OPENSSH PRIVATE KEY' + $e
           What = 'an OpenSSH private key' }
        @{ Pattern = $b + 'PGP PRIVATE KEY BLOCK' + $e + $armor + $body + $x + 'PGP PRIVATE KEY BLOCK' + $e
           What = 'a PGP private key' }
    )
    # THE CONTROL, and it is not optional now that the pattern is a shape rather
    # than a string. A scanner that has stopped matching anything reports exactly
    # the same green as a clean tree, and the 2026-08-05 near-miss is the reason
    # that is not an acceptable way to fail. One synthetic block of each kind,
    # which must match; and release.yml's shape -- a delimiter a script writes,
    # with a shell variable where the body would be -- which must not.
    #
    # Assembled from the same split delimiters as the patterns, so this file
    # still contains no whole delimiter and is still scanned like any other.
    $fake = 'AAAA' * 16
    $controls = @(
        @{ Name = 'a PEM block'
           Text = $b + 'PRIVATE KEY' + $e + "`n" + $fake + "`n" + $x + 'PRIVATE KEY' + $e }
        @{ Name = 'an OpenSSH block'
           Text = $b + 'OPENSSH PRIVATE KEY' + $e + "`n" + $fake + "`n" + $x + 'OPENSSH PRIVATE KEY' + $e }
        # With the armor headers, which is the case `$armor` exists for.
        @{ Name = 'a PGP block'
           Text = $b + 'PGP PRIVATE KEY BLOCK' + $e + "`nVersion: GnuPG v1`n`n" + $fake + "`n" + $x + 'PGP PRIVATE KEY BLOCK' + $e }
    )
    foreach ($c in $controls) {
        if (-not @($markers | Where-Object { $c.Text -match $_.Pattern }).Count) {
            throw "$($c.Name) matched no marker, so this step is scanning for nothing"
        }
    }
    $benign = $b + 'PRIVATE KEY' + $e + "'`n" + '            echo "$der_b64"' + "`n            echo '" + $x + 'PRIVATE KEY' + $e
    foreach ($m in $markers) {
        if ($benign -match $m.Pattern) {
            throw "a delimiter with no body matched $($m.What); .github/workflows/release.yml would fail again"
        }
    }

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
    # So tools/installer/Polylinker.wxs contains no files. This step proves the
    # second half of that -- it regenerates the payload fragment and asserts the
    # component set is exactly the manifest minus the three deliberate
    # exclusions -- and, since 2026-08-14, another step proves the first.
    #
    # HALF THE ASSERTION THIS STEP'S NAME PROMISES IS NO LONGER IN THIS STEP.
    # IT MOVED, ON 2026-08-14, AND THIS IS WHERE IT WENT.
    #
    # The comparison below cannot, on its own, fail the way the name promises. It
    # compares the manifest against a fragment that build-msi.ps1 generated FROM
    # the manifest four lines earlier, using the same parser, and two lists
    # derived from one source agree by construction. It would pass just as
    # happily against a Polylinker.wxs stuffed with hand-written <File> elements,
    # which is the exact thing the name forbids. So the authoring file is checked
    # directly -- and that check, four lines of `Get-Content` plus regex, used to
    # sit right here, immediately below this comment.
    #
    # Sitting here put it behind THIS STEP'S PRECONDITION, which is Windows AND a
    # built `dist/`. Reading a committed .wxs for a <File> element needs neither:
    # no WiX, no msiexec, no dist/, no Windows. Worse, the precondition's own
    # comment told a reviewer the opposite in the one sentence a reviewer relies
    # on -- that the platform-neutral half of the authoring "is asserted by the
    # step below, which reads the committed .wxs, has no precondition at all, and
    # therefore runs on all three" -- while the step below had never contained
    # the string `<File` at all. That sentence was wrong on the day it was
    # written, and on Linux and macOS the claim was made and nothing checked it.
    #
    # The ban now lives in `Test-WxsPayloadAuthoring`, called by 'the MSI takes
    # no file type away from a program the reader already uses', which reads the
    # same file, strips the same comments, has no precondition at all and so runs
    # on three legs instead of one. This step's name is made true by that step.
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
} {
    # An MSI is a Windows Installer database and `dist/` here is the Windows
    # release it would be built from. Generating the payload fragment off
    # Windows would compare a WiX component set against a manifest listing
    # `polylinker.so` and a README-LINUX.txt -- two lists that still agree,
    # because both are derived from that manifest, about an installer that will
    # never be built there. A green from that tells a reader nothing.
    #
    # The half of the MSI's authoring that IS platform-neutral -- no <File>, no
    # <ComponentGroup>, no <Extension>, the pinned UpgradeCode -- is asserted by
    # the step below, which reads the committed .wxs, has no precondition at all,
    # and therefore runs on all three.
    #
    # THAT SENTENCE WAS FALSE UNTIL 2026-08-14 and is kept here, corrected,
    # rather than quietly rewritten. The `<File>` ban was not in the step below;
    # it was in the body of THIS step, behind this precondition, so the two legs
    # that cannot run this step were told the ban covered them and it did not.
    # `git log -S` puts the claim and the split in the same commit, so it was
    # wrong when authored rather than having drifted. The ban has been moved into
    # `Test-WxsPayloadAuthoring`, which the step below calls, and the sentence is
    # now true of every element it names.
    WindowsOnly { Test-Path "$repo/dist/SHA256SUMS.txt" }
}

# THE PAYLOAD AUTHORING RULE, AS A FUNCTION SO IT CAN BE RUN OVER PLANTED INPUT.
#
# `tools/installer/Polylinker.wxs` must contain no files. The MSI's component set
# is GENERATED from dist/SHA256SUMS.txt by tools/build-msi.ps1, because
# docs/RELEASING.md refused a compiled installer on the grounds that every one of
# them carries a second list of files that drifts from the real payload -- and
# that is not hypothetical here: the notices list in release.ps1 drifted twice in
# one week, on 2026-08-03 and 2026-08-04, and a licence text stopped shipping
# each time.
#
# It is a function because a ban is a check that cannot fail while the file it
# reads is clean, and this file has been clean since the day it was written --
# which is exactly how it went four audits without anybody noticing WHICH STEP
# held the ban. The caller runs it twice: once over the real .wxs, once over a
# copy with a <File> planted in it that must be refused, and refused for that
# reason.
function Test-WxsPayloadAuthoring {
    param([string]$Wxs)
    $found = @()
    if ($Wxs -match '<File\b') { $found += 'Polylinker.wxs contains a <File> element. The MSI payload must come from dist/SHA256SUMS.txt through build-msi.ps1, so that a licence text cannot stop shipping without the manifest saying so.' }
    if ($Wxs -match '<ComponentGroup\b(?!Ref)') { $found += 'Polylinker.wxs defines a <ComponentGroup>. It may only reference the generated one with <ComponentGroupRef Id="PayloadComponents" />.' }
    if ($Wxs -notmatch '<ComponentGroupRef\s+Id="PayloadComponents"') { $found += 'Polylinker.wxs does not reference the generated PayloadComponents group, so the MSI would ship no payload at all' }
    return $found
}

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

    # THE PAYLOAD AUTHORING BAN, WHICH RUNS HERE BECAUSE HERE IT RUNS ON ALL
    # THREE. It lived until 2026-08-14 in 'the MSI is generated from the manifest
    # and not from a second file list', whose precondition is
    # `WindowsOnly { Test-Path dist/SHA256SUMS.txt }`, while that step's own
    # comment told the reader this step asserted it. This step had never
    # contained the string `<File`. Nothing else in `tools/` or `.github/` bans
    # one either -- 'the installer contacts nothing' is portable and scans the
    # same directory, but only for network and scheduling tokens. So the ban is
    # here now, where the file it reads is committed and the step it sits in has
    # no precondition at all.
    $problems += @(Test-WxsPayloadAuthoring $wxs)

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

    # NEGATIVE CONTROL FOR THE BAN, on a doctored copy of the real file rather
    # than on a hand-written string, so that the thing being refused is the thing
    # a careless edit would actually produce. One <File> is planted immediately
    # before the closing tag; the ban must report that and nothing else.
    $doctored = $wxs -replace '(?s)(</Package>)', '<Component Id="C_Sneaky" Directory="APPLICATIONFOLDER"><File Id="SneakyFile" Name="polylinker.exe" Source="!(bindpath.payload)polylinker.exe" /></Component>$1'
    if ($doctored -eq $wxs) { throw 'the probe planted nothing: Polylinker.wxs no longer ends in </Package>, so the payload authoring ban below is unproven' }
    $planted = @(Test-WxsPayloadAuthoring $doctored)
    if ($planted.Count -ne 1) { throw "the payload authoring ban found $($planted.Count) problem(s) in a copy of Polylinker.wxs carrying one planted <File> element, not 1, so it is not doing what its name says" }
    if ($planted[0] -notmatch 'contains a <File> element') { throw "the payload authoring ban refused the doctored file for the wrong reason: $($planted[0])" }

    if ($problems) { throw ($problems -join "`n        ") }
    Write-Host "        no <File>, no <Extension>, no default-handler theft, $owp additive associations, UpgradeCode pinned" -ForegroundColor DarkGray
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
    # SDK cannot build an MSI to test and the other 75 steps are still worth
    # running there. (It said 62 until 2026-08-10, 71 until 2026-08-13, 72 and then 74
    # until 2026-08-14, each of which was the count before the gate grew past it;
    # the number is this file's step total minus this one step, so it moves every
    # time a step is added. Step total today: 76.)
    #
    # FIVE PLACES OUTSIDE THIS FILE STILL SAY SEVENTY-TWO OR SEVENTY-THREE, and
    # they are stale as of 2026-08-14, when 'the browser prototype: template and
    # builder agree, and the page is built and driven where it can be' and 'the
    # archived licence evidence still matches the manifest it was fetched under'
    # were added. They are named here rather than left for a reader to trip over:
    # `docs/RELEASING.md:425` ("seventy-two"), `tools/build-msi.ps1:289`
    # ("seventy-two"), `tools/verify.ps1:12` ("at 73 steps"), `README.md:13` and
    # `CONTRIBUTING.md:103`, plus `.github/workflows/ci.yml:16` and :148. Nothing
    # in the gate reads any of them, so none of them can go red; they are prose
    # about this file and they have to be edited by hand when it grows. The two
    # quotations of a past reconciler run -- `tools/ci.ps1:148` and
    # `tools/reconcile-ledgers.ps1:195`, both "73 steps each" -- are records of
    # run 31359657821 and stay true of that run.
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
    # `msiexec` is Windows. `wix` is a .NET tool that will install anywhere, and
    # would build a .msi anywhere, but this step's oracle is INSTALLING it and
    # then reading the disk and the registry back -- there is no Linux msiexec
    # to install with and nothing to read.
    #
    # `wix` missing on Windows stays $false, not a reason: on a runner that is a
    # failure (ci.yml installs it, and a `dotnet tool install` that silently
    # stopped working is exactly the "installs its oracles and then stops" case
    # -ExpectedSkips exists for), and on a workstation with no .NET SDK it is a
    # quiet skip because no list is being checked there.
    WindowsOnly {
        (Test-Path "$repo/dist/SHA256SUMS.txt") -and
        ($null -ne (Get-Command wix -ErrorAction SilentlyContinue))
    }
}

Step 'the release workflow parses and covers four platforms' {
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
    # THE TWO LISTS ARE PINNED HERE AND NOT DERIVED FROM ANYTHING, which is the
    # opposite of what the step below this one does and is deliberate. That step
    # asks whether the updater's table and this matrix AGREE; if this list were
    # derived from either of them, adding a platform to both would satisfy both
    # checks and nobody would have had to look at what it costs. A platform is
    # an artifact users download for years, a runner is a machine somebody has
    # to pay for or GitHub has to keep free, and the MSI is built inside the
    # windows legs of this job rather than in a leg of its own. So a new one is
    # a line in this file, in a reviewed diff, and that is the whole point of
    # writing them out.
    #
    # windows-11-arm is GitHub's native ARM64 Windows image, free for public
    # repositories. It is here because ARM64 is BUILT AND TESTED THERE rather
    # than cross-compiled: nothing about an aarch64 artifact is claimed by a
    # machine that cannot run one.
    runners = sorted(e.get("os") for e in include)
    want = ["macos-latest", "ubuntu-latest", "windows-11-arm", "windows-latest"]
    if runners != want:
        problems.append(f"the matrix runs on {runners}, not {want}")
    labels = sorted(e.get("label") for e in include)
    if labels != ["linux-x64", "macos-universal", "windows-arm64", "windows-x64"]:
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

# THE UPDATER'S PLATFORM TABLE AND THE RELEASE MATRIX, WHICH NOTHING CONNECTED.
#
# `crates/pl-update/src/flow.rs` holds `PLATFORM_ARTIFACT`: a `#[cfg]` cascade
# naming, for each platform, the release file `pl update` downloads in order to
# update itself. `.github/workflows/release.yml` holds the build matrix that
# PRODUCES those files. Two lists of the same platforms, written in two
# languages, and until this step nothing compared them -- which is the shape
# this file has already shipped nine fixes for: a list of things, and a second
# list that has to agree with it because somebody remembered.
#
# THE TWO WAYS THEY DRIFT, both silent until a user hits one:
#
#   * an arm the matrix does not build. `artifact_file_name` in flow.rs builds
#     `polylinker-<version>-<label>.<extension>` and `pl update` asks the
#     release page for exactly that; there is no such asset, so the download
#     404s -- on a release that is otherwise perfectly good, from a binary that
#     has already told the user an update is waiting.
#   * a label the table does not carry. That platform installs and can never
#     update itself: `artifact_file_name` returns `None`, `pl update` declines,
#     and nothing anywhere reports that a platform was left out of the table.
#
# THERE IS A THIRD LIST, AND THIS STEP IS THE ONLY THING THAT CAN READ IT
# AGAINST THE WORKFLOW. `published_artifact_names` in that crate's test module
# is the crate's copy of every file a release attaches, and the crate's own
# tests hold the cascade against it -- but it is a COPY. Its own doc comment
# says so twice, and so does the cascade's: "two copies agreeing is not evidence
# about the release page", "`tools/ci.ps1` is the only place that reads both".
# Those sentences are claims about THIS FILE, made in a file that cannot check
# them. Either this step holds that list against the workflow or that prose is
# describing something nobody wrote.
#
# WHY THIS IS WORTH A STEP NOW AND WAS NOT BEFORE. Until windows-arm64 the
# cascade was three `Some` arms and one `None` fallback, and the fallback is
# what made the first failure impossible rather than merely unlikely: a platform
# the workflow did not build had no arm, so it named no file, so it could not
# 404. It declined, which is the honest answer and the safe one. A FOURTH ARM
# SPENDS THAT SAFETY. From the moment a cfg arm exists for a platform, the
# binary built there WILL construct a URL, so the arm and the published artifact
# have to arrive in the same commit -- and this is the thing that says so when
# they do not.
#
# No interpreter, so this runs on every leg of the gate rather than only where
# PyYAML is installed, unlike the step above that reads the same workflow
# through `yaml.safe_load`. Both constructs are PARSED and not grepped for: a
# `Select-String` for 'windows-x64' finds that string in both files today and
# would go on finding it for as long as one platform still agreed, which is a
# check that passes while the thing it is named after is broken.
#
# WHAT IT DOES NOT PROVE. These are holes, not hedges, and the first is the one
# a reader is most likely to assume away:
#
#   * that the artifact exists on a release page. Both subjects are files in
#     this repository. A tag whose Windows leg died still has its matrix entry
#     here, and this step will call the pair consistent.
#   * that the `not(any(...))` list in the fallback matches the positive arms.
#     Get that wrong and the affected platform declares PLATFORM_ARTIFACT twice
#     or not at all -- a compile error, but one that appears only when somebody
#     compiles for that platform. That one is held by
#     `the_platform_cascade_and_its_fallback_stay_mutually_exclusive` in the
#     crate itself, which reads the same text; nothing here duplicates it.
#   * that any sentence about platforms anywhere is true. This compares two
#     machine-readable constructs to each other. The doc comment above the
#     cascade, README.md, docs/RELEASING.md and tools/release-notes.md are prose
#     and are read by nothing in this step.
#   * that the extension is the RIGHT one for the platform. It checks that the
#     workflow names the file the updater will ask for. Whether a Windows reader
#     should be handed an `.msi` rather than the `.zip` is a decision, recorded
#     in flow.rs's own doc comment and held by
#     `crates/pl-update/tests/handoff.rs`.

# `PLATFORM_ARTIFACT`, read out of the Rust rather than matched inside it.
#
# The shape is one `#[cfg(...)]` attribute -- which may span four lines, as the
# macOS arm does -- immediately above one
# `const PLATFORM_ARTIFACT: Option<(&str, &str)> = Some(("label", "ext"));` or
# `= None;`. Anything else in that position THROWS rather than being skipped: a
# parser that quietly reads three arms out of a four-arm table reports the
# fourth as missing from the workflow, which sends the reader to the wrong file
# with a message that sounds authoritative.
#
# Only a `//` that OPENS a line is treated as a comment. `//` also occurs inside
# this file's string literals -- `RELEASE_BASE_URL` is an https URL -- and a
# parser that strips from there is reading its own invention.
#
# IT STOPS AT `#[cfg(test)]`, and that is not tidiness. The crate's test module
# carries raw-string PROBES that spell out complete, well-formed arms -- they
# are what its own cascade reader is proved against -- so a scan of the whole
# file reads eight arms instead of four, three of them naming windows-x64.
# Measured, not reasoned: without this the first run over the real file threw
# "flow.rs:1101 declares PLATFORM_ARTIFACT with no #[cfg] attribute above it",
# pointing at a fixture inside a test. The crate's own reader draws the line in
# the same place, at the same marker, for the same reason.
function Get-UpdaterPlatformArtifacts {
    param([string]$Path)
    $lines = @(Get-Content -LiteralPath $Path)
    $leaf = Split-Path -Leaf $Path
    $found = [System.Collections.Generic.List[object]]::new()
    $cfg = ''
    $cfgEnd = -2      # not -1: -1 would be "ends on the line above line 0"
    $inCfg = $false
    $open = 0
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        if ($line -match '^\s*#\[cfg\(test\)\]') { break }
        if ($line -match '^\s*//') { continue }
        if (-not $inCfg -and $line -match '^\s*#\[\s*cfg\b') {
            $inCfg = $true
            $cfg = ''
            $open = 0
        }
        if ($inCfg) {
            $cfg += ' ' + $line.Trim()
            $open += ([regex]::Matches($line, '\(')).Count - ([regex]::Matches($line, '\)')).Count
            if ($open -le 0) { $inCfg = $false; $cfgEnd = $i }
            continue
        }
        if ($line -notmatch '\bconst\s+PLATFORM_ARTIFACT\s*:') { continue }
        if ($cfgEnd -ne $i - 1) {
            throw ("${leaf}:$($i + 1) declares PLATFORM_ARTIFACT with no #[cfg] attribute on the line above it. " +
                   'Every arm of that cascade is chosen by its own cfg; one without is either dead code or ' +
                   'ambiguous, and this step will not guess which.')
        }
        if ($line -notmatch '=\s*(.+?);\s*$') {
            throw ("${leaf}:$($i + 1) declares PLATFORM_ARTIFACT and this step cannot read the value off the " +
                   'line. It expects the whole initialiser on one line, which is where rustfmt leaves it.')
        }
        $value = $Matches[1].Trim()
        if ($value -eq 'None') {
            $found.Add([pscustomobject]@{
                Fallback = $true; Label = $null; Extension = $null; Cfg = $cfg.Trim(); Line = $i + 1 })
        } elseif ($value -match '^Some\(\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*\)\)$') {
            $found.Add([pscustomobject]@{
                Fallback = $false; Label = $Matches[1]; Extension = $Matches[2]; Cfg = $cfg.Trim(); Line = $i + 1 })
        } else {
            throw ("${leaf}:$($i + 1) sets PLATFORM_ARTIFACT to `"$value`", which is neither None nor " +
                   'Some(("<label>", "<extension>")). The label in that tuple is half of the file name every ' +
                   'copy of pl asks a release page for, so it has to stay something a reader -- and this step -- ' +
                   'can see at a glance.')
        }
    }
    $found.ToArray()   # plainly, for the reason Get-TestNames gives
}

# The build matrix's `include:` list, as (os, label) pairs.
#
# Indentation-scoped rather than a regex over the whole file, because `label`
# appears in this workflow outside the matrix as well -- `name: ${{ matrix.label
# }}` and the artifact names -- and a pattern loose enough to find the entries
# would find those too and report platforms that do not exist.
#
# A LINE IN THE BLOCK THAT THIS CANNOT READ IS A FAILURE, not something to step
# over. YAML has other spellings for the same list (a bare `-` with the keys
# beneath it, or a flow mapping `- {os: x, label: y}`), and a parser that
# silently ignored one would report the platform as missing from the updater's
# table. Throwing names the line and asks for either the shape below or a
# parser that understands the new one.
function Get-ReleaseMatrixEntries {
    param([string]$Path)
    $leaf = Split-Path -Leaf $Path
    # Comments first, by the rule the step below this one uses: a '#' that opens
    # a line or follows whitespace. Five lines of the include block are comment,
    # and one of them names a platform.
    $lines = @(Get-Content -LiteralPath $Path | ForEach-Object { $_ -replace '(^|\s)#.*$', '' })

    if (@($lines | Where-Object { $_ -match '^\s*matrix:\s*$' }).Count -gt 1) {
        throw ("$leaf declares more than one build matrix. This step reads the first, which would then be the " +
               'wrong one; teach it which job it is meant to be reading before adding a second.')
    }
    $matrixAt = -1
    $matrixIndent = 0
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^(\s*)matrix:\s*$') { $matrixAt = $i; $matrixIndent = $Matches[1].Length; break }
    }
    if ($matrixAt -lt 0) { throw "$leaf declares no build matrix at all, so there is nothing here to compare the updater's table against" }

    $includeAt = -1
    $includeIndent = 0
    for ($i = $matrixAt + 1; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^\s*$') { continue }
        $ind = $lines[$i].Length - $lines[$i].TrimStart().Length
        if ($ind -le $matrixIndent) { break }
        if ($lines[$i] -match '^\s*include:\s*$') { $includeAt = $i; $includeIndent = $ind; break }
    }
    if ($includeAt -lt 0) { throw "${leaf}:$($matrixAt + 1) is a build matrix with no include: list, so this step can read no platform labels out of it" }

    $entries = [System.Collections.Generic.List[object]]::new()
    $current = $null
    for ($i = $includeAt + 1; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        if ($line -match '^\s*$') { continue }
        $ind = $line.Length - $line.TrimStart().Length
        if ($ind -le $includeIndent) { break }
        if ($line -match '^\s*-\s+([A-Za-z_][\w.-]*)\s*:\s*(.+?)\s*$') {
            $current = [pscustomobject]@{ Os = $null; Label = $null; Line = $i + 1 }
            $entries.Add($current)
        } elseif ($line -match '^\s*([A-Za-z_][\w.-]*)\s*:\s*(.+?)\s*$') {
            if ($null -eq $current) {
                throw "${leaf}:$($i + 1) sets a key inside the include list before any list item has started: $($line.Trim())"
            }
        } else {
            throw ("${leaf}:$($i + 1) is inside the build matrix's include list and this step cannot read it: " +
                   "$($line.Trim()). Every entry here has to be a `"- os: <runner>`" line followed by " +
                   '"label: <platform>", or this parser has to learn the new spelling -- silently skipping the ' +
                   "line would report that platform as one the updater's table has invented.")
        }
        $key = $Matches[1]
        $val = $Matches[2].Trim()
        if ($val -match '^"(.*)"$') { $val = $Matches[1] } elseif ($val -match "^'(.*)'$") { $val = $Matches[1] }
        if ($val -match '\$\{\{') {
            throw ("${leaf}:$($i + 1) gives $key as a workflow expression. This step compares literal platform " +
                   'names and cannot evaluate one, and calling an unevaluated expression a platform name would ' +
                   'be worse than saying so.')
        }
        # Only these two keys are this step's business; a matrix is free to
        # carry others and several workflows do.
        if ($key -eq 'os') { $current.Os = $val }
        elseif ($key -eq 'label') { $current.Label = $val }
    }
    foreach ($e in $entries) {
        if (-not $e.Os -or -not $e.Label) {
            throw ("${leaf}:$($e.Line) is a build matrix entry without both an os: and a label:. The label is the " +
                   'platform half of every artifact name that entry produces; an entry missing one builds ' +
                   'something this step cannot name.')
        }
    }
    $entries.ToArray()
}

# `published_artifact_names`, the crate's copy of what a release attaches.
#
# It lives in the test module, which is why the reader above stops before it and
# this one starts by finding it. Its doc comment calls itself "half of a check"
# and names this file as the other half; if it is renamed or its shape changes,
# this THROWS rather than returning an empty list, because an empty list would
# agree with any workflow at all.
function Get-PublishedArtifactFixture {
    param([string]$Path)
    $lines = @(Get-Content -LiteralPath $Path)
    $leaf = Split-Path -Leaf $Path
    $at = -1
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^(\s*)fn\s+published_artifact_names\b') { $at = $i; $indent = $Matches[1].Length; break }
    }
    if ($at -lt 0) {
        throw ("$leaf declares no fn published_artifact_names. That function is the crate's copy of every file a " +
               'release attaches, its own doc comment says holding it against .github/workflows/release.yml ' +
               'belongs to this gate, and this step is that sentence. If it has been renamed, rename it here; if ' +
               'it is gone, the sentence has to go with it.')
    }
    $names = @()
    $closed = $false
    for ($i = $at + 1; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^\s{0,}\}' -and ($lines[$i].Length - $lines[$i].TrimStart().Length) -eq $indent) { $closed = $true; break }
        foreach ($m in [regex]::Matches($lines[$i], 'polylinker-\{\w+\}-([\w.-]+?)\.(zip|tar\.gz|msi)(?![\w.])')) {
            $names += ($m.Groups[1].Value + '.' + $m.Groups[2].Value)
        }
    }
    if (-not $closed) { throw "${leaf}:$($at + 1) opens fn published_artifact_names and this step never found its closing brace" }
    $names
}

# Every release artifact `.github/workflows/release.yml` names IN FULL.
#
# `polylinker-$version-windows-x64.msi` counts; `artifacts/polylinker-*.msi`
# does not, and that asymmetry is the point. The globs at the end of that
# workflow attach whatever happens to be on disk, so a platform whose build
# quietly produced nothing is published as a release missing a file. What makes
# "every platform publishes together or none do" true is the publish job's
# by-name check, and only a name can be compared with a name.
function Get-WorkflowArtifactNames {
    param([string]$Text)
    @([regex]::Matches($Text, 'polylinker-\$\{?\w+\}?-([\w.-]+?)\.(zip|tar\.gz|msi)(?![\w.])') |
      ForEach-Object { $_.Groups[1].Value + '.' + $_.Groups[2].Value } | Sort-Object -Unique)
}

# The comparison itself, in a function so that the planted cases in the step
# below drive THIS code and not a second copy of the rules written to agree with
# it. Returns one string per problem; an empty array is agreement.
function Compare-PlatformCoverage {
    param($Arms, $Entries, [string[]]$Fixture, [string[]]$WorkflowNames)
    $problems = @()
    $armLabels = @($Arms | ForEach-Object { $_.Label })
    $entryLabels = @($Entries | ForEach-Object { $_.Label })

    foreach ($dup in @($armLabels | Group-Object | Where-Object { $_.Count -gt 1 })) {
        $problems += ("the updater's table has $($dup.Count) arms naming the platform '$($dup.Name)'. Two cfgs " +
                      'mapping to one artifact name is an arm that was copied and whose label was not changed, ' +
                      'and the platform it was copied FOR now has no entry of its own.')
    }
    foreach ($dup in @($entryLabels | Group-Object | Where-Object { $_.Count -gt 1 })) {
        $problems += ("the release matrix has $($dup.Count) entries labelled '$($dup.Name)'. Both produce the same " +
                      'artifact file name, so one of the two builds is thrown away by whichever upload runs last.')
    }

    foreach ($a in $Arms) {
        if ($entryLabels -notcontains $a.Label) {
            $problems += ("crates/pl-update/src/flow.rs:$($a.Line) makes '$($a.Label)' an updatable platform and " +
                          ".github/workflows/release.yml builds no such label. Every copy of pl on that platform " +
                          "will ask a release page for polylinker-<version>-$($a.Label).$($a.Extension) and be " +
                          'answered 404 -- which is worse than the refusal the None fallback used to give, because ' +
                          'by then the binary has already told the user an update is available.')
        }
    }
    foreach ($e in $Entries) {
        if ($armLabels -notcontains $e.Label) {
            $problems += (".github/workflows/release.yml:$($e.Line) builds '$($e.Label)' and the cascade in " +
                          'crates/pl-update/src/flow.rs has no arm for it, so everyone running that build is on a ' +
                          'copy that can never update itself. `pl update` there refuses with PlatformUnsupported ' +
                          'and no line anywhere says the platform was left out of the table.')
        }
    }

    # THE FILE NAME, not merely the label. A matrix entry says the platform is
    # BUILT; the publish job's by-name checks are what say the file is
    # PUBLISHED, and those two came apart once already -- `fail-fast: false`
    # means one leg can die while the others succeed, which is why that job
    # lists its artifacts by name rather than globbing them.
    foreach ($a in $Arms) {
        $mine = "$($a.Label).$($a.Extension)"
        if ($WorkflowNames -notcontains $mine) {
            $problems += ("nothing in .github/workflows/release.yml names polylinker-<version>-$mine, which is the " +
                          'exact file artifact_file_name() constructs on that platform. The publish job checks its ' +
                          'artifacts BY NAME before the release is created -- that is what makes "all platforms ' +
                          'publish together or none do" true -- and a name that is not on that list is a file the ' +
                          'release can be published without.')
        }
    }

    # AND THE CRATE'S COPY OF THE PUBLISHED LIST, BOTH WAYS. This is the half
    # `published_artifact_names` says it cannot do: it is a fixture, the crate's
    # tests hold the cascade against it, and two copies agreeing is not evidence
    # about a release page. Both directions, because each is a different defect.
    # A fixture entry the workflow does not publish means the crate's suite is
    # passing against a release that does not exist -- the ARM64 tests would go
    # green on windows-arm64.msi while nothing built one. A published file the
    # fixture does not name means the suite has never seen an artifact users
    # can download.
    foreach ($f in $Fixture) {
        if ($WorkflowNames -notcontains $f) {
            $problems += ("published_artifact_names() in crates/pl-update/src/flow.rs claims a release attaches " +
                          "polylinker-<version>-$f, and .github/workflows/release.yml names no such file. The " +
                          "crate's tests build their whole manifest fixture out of that list, so they are green " +
                          'against a release page that does not have it.')
        }
    }
    foreach ($n in $WorkflowNames) {
        if ($Fixture -notcontains $n) {
            $problems += (".github/workflows/release.yml publishes polylinker-<version>-$n and " +
                          'published_artifact_names() in crates/pl-update/src/flow.rs does not name it, so no test ' +
                          'in that crate has ever seen a manifest containing a file users can download.')
        }
    }

    # The three spellings of one platform, which must agree: the cfg the arm is
    # compiled in under, the label the artifact is named with, and the runner
    # the matrix builds it on.
    $runnerOs = @{ windows = 'windows'; ubuntu = 'linux'; macos = 'macos' }
    foreach ($a in $Arms) {
        $os = @([regex]::Matches($a.Cfg, 'target_os\s*=\s*"([^"]+)"') |
                ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
        if ($os.Count -ne 1) {
            $problems += ("crates/pl-update/src/flow.rs:$($a.Line) is selected by a cfg naming $($os.Count) " +
                          "operating system(s) ($($os -join ', ')), so nothing here can say which platform " +
                          "'$($a.Label)' is the artifact for.")
            continue
        }
        $want = ($a.Label -split '-')[0]
        if ($os[0] -ne $want) {
            $problems += ("crates/pl-update/src/flow.rs:$($a.Line) is compiled in under target_os = " +
                          "`"$($os[0])`" and names its artifact '$($a.Label)', whose platform word is '$want'. " +
                          'One of the two was copied from the arm above it and not finished.')
        }
    }
    foreach ($e in $Entries) {
        $prefix = ($e.Os -split '-')[0].ToLowerInvariant()
        if (-not $runnerOs.ContainsKey($prefix)) {
            $problems += (".github/workflows/release.yml:$($e.Line) builds '$($e.Label)' on the runner " +
                          "'$($e.Os)', which this step does not recognise as Windows, Linux or macOS. Teach it " +
                          'the new runner family rather than leaving that label compared against nothing.')
            continue
        }
        $want = ($e.Label -split '-')[0]
        if ($runnerOs[$prefix] -ne $want) {
            $problems += (".github/workflows/release.yml:$($e.Line) builds the artifact labelled '$($e.Label)' on " +
                          "'$($e.Os)', which is $($runnerOs[$prefix]). That label is the platform half of the file " +
                          "name pl update asks for, so a $want user would be handed a $($runnerOs[$prefix]) build.")
        }
    }
    return $problems
}

Step 'the updater''s platform table and the release workflow agree, in both directions' {
    $flowPath = Join-Path $repo 'crates/pl-update/src/flow.rs'
    $wfPath = Join-Path $repo '.github/workflows/release.yml'
    foreach ($p in $flowPath, $wfPath) {
        if (-not (Test-Path -LiteralPath $p)) { throw "$p is missing, and it is one of the two files this step exists to compare" }
    }

    $table = @(Get-UpdaterPlatformArtifacts $flowPath)
    $arms = @($table | Where-Object { -not $_.Fallback })
    $fallbacks = @($table | Where-Object { $_.Fallback })
    $fixture = @(Get-PublishedArtifactFixture $flowPath)
    $entries = @(Get-ReleaseMatrixEntries $wfPath)
    $wfNames = @(Get-WorkflowArtifactNames ((Get-Content -LiteralPath $wfPath) -join "`n"))

    # FLOORS, because a parser that has stopped matching enumerates nothing and
    # then agrees with everything: two empty sets are equal, and three of the
    # four comparisons below are set comparisons. Three platforms and four files
    # are what this project shipped before windows-arm64, so lowering any of
    # these is a deliberate line in a diff.
    if ($arms.Count -lt 3) {
        throw ("only $($arms.Count) platform arm(s) parsed out of crates/pl-update/src/flow.rs; the cascade has " +
               'at least three, so this parser is broken and the comparison below would prove nothing')
    }
    if ($entries.Count -lt 3) {
        throw ("only $($entries.Count) matrix entr(ies) parsed out of .github/workflows/release.yml; the release " +
               'builds at least three platforms, so this parser is broken and the comparison below would prove nothing')
    }
    if ($fixture.Count -lt 4) {
        throw ("only $($fixture.Count) file name(s) read out of published_artifact_names(); it has named at least " +
               'four since the MSI shipped, so this parser is broken and would agree with any workflow at all')
    }
    if ($wfNames.Count -lt 4) {
        throw ("only $($wfNames.Count) artifact name(s) written out in full in .github/workflows/release.yml. Its " +
               'publish job names every file it will not publish a release without, and if that has become a loop ' +
               'over a list or a glob then the comparison below is over an empty set and this step proves nothing. ' +
               'Teach this step the new shape rather than letting it pass.')
    }
    # AND THE FALLBACK MUST SURVIVE. It is what makes `pl update` decline on a
    # platform with no build instead of guessing at the closest artifact, and
    # without it that platform has no PLATFORM_ARTIFACT at all -- which is a
    # compile error nobody sees until somebody builds there.
    if ($fallbacks.Count -ne 1) {
        throw ("the cascade in crates/pl-update/src/flow.rs has $($fallbacks.Count) None arm(s) and needs exactly " +
               'one. That arm is the whole of the refusal: it is why a platform the release workflow does not ' +
               'build is told there is no update for it rather than being handed the nearest file.')
    }

    $problems = @(Compare-PlatformCoverage -Arms $arms -Entries $entries -Fixture $fixture -WorkflowNames $wfNames)

    # THE CONTROL, on planted files outside this repository, because a
    # comparator that has stopped comparing reports the same clean as a tree
    # that agrees -- and the real files are green only when they agree, which is
    # exactly the state in which a broken checker is indistinguishable from a
    # working one. Six cases: each rule that can fire, fired once, plus a
    # consistent set that must produce NOTHING. That last one is not padding: a
    # checker that objects to everything is deleted as noise rather than fixed,
    # and this file has watched that happen.
    $probeDir = Join-Path $tmp "pl-platform-probe-$PID"
    if (Test-Path -LiteralPath $probeDir) { Remove-Item -LiteralPath $probeDir -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $probeDir | Out-Null
    try {
        # Four shapes the readers have to get right, all of which a draft of
        # them got wrong: a four-line cfg, a doc comment naming a real label,
        # the `#[cfg(test)]` boundary, and -- past that boundary -- a raw-string
        # probe that IS a well-formed arm. The real flow.rs carries five of
        # those probes, and reading them cost the first run of this step.
        $flowProbe = Join-Path $probeDir 'flow.rs'
        Set-Content -LiteralPath $flowProbe -Value @(
            '/// Prose naming linux-riscv64, which is not an arm and must not be read as one.',
            '#[cfg(all(target_os = "linux", target_arch = "x86_64"))]',
            'const PLATFORM_ARTIFACT: Option<(&str, &str)> = Some(("linux-x64", "tar.gz"));',
            '#[cfg(all(',
            '    target_os = "linux",',
            '    any(target_arch = "riscv64", target_arch = "loongarch64")',
            '))]',
            'const PLATFORM_ARTIFACT: Option<(&str, &str)> = Some(("linux-riscv64", "tar.gz"));',
            '#[cfg(not(any(all(target_os = "linux", target_arch = "x86_64"))))]',
            'const PLATFORM_ARTIFACT: Option<(&str, &str)> = None;',
            '#[cfg(test)]',
            'mod tests {',
            '    fn published_artifact_names(version: &Version) -> Vec<String> {',
            '        vec![',
            '            format!("polylinker-{version}-linux-x64.tar.gz"),',
            '            format!("polylinker-{version}-linux-riscv64.tar.gz"),',
            '        ]',
            '    }',
            '',
            '    const PROBE: &str = "const PLATFORM_ARTIFACT: Option<(&str, &str)> = Some((0, 0));";',
            '}'
        )
        $wfProbe = Join-Path $probeDir 'release.yml'
        Set-Content -LiteralPath $wfProbe -Value @(
            'jobs:',
            '  build:',
            '    strategy:',
            '      matrix:',
            '        include:',
            '          - os: ubuntu-latest',
            '            label: linux-x64',
            '          - os: ubuntu-latest',
            '            label: linux-riscv64',
            '    steps:',
            '      - run: echo this is not a matrix entry'
        )
        $probeTable = @(Get-UpdaterPlatformArtifacts $flowProbe)
        $probeArms = @($probeTable | Where-Object { -not $_.Fallback })
        $probeFixture = @(Get-PublishedArtifactFixture $flowProbe)
        $probeEntries = @(Get-ReleaseMatrixEntries $wfProbe)
        if ($probeArms.Count -ne 2) {
            throw ("the table reader found $($probeArms.Count) of the 2 planted arms. One carries a cfg spanning " +
                   'four lines and there is a third, well-formed one past the #[cfg(test)] marker that it must ' +
                   'not have counted.')
        }
        if (@($probeTable | Where-Object { $_.Fallback }).Count -ne 1) {
            throw 'the table reader did not see the planted None fallback, so its absence would not be noticed either'
        }
        if (($probeFixture -join ',') -ne 'linux-x64.tar.gz,linux-riscv64.tar.gz') {
            throw "the fixture reader read published_artifact_names() as [$($probeFixture -join ', ')]"
        }
        if ($probeEntries.Count -ne 2 -or $probeEntries[1].Label -ne 'linux-riscv64') {
            throw ("the matrix reader found $($probeEntries.Count) planted entries, the second labelled " +
                   "'$($probeEntries[1].Label)'")
        }
        $both = @('linux-x64.tar.gz', 'linux-riscv64.tar.gz')

        $agree = @(Compare-PlatformCoverage -Arms $probeArms -Entries $probeEntries -Fixture $probeFixture -WorkflowNames $both)
        if ($agree.Count -ne 0) {
            throw ("a consistent planted set produced $($agree.Count) problem(s), so this comparator objects to " +
                   "everything and its silence on the real files means nothing:`n        " + ($agree -join "`n        "))
        }
        $tableOnly = @(Compare-PlatformCoverage -Arms $probeArms -Entries @($probeEntries[0]) -Fixture $probeFixture -WorkflowNames $both)
        if ($tableOnly.Count -ne 1 -or $tableOnly[0] -notmatch 'linux-riscv64' -or $tableOnly[0] -notmatch '404') {
            throw ("an arm the matrix does not build was not reported as the 404 it is. Got " +
                   "$($tableOnly.Count) problem(s):`n        " + ($tableOnly -join "`n        "))
        }
        $matrixOnly = @(Compare-PlatformCoverage -Arms @($probeArms[0]) -Entries $probeEntries -Fixture $probeFixture -WorkflowNames $both)
        if ($matrixOnly.Count -ne 1 -or $matrixOnly[0] -notmatch 'linux-riscv64' -or $matrixOnly[0] -notmatch 'never update itself') {
            throw ("a matrix label with no table arm was not reported as a platform that cannot update itself. Got " +
                   "$($matrixOnly.Count) problem(s):`n        " + ($matrixOnly -join "`n        "))
        }
        $unnamed = @(Compare-PlatformCoverage -Arms $probeArms -Entries $probeEntries -Fixture $probeFixture `
                        -WorkflowNames @('linux-x64.tar.gz'))
        if ($unnamed.Count -ne 2 -or @($unnamed | Where-Object { $_ -match 'artifact_file_name' }).Count -ne 1 -or
            @($unnamed | Where-Object { $_ -match 'published_artifact_names' }).Count -ne 1) {
            throw ("a file the workflow never names in full was not reported from both the cascade and the " +
                   "fixture. Got $($unnamed.Count) problem(s):`n        " + ($unnamed -join "`n        "))
        }
        $stray = @(Compare-PlatformCoverage -Arms $probeArms -Entries $probeEntries -Fixture $probeFixture `
                      -WorkflowNames ($both + 'linux-mips.tar.gz'))
        if ($stray.Count -ne 1 -or $stray[0] -notmatch 'linux-mips') {
            throw ("a file the workflow publishes and the crate's fixture has never heard of was not reported. Got " +
                   "$($stray.Count) problem(s):`n        " + ($stray -join "`n        "))
        }
        $wrongRunner = @(Compare-PlatformCoverage -Arms @($probeArms[0]) `
                            -Entries @([pscustomobject]@{ Os = 'windows-latest'; Label = 'linux-x64'; Line = 7 }) `
                            -Fixture @('linux-x64.tar.gz') -WorkflowNames @('linux-x64.tar.gz'))
        if ($wrongRunner.Count -ne 1 -or $wrongRunner[0] -notmatch 'which is windows') {
            throw ("a linux artifact built on a Windows runner was not reported. Got " +
                   "$($wrongRunner.Count) problem(s):`n        " + ($wrongRunner -join "`n        "))
        }
    } finally {
        Remove-Item -LiteralPath $probeDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    if ($problems) { throw ($problems -join "`n        ") }
    # THE WHOLE FORMAT STRING IS PARENTHESISED, for the reason written out at
    # length above 'the cross-platform scripts touch no environment variable
    # unguarded': `-f` binds tighter than `+`, so without these brackets only
    # the last literal is formatted and the line prints "{0} platform(s): {1}".
    # Which is exactly what the first run of this step printed.
    Write-Host (("        {0} platform(s): {1}. Each built by the matrix, and all {2} file(s) the crate says a " +
                 "release attaches are named in full by the workflow. Comparator verified on six planted cases") -f
                $arms.Count, (($arms | ForEach-Object { "$($_.Label) -> .$($_.Extension)" }) -join ', '),
                $fixture.Count) -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
}

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

# EVERY ENVIRONMENT VARIABLE THE THREE CROSS-PLATFORM SCRIPTS READ, AGAINST AN
# ALLOWLIST -- NOT AGAINST A LIST OF THE WINDOWS-ONLY ONES.
#
# WHY THIS EXISTS. The first three-platform release failed on both non-Windows
# legs at `Join-Path $env:USERPROFILE '.cargo\bin'`. Off Windows that variable
# is not merely empty, it is absent; `Join-Path` refuses a null `-Path`; and
# `$ErrorActionPreference = 'Stop'` promotes the refusal to a fatal error. The
# Linux job died in 41 seconds having compiled nothing.
#
# Every gate in this file used to run on Windows only, where the faulty line
# works perfectly. That is the actual defect: a Windows-only assumption in a
# script whose entire purpose is to run on three platforms cannot be caught by
# executing it there, so it has to be caught by reading it. It was in fact read,
# and called harmless -- prepending an empty string is a no-op, which is true of
# the assignment and irrelevant to the call that produces it.
#
# WHY THE DENYLIST WENT. This step used to carry eleven Windows-only names --
# USERPROFILE, APPDATA, LOCALAPPDATA, PROGRAMFILES, PROGRAMDATA, SYSTEMROOT,
# WINDIR, COMSPEC, USERNAME, HOMEDRIVE, HOMEPATH -- and TEMP was not among them.
# So the seventeen unguarded `$env:TEMP` uses in this very file, the exact
# spelling named in the header as the thing that killed the release, were
# invisible to the step written to catch that class. A denylist fails by
# omission and reports the omission as cleanliness; and the omission is always
# the variable nobody thought of, which is the definition of the case that bites.
#
# So the polarity is inverted. EVERY dereference is a finding unless it is
# guarded, or the name is on a short list of variables that genuinely exist on
# all three platforms. Adding a new one to that list is a visible diff with a
# reason beside it. Forgetting to is a red gate rather than silence.
#
# BOTH SPELLINGS. `$env:NAME` and `${env:NAME}` are the same dereference, and
# this step used to match only the first -- so `${env:ProgramFiles(x86)}` was
# invisible to it too. On 2026-08-09 a new helper in this file read four
# Windows-only variables unguarded, the step failed the gate naming three of
# them, and the fourth was the one written in braces. The braced form is also
# the ONLY way to write a variable whose name is not a bare identifier, so the
# names most likely to need it were exactly the ones exempt.
#
# A use is accepted inside an `if` that tests the same variable, or inside one
# that tests the platform. Guard scope is tracked by brace depth.
function Find-UnguardedEnvUses {
    param([string[]]$Lines, [string]$Label, [string[]]$Everywhere)
    $problems = @()
    $depth = 0
    $guards = @{}          # depth -> names guarded at that depth
    $platformGuard = @{}

    for ($i = 0; $i -lt $Lines.Count; $i++) {
        # Single-quoted literals FIRST, then comments. A comment naming a
        # variable is documentation and a quoted 'USERPROFILE' is a string, not
        # a dereference -- this very step would otherwise flag the prose above
        # it, which is the mistake the release-workflow checker already made
        # once and had to be taught not to repeat. The old version of this line
        # said it stripped both and stripped only comments; it survived because
        # no single-quoted literal in these three files happened to spell one of
        # the eleven names. That is luck, and this step is about not relying on
        # it.
        $code = $Lines[$i] -replace "'[^']*'", '' -replace '(^|\s)#.*$', ''

        # Every name this line dereferences, in either spelling.
        $names = @()
        foreach ($m in [regex]::Matches($code, '\$env:([A-Za-z_][A-Za-z0-9_]*)|\$\{env:([^}]+)\}')) {
            $n = if ($m.Groups[1].Success) { $m.Groups[1].Value } else { $m.Groups[2].Value }
            $names += $n
        }
        if ($names.Count -eq 0) {
            $depth += ([regex]::Matches($code, '\{')).Count - ([regex]::Matches($code, '\}')).Count
            if ($depth -lt 0) { $depth = 0 }
            foreach ($d in @($guards.Keys))        { if ($d -gt $depth) { $guards.Remove($d) } }
            foreach ($d in @($platformGuard.Keys)) { if ($d -gt $depth) { $platformGuard.Remove($d) } }
            continue
        }

        $opensGuard = @()
        $guardsPlatform = $false
        if ($code -match '^\s*(\}\s*)?(else)?if\s*\(') {
            $opensGuard = @($names)
            if ($code -match '\$(IsWindows|onWindows|IsLinux|IsMacOS|onMac|onLinux)\b') { $guardsPlatform = $true }
        }

        $active = @()
        $platformActive = $false
        foreach ($d in $guards.Keys) { if ($d -le $depth) { $active += $guards[$d] } }
        foreach ($d in $platformGuard.Keys) { if ($d -le $depth -and $platformGuard[$d]) { $platformActive = $true } }

        foreach ($v in ($names | Sort-Object -Unique)) {
            # Compared case-insensitively, because Windows environment variable
            # names are and `$env:ProgramFiles` and `$env:PROGRAMFILES` are one
            # variable.
            if ($Everywhere -contains $v.ToUpperInvariant()) { continue }
            if ($opensGuard -contains $v) { continue }            # this line IS the guard
            if ($active -contains $v) { continue }                # inside its own guard
            if ($platformActive -or $guardsPlatform) { continue }  # inside a platform branch
            # A bare assignment cannot fail; only passing it onward can.
            if ($code -match ('^\s*(\$env:' + [regex]::Escape($v) + '\b|\$\{env:' + [regex]::Escape($v) + '\})\s*=')) { continue }
            # ONE FORMAT STRING, not three concatenated with `-f` on the last.
            # `-f` binds tighter than `+`, so the first version formatted only
            # the final literal and the message came out reading
            # "{0}:{1} reads $env:{2} outside any guard" -- which is also why
            # the self-test below matches on the message and not merely on the
            # count.
            $problems += ("{0}:{1} reads `$env:{2} outside any guard. That variable is not on the everywhere list, so it may be ABSENT rather than empty on some platform -- and Join-Path treats a null -Path as a terminating error: {3}" -f
                          $Label, ($i + 1), $v, $code.Trim())
        }

        if ($opensGuard.Count -or $guardsPlatform) {
            $inner = $depth + 1
            if ($opensGuard.Count) { $guards[$inner] = $opensGuard }
            if ($guardsPlatform)   { $platformGuard[$inner] = $true }
        }
        $depth += ([regex]::Matches($code, '\{')).Count - ([regex]::Matches($code, '\}')).Count
        if ($depth -lt 0) { $depth = 0 }
        foreach ($d in @($guards.Keys))        { if ($d -gt $depth) { $guards.Remove($d) } }
        foreach ($d in @($platformGuard.Keys)) { if ($d -gt $depth) { $platformGuard.Remove($d) } }
    }
    return $problems
}

Step 'the cross-platform scripts touch no environment variable unguarded' {
    # THE ALLOWLIST, which is two names and has to earn both.
    #
    # PATH is the only variable POSIX and Windows both guarantee. PL_CORPUS is
    # set by this file before it is read, so it is this repository's own and
    # exists exactly when the code that reads it says it does.
    #
    # Everything else -- TEMP, USERPROFILE, HOME, SystemRoot, ProgramFiles,
    # LOCALAPPDATA, ProgramData, RUNNER_TEMP -- is absent on at least one of the
    # three platforms and must be guarded where it is read. HOME is the mirror
    # case worth naming: it is the Unix one, it does not exist on Windows, and
    # the guard on the cargo-bin line at the top of this file is there because
    # of this rule rather than in spite of it.
    $everywhere = 'PATH', 'PL_CORPUS'

    $files = 'tools/release.ps1', 'tools/ci.ps1', 'tools/check-archive.ps1'
    $problems = @()
    $seen = @{}
    foreach ($rel in $files) {
        $p = "$repo/$rel"
        if (-not (Test-Path $p)) { throw "$rel is missing" }
        $lines = @(Get-Content -LiteralPath $p)
        $problems += @(Find-UnguardedEnvUses -Lines $lines -Label $rel -Everywhere $everywhere)
        foreach ($m in [regex]::Matches(($lines -join "`n"), '\$env:([A-Za-z_][A-Za-z0-9_]*)|\$\{env:([^}]+)\}')) {
            $n = if ($m.Groups[1].Success) { $m.Groups[1].Value } else { $m.Groups[2].Value }
            $seen[$n.ToUpperInvariant()] = $true
        }
    }

    # THE ALLOWLIST ITSELF IS CHECKED. A name on it that nothing reads is slack,
    # and slack on an allowlist is how the next variable gets waved through by a
    # line somebody added "while I am here". Both of these are read today.
    foreach ($v in $everywhere) {
        if (-not $seen.Contains($v)) {
            $problems += "the everywhere list names $v, which none of the three scripts reads; a name nobody uses is a hole nobody notices"
        }
    }

    # THE CONTROL, and it is the whole reason this step was rewritten. A scanner
    # that has stopped matching reports the same clean as a clean tree. Four
    # planted lines: the two shapes that must be caught, and the two guards that
    # must not be. TEMP is deliberately the subject of the first, because TEMP is
    # exactly what the eleven-name denylist could not see.
    $planted = @(
        '$out = Join-Path $env:TEMP ''probe''',                   # 1: must be caught
        'if (${env:ProgramFiles(x86)}) { $x = 1 }',               # 2: guard, must not
        '$y = Join-Path ${env:ProgramFiles(x86)} ''Git''',        # 3: outside it, must be caught
        'if ($env:TEMP) { $z = Join-Path $env:TEMP ''ok'' }'      # 4: guard + use, must not
    )
    $found = @(Find-UnguardedEnvUses -Lines $planted -Label 'planted' -Everywhere $everywhere)
    $caught = @($found | ForEach-Object { if ($_ -match '^planted:(\d+)') { [int]$Matches[1] } })
    if (($caught | Sort-Object) -join ',' -ne '1,3') {
        throw ("the scanner caught planted lines [$($caught -join ', ')] and should have caught exactly 1 and 3. " +
               "It is no longer measuring what this step claims:`n        " + ($found -join "`n        "))
    }

    if ($problems) { throw ($problems -join "`n        ") }
    Write-Host ("        $($files.Count) scripts, $($seen.Count) environment variable(s) read, " +
                "all guarded or on the everywhere list; scanner verified on planted input") -ForegroundColor DarkGray
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
    # NOT `$exe`, which is this file's platform suffix. A local named `$exe`
    # inside a step body does not clobber the script-level one -- PowerShell
    # creates it in the scriptblock's own scope -- but a reader cannot tell that
    # from the line, and the next person to move a line would be right to worry.
    $plAbs = (Resolve-Path $pl).Path
    $out = python bench/run.py bench/polylinker-bench.json -- $plAbs bench-adapter 2>&1
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
    # COMPARED AS TEXT WITH THE LINE ENDINGS NORMALISED, not as a SHA-256 of
    # the bytes, and the reason is the same one written out at length above
    # 'renderers agree (fixture is current)': `>` re-encodes the generator's
    # stdout with the PLATFORM's newline. `bench/polylinker-bench.json` is
    # committed with CRLF because it was last regenerated through this line on
    # Windows, so a byte hash compares a Windows-written fixture against a
    # Linux-written regeneration and reports "generate.py is no longer
    # deterministic" about a file generate.py produced identically.
    #
    # Nothing is lost that this step ever claimed. Its subject is whether
    # generate.py's OUTPUT is a function of its input; the newline the shell
    # wrapped that output in is a property of the shell. `-cne`, so a case
    # change in a checksum is still a difference.
    #
    # generate.py writes to stdout and takes no output path, which is why this
    # cannot follow the fixture step's better pattern of having the generator
    # write the file itself.
    $regen = Join-Path $tmp 'pl-bench-regen.json'
    python bench/generate.py > $regen 2>$null
    if ($LASTEXITCODE -ne 0) { Write-Output 'generate.py failed'; return }
    $a = ([IO.File]::ReadAllText((Join-Path $repo 'bench/polylinker-bench.json'))) -replace "`r`n", "`n"
    $b = ([IO.File]::ReadAllText($regen)) -replace "`r`n", "`n"
    Remove-Item -LiteralPath $regen -Force -ErrorAction SilentlyContinue
    # A floor, because two empty strings compare equal: if the generator wrote
    # nothing and the fixture were ever emptied, this would report ok.
    if ($a.Length -lt 1000 -or $b.Length -lt 1000) {
        Write-Output "the fixture is $($a.Length) chars and the regeneration $($b.Length); one of them is empty and this comparison proved nothing"
        $global:LASTEXITCODE = 1
    } elseif ($a -cne $b) {
        Write-Output 'generate.py is no longer deterministic'
        $global:LASTEXITCODE = 1
    } else { $global:LASTEXITCODE = 0 }
} { (HavePy 'seguid') -and (HavePy 'pydna') }

# The feature-database rules. Both of these lived only in .github/workflows/
# ci.yml, so the local gate could not tell you that you had broken them and you
# found out from a red badge after pushing -- which is the same complaint this
# file's own header makes about anything that only runs somewhere else.
#
# They are here NOW because they are hermetic now. The writer audit used to mean
# running the real build against live EBI and NCBI, which is not something a
# local pre-push gate can honestly depend on; `check_writer.py` drives the same
# writer over the committed tables and touches no network, so it runs anywhere,
# offline, in about a second.
Write-Host "`nfeature database" -ForegroundColor Cyan
Step 'no feature row asserts more than a human signed' {
    python features/build/check_signoff.py --quiet
} { Have python }
Step 'the build''s writer reads SIGNOFF.tsv and never writes it' {
    python features/build/check_writer.py --quiet
} { Have python }
# THE FIRST PART OF THE TAINT GATE'S SUBJECT THAT CAN RUN HERE AT ALL.
#
# `features/build/taint_gate.py` fetches pLannotate's snapgene.csv from a third
# party, so it cannot live in this file: the rule this gate keeps is that a step
# whose result depends on somebody else's uptime belongs in a workflow. That
# left the project's central licensing control with no local twin, which
# `.github/workflows/ci.yml` states in its own header.
#
# This part needs nothing. It reads every stage module named in `build.STAGES`,
# requires each to declare what it does about a boundary convention arriving
# from a depositor's INSDC feature table -- ENA folds a submitter's SnapGene
# `/label` into the `/note`, so the route is real and the taint gate's
# description comparison cannot see it -- and then drives the mechanisms those
# declarations name: `stage_classb`'s SnapGene screen against a record carrying
# the tell and one without, `stage_uniprot`'s translation test against a CDS one
# residue out. Offline, about a second.
#
# READ features/SOURCING.md section 0.6 BEFORE QUOTING THIS STEP. It does not
# show that no coordinate here agrees with SnapGene's, and no check can; what it
# shows is that no stage reached the table without answering the question.
Step 'every stage declares what it does about SnapGene arriving through INSDC' {
    python features/build/insdc_posture.py
} { Have python }

# The archived licence evidence, against the manifest it was fetched under.
#
# `features/build/archive_legal.py` was run BY NO GATE ON ANY PLATFORM. Audit
# round 2 recorded that, round 3 re-derived it by intersecting `git ls-files`
# against this file plus all three workflows, and it was still true at v0.10.2.
# Its `--check` mode fetches nothing: it reads `legal/MANIFEST.json`, re-hashes
# the seven archived files beside it, and compares. All eight are committed, so
# this needs a Python interpreter and nothing else -- no network, no corpus, no
# platform, about a tenth of a second.
#
# WHY IT IS WORTH A STEP. `features/SOURCING.md` section 2 Stage 0.3 gives the
# reason those files exist: `www.uniprot.org/help/license` and the ENA policy
# page are JavaScript shells with ZERO licence text, so a URL in a document is
# evidence of nothing and a reader who follows it sees an empty page. What makes
# `legal/` evidence rather than a folder of HTML is the recorded sha256 and the
# retrieval date beside each file. A file that has drifted from its hash is a
# licence claim with nothing behind it, and until now nothing anywhere said so.
#
# THE FLOOR IS NOT DECORATION. `--check` walks `manifest["files"]`, so an EMPTY
# manifest prints "0 archived file(s), all matching their recorded sha256" and
# exits 0 -- a check that cannot fail, over an archive that has been deleted.
# The count is read back out of that line and floored at the seven committed
# today. The floor lives in this caller rather than in the script because the
# script is not this step's to edit, and a floor is cheaper here than a second
# copy of the rule there.
Step 'the archived licence evidence still matches the manifest it was fetched under' {
    $out = & python features/build/archive_legal.py --check 2>&1
    $code = $LASTEXITCODE
    if ($code -ne 0) { $out | ForEach-Object { Write-Output $_ }; $global:LASTEXITCODE = $code; return }
    $n = 0
    foreach ($line in $out) { if ("$line" -match '^(\d+) archived file\(s\), all matching') { $n = [int]$Matches[1] } }
    if ($n -lt 7) { throw "archive_legal.py --check reported $n archived file(s) and exited 0; legal/ holds seven, and an empty or truncated manifest is exactly the input that check reports ok over" }
    Write-Host ("        {0} archived licence file(s), each matching the sha256 recorded when it was fetched" -f $n) -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
} { Have python }

# THE SKIP DISCIPLINE -- because on a runner it is the only thing standing
# between "the gate passed" and "the gate did not run".
#
# A skip costs nothing here. On a workstation that is correct and deliberate:
# the alternative is a gate that goes red because somebody has not got SciPy,
# which is how a gate stops being read. On a runner it inverts. Nobody watches
# a green check for a list of grey lines, so a job that installs its oracles and
# then silently stops installing one of them -- a wheel that stops building for
# 3.14, a `pip install` line edited in a hurry -- goes on reporting success
# while measuring less every month. That is the same shape as the defect this
# whole file exists to answer: a check that cannot fail proves nothing, and a
# check that can be skipped without anybody being told cannot fail.
#
# WHAT CHANGED WHEN THE GATE WENT TO THREE PLATFORMS. The committed list used to
# name every step allowed to skip, full stop. That worked while there was one
# runner and cannot express "runs on Windows, cannot run on Linux": under three
# flat lists, or one list with a platform column, adding
# 'the built binaries carry their icon and version resource' to the Linux side
# is a one-line diff that reads as bookkeeping, and NOTHING anywhere then says
# the step still runs on Windows. Both of those designs add a second,
# unverified claim -- which platforms a step should run on -- on top of the one
# that is verified.
#
# So the platform half of the list is DERIVED instead. A precondition returns
# the reason it is unmet, from the two-word vocabulary at the top of this file,
# and three rules are checked here:
#
#   L1  Every skip carries a declared reason. $false -- a missing SciPy, a
#       missing node, a missing WiX -- is a skip nobody declared and is a
#       FAILURE. This is exactly what the old list bought, and it costs no list.
#   L2  A platform reason must agree with the platform: 'not windows' may only
#       appear when $IsWindows is false, and on Windows it may not appear at
#       all. So a Windows-only step cannot be made to skip on Windows by
#       relabelling it, and a step that is not Windows-only cannot be quietly
#       excused off Windows without hand-writing the string into its own
#       precondition under its own comment.
#   L3  The steps that skipped for want of a CORPUS are exactly the committed
#       list, by set equality in both directions, and every name on it names a
#       real step.
#
# L3 is the list that survives, and it is one flat list again -- because "there
# is no corpus on this machine" is a fact about the machine and not about the
# platform, so the SAME five names are right on all three runners. The file
# needs no platform column because it never has anything platform-shaped in it.
#
# Set equality for L3, and not:
#
#   * a count. `$skipped.Count -eq 5` is satisfied by the WRONG five skipping,
#     and the wrong five is the likely five: drop `scipy` and `pypdf` from the
#     install line and set PL_CORPUS to a directory holding two .dna files, and
#     the number is still five while three oracles have stopped running.
#   * a subset ("no more than these"). A step on the list that stopped skipping
#     is also news -- either somebody solved the precondition, in which case the
#     list should say so, or the precondition is now being satisfied by
#     something nobody intended. On a runner it means somebody put lab plasmids
#     on a runner, which is news of a different kind again.
#   * a floor. Nothing to floor: the right number is exact.
#
# The names must also BE names. A step renamed here while the list keeps the old
# spelling would present as "expected to skip and did not", which reads as good
# news and is not. That is the same shape as 'every coding record translates to
# its protein', which spent weeks running zero tests because it filtered on a
# name that had been renamed away -- so it is checked separately and said in
# different words.
#
# WHAT STILL CANNOT BE SEEN FROM ONE LEG: a step that skipped on ALL THREE
# platforms. Nothing here can tell that a step declared 'not windows' twice was
# ever observed to run anywhere. `tools/reconcile-ledgers.ps1` is where that is
# checked, over the -Ledger files the three runners upload.
if ($ExpectedSkips) {
    $checkName = 'the steps that skipped are exactly the ones this machine was told to expect'
    $problems = @()

    # L1 and L2 first, because they need no file and hold on a machine with no
    # list at all.
    foreach ($row in $script:stepLedger) {
        if ($row.State -ne 'skipped') { continue }
        if (-not $row.Reason) {
            $problems += ("`"$($row.Name)`" skipped and its precondition declared no reason. Install what it " +
                          'needs, or -- if it genuinely cannot run on this platform -- say so IN THE ' +
                          'PRECONDITION by returning one of: ' + ($script:ReasonVocabulary -join ', ') +
                          '. An unannounced skip is how six releases shipped behind a gate nobody was running.')
            continue
        }
        if ($script:ReasonVocabulary -notcontains $row.Reason) {
            $problems += ("`"$($row.Name)`" skipped with the reason `"$($row.Reason)`", which is not in the " +
                          'vocabulary (' + ($script:ReasonVocabulary -join ', ') + '). A reason nobody checks ' +
                          'is a label, and a label is what this replaced.')
            continue
        }
        # L2. The string is checked against the platform rather than believed.
        if ($row.Reason -eq 'not windows' -and $onWindows) {
            $problems += ("`"$($row.Name)`" skipped saying 'not windows', and this IS Windows. Either the " +
                          'precondition is lying or it is testing something else and calling it the platform.')
        }
    }
    # L3.
    if (-not (Test-Path -LiteralPath $ExpectedSkips)) {
        $problems += "-ExpectedSkips names $ExpectedSkips, which does not exist"
        $expected = @()
    } else {
        # `(^|\s)#` and not `#`, the same rule the env-var step above uses: a
        # step name is free to contain a '#', and only a '#' that opens a line
        # or follows whitespace is a comment.
        $expected = @(Get-Content -LiteralPath $ExpectedSkips |
            ForEach-Object { ($_ -replace '(^|\s)#.*$', '').Trim() } |
            Where-Object { $_ })
        $corpusSkips = @($script:stepLedger | Where-Object { $_.State -eq 'skipped' -and $_.Reason -eq 'corpus' } |
                         ForEach-Object { $_.Name })
        if ($expected.Count -eq 0 -and $corpusSkips.Count -eq 0) {
            # Not an error, but say it out loud rather than printing ok for a
            # comparison of two empty sets.
            Write-Host '        the list is empty and nothing skipped for want of a corpus' -ForegroundColor DarkGray
        }
        foreach ($e in $expected) {
            if ($script:steps -notcontains $e) {
                $problems += ("`"$e`" is on the list and names no step in this gate. It has been renamed or " +
                              'removed, and the list has been silently protecting nothing since.')
            } elseif ($corpusSkips -notcontains $e) {
                $problems += ("`"$e`" is on $ExpectedSkips and did not skip for want of a corpus. That file " +
                              'names ONLY the corpus skips -- a step that cannot run on this platform declares ' +
                              'it in its own precondition and must not be listed here. If a corpus really has ' +
                              'appeared on this machine, delete the line in the same commit.')
            }
        }
        foreach ($s in $corpusSkips) {
            if ($expected -notcontains $s) {
                $problems += ("`"$s`" skipped for want of a corpus and is not on the list. Add it, with the " +
                              'reason it needs one.')
            }
        }
    }

    if ($problems) {
        Write-Host ("  FAIL  {0}" -f $checkName) -ForegroundColor Red
        $problems | ForEach-Object { Write-Host "        $_" }
        $script:failed += $checkName
    } else {
        $declared = @($script:stepLedger | Where-Object { $_.State -eq 'skipped' })
        Write-Host ("  ok    {0} ({1} corpus, {2} skipped in all)" -f
                    $checkName, $expected.Count, $declared.Count) -ForegroundColor Green
    }
}

# The ledger, for the cross-platform reconciler. Written even when the run
# failed: a leg that went red still has a true record of what it ran, and the
# reconciler's job -- has every step run SOMEWHERE -- is exactly the question
# whose answer a failed leg still carries.
if ($Ledger) {
    $dir = Split-Path -Parent $Ledger
    if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
    # TAB-SEPARATED AND LF-TERMINATED, with the platform on the first line. A
    # step name may contain a comma (three do) and may not contain a tab.
    $osName = if ($onWindows) { 'windows' } elseif ($onMac) { 'macos' } elseif ($onLinux) { 'linux' } else { 'unknown' }
    $rows = @("# platform`t$osName")
    foreach ($row in $script:stepLedger) {
        if ($row.Name -match "`t") { throw "the step name `"$($row.Name)`" contains a tab and cannot go in the ledger" }
        $rows += ("{0}`t{1}`t{2}" -f $row.Name, $row.State, $row.Reason)
    }
    [System.IO.File]::WriteAllText($Ledger, ($rows -join "`n") + "`n")
    Write-Host ("        ledger: $($script:stepLedger.Count) step(s) -> $Ledger") -ForegroundColor DarkGray
}

$elapsed = (Get-Date) - $started
Write-Host ''
if ($script:skipped) {
    # WITH THE REASON. The old line printed the names alone, so a reader could
    # not tell "no corpus here" from "this cannot run on this platform" from
    # "somebody's SciPy stopped importing" -- three quite different pieces of
    # news that the summary rendered identically.
    Write-Host 'skipped:' -ForegroundColor DarkGray
    foreach ($row in ($script:stepLedger | Where-Object { $_.State -eq 'skipped' })) {
        $why = if ($row.Reason) { $row.Reason } else { 'no reason declared' }
        Write-Host ("  {0}  ({1})" -f $row.Name, $why) -ForegroundColor DarkGray
    }
}
if ($script:failed.Count -eq 0) {
    Write-Host ("GATE PASSED in {0:N0}s" -f $elapsed.TotalSeconds) -ForegroundColor Green
    exit 0
} else {
    Write-Host ("GATE FAILED: {0}" -f ($script:failed -join ', ')) -ForegroundColor Red
    exit 1
}
