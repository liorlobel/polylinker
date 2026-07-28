<#
.SYNOPSIS
    Build the release artifacts and record exactly what they are.

.DESCRIPTION
    Produces the binaries, a SHA-256 manifest, and a build record naming the
    toolchain and commit. Signs them if — and only if — credentials are
    supplied; otherwise it says loudly that the output is unsigned and keeps
    going, because an unsigned build that says so is useful and an unsigned
    build that pretends otherwise is not.

    What this script deliberately does NOT do:

    * **It does not auto-update anything.** See docs/RELEASING.md. A tool whose
      selling point is that it runs offline and sends nothing anywhere should
      not also fetch and execute code it was not asked for.
    * **It does not create or handle signing keys.** It takes an identity that
      already exists in the platform's own store and asks the platform's own
      tool to use it. A build script that can mint a signing key is a build
      script that can leak one.

.PARAMETER WindowsCert
    Subject name of a code-signing certificate in the current user's store.
    Omit to produce an unsigned Windows build.

.PARAMETER MacIdentity
    A "Developer ID Application: ..." identity. Omit to produce an unsigned
    macOS build. Notarisation additionally needs an app-specific password and
    is not attempted here without one.

.PARAMETER Out
    Where the artifacts go. Default: dist/
#>
param(
    [string]$WindowsCert = '',
    [string]$MacIdentity = '',
    [string]$Out = 'dist',

    # For the gate, which checks that the script runs and that its manifest
    # verifies. Nothing is suppressed that changes what is produced.
    [switch]$Quiet
)

$ErrorActionPreference = 'Stop'
function Say($msg, $colour = 'Gray') { if (-not $Quiet) { Write-Host $msg -ForegroundColor $colour } }
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Same as tools/ci.ps1: a shell that has not sourced the profile does not have
# the toolchain on PATH, and a release script failing on that is noise.
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:PATH = "$cargoBin;$env:PATH" }

Say 'Polylinker release' Cyan

# The build record. A release nobody can trace back to a commit is a release
# nobody can reproduce, and "which build is this?" is the first question after
# any bug report.
$commit = (git rev-parse HEAD 2>$null)
if (-not $commit) { $commit = 'not a git checkout' }
$dirty = (git status --porcelain 2>$null)
$rustc = (rustc --version)
$stamp = (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssZ')

if ($dirty) {
    Say '  WARNING: the working tree has uncommitted changes.' Yellow
    Say '  The commit recorded below does not describe these binaries.' Yellow
}

New-Item -ItemType Directory -Force $Out | Out-Null

Say '  building...'
# `$ErrorActionPreference = 'Stop'` turns *any* stderr from a native command
# into a terminating error, and cargo writes warnings there. A warning must not
# abort a release — it did, on an "output filename collision" note about a PDB —
# so the exit code is what decides, which is the only thing that actually says
# whether the build worked.
$prevEAP = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
cargo build --release --workspace 2>&1 | Out-Null
$buildOk = ($LASTEXITCODE -eq 0)
$ErrorActionPreference = $prevEAP
if (-not $buildOk) { throw 'the release build failed' }

$artifacts = @()
foreach ($name in 'pl.exe', 'polylinker.exe', 'pl-mcp.exe') {
    $p = Join-Path 'target/release' $name
    if (Test-Path $p) {
        Copy-Item $p (Join-Path $Out $name) -Force
        $artifacts += $name
    }
}

# The Python extension has to be *named* for the platform or it cannot be
# imported at all. CPython on Windows loads `.pyd`, not `.dll`, however
# correctly the DLL was built — and cargo has no say in the matter, so the
# rename belongs here. Shipping `polylinker.dll` and expecting the user to
# rename it is a papercut that reads as "the wheel is broken".
$pyBuilt = Join-Path 'target/release' 'polylinker.dll'
$pyShipped = 'polylinker.pyd'
if (-not (Test-Path $pyBuilt)) {
    $pyBuilt = Join-Path 'target/release' 'libpolylinker.so'
    $pyShipped = 'polylinker.so'
}
if (Test-Path $pyBuilt) {
    Copy-Item $pyBuilt (Join-Path $Out $pyShipped) -Force
    $artifacts += $pyShipped
}

if (-not $artifacts) { throw 'the build produced nothing to ship' }

# Signing. Absent credentials this reports and continues; it never silently
# produces something that looks signed.
$signed = @()
if ($WindowsCert) {
    Say "  signing with '$WindowsCert'..."
    foreach ($a in $artifacts) {
        $f = Join-Path $Out $a
        # The platform's own tool, using an identity already in the platform's
        # own store. Nothing here handles key material.
        & signtool sign /n $WindowsCert /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $f
        if ($LASTEXITCODE -ne 0) { throw "signtool failed on $a" }
        $signed += $a
    }
} else {
    Say '  NOT SIGNED: no -WindowsCert given.' Yellow
    Say '  Windows SmartScreen will warn on first run. See docs/RELEASING.md.' Yellow
}
if ($MacIdentity) {
    Say '  macOS signing must run on macOS; skipping here.' Yellow
}

# SHA-256 over every artifact. This is what a user or an IT department can check
# against, and it is the only integrity guarantee an unsigned build has.
$manifest = @()
$manifest += "polylinker release manifest 1"
$manifest += "built: $stamp"
$manifest += "commit: $commit$(if ($dirty) { ' (WORKING TREE DIRTY - not reproducible)' })"
$manifest += "rustc: $rustc"
$manifest += "signed: $(if ($signed) { $signed -join ', ' } else { 'no' })"
$manifest += '--'
foreach ($a in $artifacts | Sort-Object) {
    $h = (Get-FileHash (Join-Path $Out $a) -Algorithm SHA256).Hash.ToLower()
    $manifest += "$h  $a"
}
$manifestPath = Join-Path $Out 'SHA256SUMS.txt'
# LF, no BOM, and pure ASCII.
#
# The first two because a checksum file with CRLF or a BOM does not verify with
# sha256sum on the machine of whoever is checking it. ASCII because Windows
# PowerShell 5.1 reads a BOM-less script as ANSI, so a non-ASCII character in a
# string here is read as several and written back out double-encoded: an em-dash
# in the dirty-tree warning arrived in the manifest as three mojibake bytes.
# There is nothing a checksum file needs that ASCII cannot spell.
[System.IO.File]::WriteAllText(
    (Resolve-Path -LiteralPath $Out).Path + [System.IO.Path]::DirectorySeparatorChar + 'SHA256SUMS.txt',
    ($manifest -join "`n") + "`n",
    (New-Object System.Text.UTF8Encoding($false))
)

Say ''
if (-not $Quiet) { Get-Content $manifestPath | ForEach-Object { Write-Host "  $_" } }
Say ''
if (-not $signed) {
    Say 'This build is UNSIGNED.' Yellow
    Say 'Publish SHA256SUMS.txt beside it so it can be verified anyway.' Yellow
}
