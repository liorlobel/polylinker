<#
.SYNOPSIS
    Build the single-file browser tool.

.DESCRIPTION
    Compiles pl-wasm to wasm32 and inlines it into the HTML as base64, producing
    one self-contained file.

    Inlining rather than shipping a .wasm alongside is the whole point: the page
    has to work from a USB stick, opened over file://, on a managed PC with no
    install rights and no network. A second file that must be fetched breaks
    that -- browsers block cross-origin fetches from file:// URLs.

    The cost is a third more bytes than the raw module. It is worth it.

.EXAMPLE
    .\tools\build-web.ps1
    .\tools\build-web.ps1 -Open
#>
[CmdletBinding()]
param(
    [switch]$Open,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:PATH = "$cargoBin;$env:PATH" }

$template = Join-Path $repo 'prototype\dna-reader.template.html'
$output = Join-Path $repo 'prototype\dna-reader.html'
$wasm = Join-Path $repo 'target\wasm32-unknown-unknown\wasm\pl_wasm.wasm'

if (-not (Test-Path $template)) { throw "template not found: $template" }

if (-not $SkipBuild) {
    Write-Host 'building pl-wasm for wasm32-unknown-unknown ...'
    Push-Location $repo
    try {
        $targets = rustup target list --installed
        if ($targets -notcontains 'wasm32-unknown-unknown') {
            Write-Host '  adding the wasm32-unknown-unknown target'
            rustup target add wasm32-unknown-unknown | Out-Null
        }
        cargo build -p pl-wasm --target wasm32-unknown-unknown --profile wasm
        if ($LASTEXITCODE -ne 0) { throw 'wasm build failed' }
    } finally { Pop-Location }
}

if (-not (Test-Path $wasm)) { throw "wasm artifact not found: $wasm" }

$bytes = [System.IO.File]::ReadAllBytes($wasm)
$b64 = [Convert]::ToBase64String($bytes)

$html = [System.IO.File]::ReadAllText($template)
$marker = '{{WASM_BASE64}}'
if (-not $html.Contains($marker)) { throw "template has no $marker placeholder" }
$html = $html.Replace($marker, $b64)

# The molecule shown on load is a committed GenBank file, read through the same
# core as any real file. Inlined as a JSON string literal so quoting, newlines
# and any non-ASCII survive intact.
$demoPath = Join-Path $repo 'prototype\demo-construct.gb'
if (-not (Test-Path $demoPath)) { throw "demo not found: $demoPath (run tools/make-demo.py)" }
$demo = [System.IO.File]::ReadAllText($demoPath) -replace "`r`n", "`n"
$demoLiteral = $demo | ConvertTo-Json -Compress
$html = $html.Replace('{{DEMO_GENBANK}}', $demoLiteral)

# Stamp provenance so a stale build is identifiable from the page itself.
$stamp = "pl-wasm {0:N0} bytes, built {1}" -f $bytes.Length, (Get-Date -Format 'yyyy-MM-dd HH:mm')
$html = $html.Replace('{{BUILD_STAMP}}', $stamp)

[System.IO.File]::WriteAllText($output, $html, (New-Object System.Text.UTF8Encoding($false)))

$outSize = (Get-Item $output).Length
Write-Host ''
Write-Host ("  wasm      {0,10:N0} bytes" -f $bytes.Length)
Write-Host ("  base64    {0,10:N0} bytes" -f $b64.Length)
Write-Host ("  html out  {0,10:N0} bytes  ({1:N0} KB)" -f $outSize, ($outSize / 1KB))
Write-Host ("  -> {0}" -f $output)
Write-Host ''
Write-Host 'Single file, no network, no install. Open it directly or hand it to someone.'

if ($Open) { Start-Process $output }
