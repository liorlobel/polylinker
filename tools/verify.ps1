<#
.SYNOPSIS
    Build and verify Polylinker on Windows.

.DESCRIPTION
    Runs the same checks as CI: unit tests, corpus validation against real
    files, clippy, formatting, and the two cross-implementation checks against
    Python and Biopython.

    Requires a linker. rustup does not ship one, so on Windows this needs the
    MSVC toolset (Visual Studio Build Tools, "MSVC v143 ... build tools" plus a
    Windows SDK) or MinGW-w64.

.PARAMETER Corpus
    Directory of real .dna / .gb files. Corpus tests skip without it.

.EXAMPLE
    .\tools\verify.ps1 -Corpus "$env:USERPROFILE\OneDrive\plasmids"
#>
[CmdletBinding()]
param(
    [string]$Corpus,
    [switch]$SkipCrossChecks
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

# rustup installs per-user and does not always modify PATH.
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:PATH = "$cargoBin;$env:PATH" }

function Section($t) { Write-Host "`n=== $t ===" -ForegroundColor Cyan }
function Fail($t) { Write-Host "  FAIL  $t" -ForegroundColor Red; $script:failed++ }
function Pass($t) { Write-Host "  ok    $t" -ForegroundColor Green }
$script:failed = 0

Section 'toolchain'
try {
    Write-Host "  $(rustc --version)"
    Write-Host "  $(cargo --version)"
} catch {
    Write-Host "  cargo not found. Install from https://rustup.rs" -ForegroundColor Red
    exit 1
}

# A missing linker is the most common Windows failure and produces a confusing
# error deep in a build log, so check for it up front and say what to install.
Section 'linker'
$link = Get-Command link.exe -ErrorAction SilentlyContinue
if (-not $link) {
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vs = & $vswhere -latest -products * `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath 2>$null
        if ($vs) {
            $link = Get-ChildItem "$vs\VC\Tools\MSVC" -Recurse -Filter 'link.exe' `
                -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match '\\Hostx64\\x64\\' } |
                Select-Object -First 1
        }
    }
}
if ($link) {
    Write-Host "  found: $($link.Source ?? $link.FullName)"
} else {
    Write-Host @"
  No MSVC linker found. cargo cannot link on the msvc target without one.
  Install Visual Studio Build Tools with the C++ toolset:

    winget install --id Microsoft.VisualStudio.2022.BuildTools --exact ``
      --override "--quiet --wait --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64"

  Then run this from a *new* shell so the environment is picked up.
"@ -ForegroundColor Yellow
    exit 1
}

Section 'unit tests'
$out = cargo test --workspace --lib 2>&1
$sum = ($out | Select-String '^test result').Line
if ($LASTEXITCODE -eq 0) { Pass ($sum -join '; ') } else { $out | Select-Object -Last 25; Fail 'unit tests' }

if ($Corpus) {
    if (-not (Test-Path $Corpus)) { throw "corpus not found: $Corpus" }
    $env:PL_CORPUS = (Resolve-Path $Corpus).Path
    Section "corpus validation  ($env:PL_CORPUS)"
    $out = cargo test -p pl-fileio --test corpus -- --nocapture --test-threads=4 2>&1
    $out | Select-String 'byte-exact|dropping derived|dna ->|GenBank:|digest inv|declared a length|misrepresent' |
        ForEach-Object { Write-Host "  $($_.Line.Trim())" }
    if ($LASTEXITCODE -eq 0) { Pass 'corpus tests' } else { $out | Select-Object -Last 30; Fail 'corpus tests' }
} else {
    Section 'corpus validation'
    Write-Host '  skipped: pass -Corpus <dir> to enable' -ForegroundColor DarkGray
}

Section 'lint and format'
$clippy = cargo clippy --workspace --all-targets 2>&1
$n = ($clippy | Select-String '^(warning|error)(:|\[)').Count
if ($n -eq 0) { Pass 'clippy clean' } else { $clippy | Select-String -Context 0,4 '^(warning|error)(:|\[)' | Select-Object -First 6; Fail "$n clippy finding(s)" }
cargo fmt --all --check *> $null
if ($LASTEXITCODE -eq 0) { Pass 'rustfmt clean' } else { Fail 'rustfmt would reformat (run: cargo fmt --all)' }

Section 'release binary'
cargo build --release -q 2>&1 | Out-Null
$bin = Join-Path $repo 'target\release\pl.exe'
if (Test-Path $bin) {
    Pass ("pl.exe  {0:N0} bytes" -f (Get-Item $bin).Length)
    & $bin --version
} else { Fail 'no binary produced' }

if ($Corpus -and -not $SkipCrossChecks) {
    $py = Get-Command python -ErrorAction SilentlyContinue
    if (-not $py) {
        Section 'cross-checks'
        Write-Host '  skipped: python not found' -ForegroundColor DarkGray
    } else {
        Section 'cross-check: rust reader vs python reader'
        python "$repo\reference\python\tests\xcheck_rust.py" $bin "$env:PL_CORPUS\**\*.dna" 2>&1 |
            Select-Object -Last 8 | ForEach-Object { Write-Host "  $_" }
        if ($LASTEXITCODE -ne 0) { Fail 'rust and python readers disagree' }

        Section 'cross-check: biopython reads what pl writes'
        $tmp = Join-Path $env:TEMP 'pl-gb-interop'
        python "$repo\reference\python\tests\xcheck_rust_genbank.py" $bin "$env:PL_CORPUS\**\*.dna" $tmp 2>&1 |
            Select-Object -First 7 | ForEach-Object { Write-Host "  $_" }
        if ($LASTEXITCODE -ne 0) { Fail 'biopython rejected output' }
    }
}

Write-Host ''
if ($script:failed -eq 0) {
    Write-Host 'ALL CHECKS PASSED' -ForegroundColor Green
    exit 0
} else {
    Write-Host "$($script:failed) CHECK(S) FAILED" -ForegroundColor Red
    exit 1
}
