<#
.SYNOPSIS
    Assert that no shipped Windows binary needs a C runtime the user cannot install.

.DESCRIPTION
    VCRUNTIME140.dll is not part of Windows. It arrives with the Visual C++
    2015-2022 redistributable, whose installer requires administrator rights, and
    `docs/PLAN.md:120` describes the primary user as someone who has none. On a
    freshly imaged locked-down machine an app that imports it is a missing-DLL
    dialog rather than an app. `.cargo/config.toml` links the CRT statically to
    remove that dependency, for every Windows triple.

    THE CHECK IS A BYTE SCAN, NOT `dumpbin`. An imported DLL's name is stored as
    a literal ASCII string in the PE import directory, so if the import exists
    the string is certainly there. The check asserts ABSENCE, so the only error
    this method can make is a false FAILURE from an incidental occurrence of the
    name -- the safe direction, and one that would be investigated rather than
    ignored. It also needs nothing installed, so unlike `dumpbin` it runs on a
    Rust-only machine, on a CI runner, and -- the reason this file exists -- on
    the `windows-11-arm` runner, which is the one place a `dumpbin` from the x64
    MSVC layout could not be relied on.

    WHY THIS IS A FILE AND NOT A FUNCTION IN tools/ci.ps1. The gate has three
    legs and none of them is ARM64. Between v0.11.0 and this file, the one
    architecture whose `.cargo/config.toml` block was missing was the one
    architecture no caller of this scan could reach, and the result shipped: all
    four ARM64 binaries in v0.11.0 import VCRUNTIME140.dll. A scan that lives
    inside the gate can only ever check what the gate runs on. This one is
    called by `tools/ci.ps1` AND by the ARM64 leg of `.github/workflows/ci.yml`,
    and both get the same implementation rather than a transcription of it.

.PARAMETER Dir
    Directory to scan. Every .exe, .pyd and .dll directly inside it is read.

.PARAMETER MinBinaries
    Fail if fewer than this many binaries were examined. A scan that found
    nothing to scan has checked nothing and must not report success -- this is
    the vacuity guard, and it is the reason this script cannot quietly pass on
    an empty or mistyped directory. Defaults to 4: polylinker.exe, pl.exe,
    pl-mcp.exe and polylinker.pyd, which is what every Windows release carries.

.OUTPUTS
    Nothing on success, beyond one summary line. Throws on failure.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Dir,
    [int]$MinBinaries = 4
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $Dir)) {
    throw "check-crt: there is no directory '$Dir', so this scan read nothing"
}

# MSVCP140 is the C++ half and api-ms-win-crt-runtime the ucrt forwarder; a
# binary that static-links the CRT has none of the four. VCRUNTIME140_1.dll is
# listed separately because it is a distinct file that ships beside the first
# one and is missing independently of it.
$banned = 'VCRUNTIME140.dll', 'VCRUNTIME140_1.dll', 'MSVCP140.dll', 'api-ms-win-crt-runtime-l1-1-0.dll'

$checked = 0
$offenders = @()
foreach ($f in (Get-ChildItem -LiteralPath $Dir -File |
                Where-Object { $_.Extension -in '.exe', '.pyd', '.dll' })) {
    $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
    $ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
    foreach ($b in $banned) {
        if ($ascii.IndexOf($b, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
            $offenders += "$($f.Name) references $b"
        }
    }
    $checked++
}

if ($offenders.Count -gt 0) {
    throw ("these binaries need a C runtime the user may not be allowed to install:" +
           [Environment]::NewLine + '  ' + ($offenders -join ([Environment]::NewLine + '  ')) +
           [Environment]::NewLine +
           'Check .cargo/config.toml still sets +crt-static FOR THIS TARGET TRIPLE -- it has one ' +
           '[target.<triple>] block per Windows architecture and a missing block fails exactly ' +
           'this way -- and that no job in the workflow sets RUSTFLAGS, which REPLACES those ' +
           'flags rather than merging with them.')
}

if ($checked -lt $MinBinaries) {
    throw ("check-crt: only $checked binary(ies) examined in '$Dir'; expected at least " +
           "$MinBinaries. A scan with nothing to scan has checked nothing, and reporting " +
           'success here would be the failure this script exists to prevent.')
}

Write-Host "        $checked binaries, none needs the VC++ redistributable" -ForegroundColor DarkGray
