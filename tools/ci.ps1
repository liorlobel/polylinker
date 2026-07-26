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

Write-Host "`nbenchmark" -ForegroundColor Cyan
Step 'polylinker-bench' {
    python bench/run.py bench/polylinker-bench.json -- target/release/pl.exe bench-adapter
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
