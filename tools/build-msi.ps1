<#
.SYNOPSIS
    Build polylinker-<version>-windows-x64.msi from a dist/ that tools/release.ps1
    has already produced.

.DESCRIPTION
    THE POINT OF THIS SCRIPT IS THAT THE MSI HAS NO FILE LIST OF ITS OWN.

    docs/RELEASING.md argued against a compiled installer on the grounds that
    every one of them carries a second list of files -- "a WiX <Component> set,
    an Inno [Files] section" -- which drifts from the real payload. That is not
    a hypothetical: the notices list in tools/release.ps1 drifted twice in a
    single week, on 2026-08-03 and 2026-08-04, and each time a licence text
    stopped shipping.

    So Polylinker.wxs contains no files. This script reads dist/SHA256SUMS.txt --
    the manifest tools/release.ps1 writes, Install-Polylinker.ps1 installs from,
    and tools/check-archive.ps1 verifies -- and GENERATES the component set from
    it. A file that reaches the archive reaches the MSI, and a gate step asserts
    the two agree.

    It writes nothing into dist/. Three separate set-equality checks compare
    dist/ against the manifest and exclude only SHA256SUMS.txt, *.zip and
    *.zip.sha256, so an .msi left there would either be swept into the manifest
    and the zip, or fail those checks. Output goes to -Out, which defaults to a
    sibling directory.

.PARAMETER Dist
    The directory tools/release.ps1 produced. Must contain SHA256SUMS.txt.

.PARAMETER Out
    Where to write the .msi. Created if absent. Never dist/.

.PARAMETER Version
    Overrides the version. By default it is read from the workspace Cargo.toml,
    which is the single source; release.ps1, ci.ps1 and bins/winres.rs all
    re-read that same string rather than restating it.

.PARAMETER KeepIntermediate
    Leave the generated Payload.wxs and LICENSE.rtf next to the .msi for
    inspection. The gate uses this.
#>
[CmdletBinding()]
param(
    [string]$Dist = 'dist',
    [string]$Out = 'msi',
    [string]$Version,
    [switch]$KeepIntermediate,
    # Generate Payload.wxs and LICENSE.rtf and stop, without invoking wix. This
    # is what lets tools/ci.ps1 assert that the MSI's file set equals the
    # manifest on a machine that has no .NET SDK -- which is the machine this
    # project is developed on, so without it the most important MSI check would
    # only ever run in CI.
    [switch]$GenerateOnly
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

function Say($m, $c = 'Gray') { Write-Host $m -ForegroundColor $c }

# Files that are IN the archive but must not be IN the MSI.
#
# The two installer files are excluded because this IS the installer: shipping
# the PowerShell installer inside the MSI would put two installers on the disk
# and leave the reader to guess which one is in charge. SHA256SUMS.txt is
# excluded because it describes the archive, and an MSI that has been installed
# is no longer that archive -- the file list is in the MSI's own File table,
# which msiexec verifies on repair.
$ExcludeFromMsi = @('SHA256SUMS.txt', 'Install-Polylinker.ps1', 'Install.cmd')

if (-not $Version) {
    $cargo = Get-Content -LiteralPath "$repo/Cargo.toml" -Raw
    if ($cargo -notmatch '(?m)^\s*version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"') {
        throw 'could not read the version from Cargo.toml'
    }
    $Version = $Matches[1]
}
# Windows Installer compares only the first three fields and caps them at
# 255.255.65535. A version outside that range silently stops upgrades working,
# which is the kind of failure that shows up one release later.
$vp = $Version.Split('.')
if ($vp.Count -ne 3) { throw "the version must be three numeric fields, not '$Version'" }
if ([int]$vp[0] -gt 255 -or [int]$vp[1] -gt 255 -or [int]$vp[2] -gt 65535) {
    throw "'$Version' cannot be expressed as an MSI ProductVersion (limits are 255.255.65535)"
}

$distFull = (Resolve-Path -LiteralPath $Dist).Path
$manifestPath = Join-Path $distFull 'SHA256SUMS.txt'
if (-not (Test-Path $manifestPath)) {
    throw "there is no SHA256SUMS.txt in $distFull. Run tools/release.ps1 first."
}
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$outFull = (Resolve-Path -LiteralPath $Out).Path
# Checked after the directory exists, because Resolve-Path throws on one that
# does not -- and the check is worth having: writing the MSI into dist/ is the
# single mistake that would corrupt the archive rather than merely fail.
if ($outFull -eq $distFull) {
    throw 'the MSI must not be written into dist/; see the comment at the top of this script'
}

# ---------------------------------------------------------------- the manifest
# Format: a header, a '--' line, then '<sha256>  <relative path>' per file.
$members = @()
$past = $false
foreach ($line in Get-Content -LiteralPath $manifestPath) {
    if (-not $past) { if ($line.Trim() -eq '--') { $past = $true }; continue }
    if ($line -match '^[0-9a-f]{64}\s\s(.+)$') { $members += $Matches[1].Trim() }
}
if (-not $members) { throw "no file lines parsed out of $manifestPath" }

$payload = $members | Where-Object { $ExcludeFromMsi -notcontains $_ } | Sort-Object
$skipped = $members | Where-Object { $ExcludeFromMsi -contains $_ }
foreach ($p in $payload) {
    if (-not (Test-Path -LiteralPath (Join-Path $distFull $p))) {
        throw "the manifest lists $p but it is not on disk in $distFull"
    }
}
Say "  manifest: $($members.Count) file(s); $($payload.Count) into the MSI, $($skipped.Count) excluded ($($skipped -join ', '))"

# --------------------------------------------------------- the payload fragment
# Ids must start with a letter or underscore and contain only [A-Za-z0-9_.]. A
# collision would silently merge two components, so the map is checked to be
# injective rather than assumed to be.
function Id-For($relative, $prefix) {
    $s = $relative -replace '[^A-Za-z0-9_.]', '_'
    "${prefix}_$s"
}
$ids = @{}
foreach ($p in $payload) {
    $id = Id-For $p 'f'
    if ($ids.ContainsKey($id)) { throw "two payload paths collapse to the same Id '$id': '$p' and '$($ids[$id])'" }
    $ids[$id] = $p
}

# Group by directory so the fragment can declare the tree.
$byDir = @{}
foreach ($p in $payload) {
    $d = Split-Path -Parent $p
    if ($null -eq $d) { $d = '' }
    if (-not $byDir.ContainsKey($d)) { $byDir[$d] = @() }
    $byDir[$d] += $p
}

$sb = [System.Text.StringBuilder]::new()
[void]$sb.AppendLine('<?xml version="1.0" encoding="utf-8"?>')
[void]$sb.AppendLine('<!--')
[void]$sb.AppendLine('  GENERATED by tools/build-msi.ps1 from dist/SHA256SUMS.txt. Do not edit, and do')
[void]$sb.AppendLine('  not commit: the whole point is that the MSI keeps no file list a human can')
[void]$sb.AppendLine('  forget to update. tools/ci.ps1 regenerates it and asserts it equals the')
[void]$sb.AppendLine('  manifest.')
[void]$sb.AppendLine('-->')
[void]$sb.AppendLine('<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">')
[void]$sb.AppendLine('  <Fragment>')
[void]$sb.AppendLine('    <ComponentGroup Id="PayloadComponents">')

foreach ($d in ($byDir.Keys | Sort-Object)) {
    $dirId = if ($d -eq '') { 'APPLICATIONFOLDER' } else { 'd_' + ($d -replace '[^A-Za-z0-9_.]', '_') }
    if ($d -ne '') {
        [void]$sb.AppendLine("      <!-- $d -->")
    }
    foreach ($p in ($byDir[$d] | Sort-Object)) {
        $fid = Id-For $p 'f'
        $src = '!(bindpath.payload)' + ($p -replace '/', '\')
        $leaf = Split-Path -Leaf $p
        # Name IS REQUIRED here, and leaving it out is not a tidiness question.
        #
        # WiX derives a File's destination name from the last path segment of
        # @Source. It resolves !(bindpath.payload) when it goes looking for the
        # bytes, but the NAME is taken from the unresolved string -- and there is
        # no separator after the closing parenthesis to split on. So the whole
        # thing becomes one segment, and the first build of this installer put
        # files on disk called
        #
        #     !(bindpath.payload)polylinker.exe
        #
        # It installed, uninstalled, registered and unregistered cleanly. Every
        # registry assertion passed. Only a check that looked for the payload BY
        # NAME on disk could have caught it, which is the one that did.
        [void]$sb.AppendLine("      <Component Id=""c$fid"" Directory=""$dirId"">")
        [void]$sb.AppendLine("        <File Id=""$fid"" Name=""$leaf"" Source=""$src"" KeyPath=""yes"" />")
        [void]$sb.AppendLine('      </Component>')
    }
}
[void]$sb.AppendLine('    </ComponentGroup>')

# The directory tree for the subdirectories the manifest happens to use. Derived,
# so a new subdirectory in the archive needs no edit here.
$subdirs = $byDir.Keys | Where-Object { $_ -ne '' } | Sort-Object
if ($subdirs) {
    [void]$sb.AppendLine('    <DirectoryRef Id="APPLICATIONFOLDER">')
    foreach ($d in $subdirs) {
        if ($d -match '[\\/]') { throw "nested subdirectory '$d' in the manifest; this generator handles one level" }
        $dirId = 'd_' + ($d -replace '[^A-Za-z0-9_.]', '_')
        [void]$sb.AppendLine("      <Directory Id=""$dirId"" Name=""$d"" />")
    }
    [void]$sb.AppendLine('    </DirectoryRef>')
}
[void]$sb.AppendLine('  </Fragment>')
[void]$sb.AppendLine('</Wix>')

$payloadWxs = Join-Path $outFull 'Payload.wxs'
Set-Content -LiteralPath $payloadWxs -Value $sb.ToString() -Encoding UTF8
Say "  generated: Payload.wxs, $($payload.Count) components, $(($subdirs | Measure-Object).Count) subdirectory(ies)"

# ------------------------------------------------------------------ the EULA
# WixUI wants RTF and the repository has none, so one is produced from the
# committed licence texts rather than a second copy of the licence being written
# by hand. Only the four RTF metacharacters need escaping; the text is ASCII.
#
# BOTH TEXTS SINCE 2026-08-06. Polylinker is offered as `MIT OR Apache-2.0`, and
# a licence page showing one of two alternatives has made the choice for the
# person clicking Accept. The MIT text goes first because it is twenty lines and
# the Apache text is four hundred: a reader who scrolls past the first screen at
# least knows there were two.
$mitSrc = if (Test-Path "$repo/LICENSE-MIT") { "$repo/LICENSE-MIT" } else { $null }
$apacheSrc = if (Test-Path "$repo/LICENSE") { "$repo/LICENSE" } elseif (Test-Path "$repo/LICENSE.txt") { "$repo/LICENSE.txt" } else { $null }
if (-not $apacheSrc) { throw 'no LICENSE or LICENSE.txt at the repository root to build the MSI licence page from' }
if (-not $mitSrc) { throw 'no LICENSE-MIT at the repository root; the MSI would offer MIT OR Apache-2.0 and show only the Apache half' }
$licenseSrcs = @($mitSrc, $apacheSrc)
$header = @(
    'Polylinker is licensed under either of the two licences below, at your',
    'option. You need comply with only one of them.',
    '',
    ('=' * 76),
    ''
) -join "`n"
$raw = $header + (($licenseSrcs | ForEach-Object {
    (Get-Content -LiteralPath $_ -Raw).TrimEnd() + "`n`n" + ('=' * 76) + "`n"
}) -join "`n")
# Written as separate statements on purpose. Chained -replace operators with
# backslash-heavy patterns parse in a way that is easy to get wrong and hard to
# read, and the first attempt at this line did get it wrong.
$esc = $raw
$esc = $esc.Replace('\', '\\')
$esc = $esc.Replace('{', '\{')
$esc = $esc.Replace('}', '\}')
$esc = $esc.Replace("`r`n", "`n")
$esc = $esc.Replace("`n", "\par`r`n")
$rtf = "{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0\fnil\fcharset0 Segoe UI;}}`r`n\viewkind4\uc1\pard\f0\fs18 $esc\par`r`n}"
$rtfPath = Join-Path $outFull 'LICENSE.rtf'
Set-Content -LiteralPath $rtfPath -Value $rtf -Encoding ASCII
Say "  generated: LICENSE.rtf from $(($licenseSrcs | ForEach-Object { Split-Path -Leaf $_ }) -join ' + ') ($((Get-Item $rtfPath).Length) bytes)"

if ($GenerateOnly) {
    Say "  -GenerateOnly: stopping before wix build" Yellow
    $global:LASTEXITCODE = 0
    return
}

# ------------------------------------------------------------------ wix build
$wix = (Get-Command wix -ErrorAction SilentlyContinue)?.Source
if (-not $wix) {
    $candidate = Join-Path $env:USERPROFILE '.dotnet\tools\wix.exe'
    if (Test-Path $candidate) { $wix = $candidate }
}
if (-not $wix) {
    throw @'
wix was not found.

The MSI is built with the WiX Toolset, which is a .NET global tool:

    dotnet tool install --global wix --version 5.0.2
    wix extension add -g WixToolset.UI.wixext/5.0.2

That needs a .NET SDK, not just the runtime. Both workflows install it on their
windows-latest runners -- ci.yml's `gate` job and release.yml's build job -- and
tools/ci.ps1 preconditions exactly one step on it, 'the MSI installs, does what
it says, uninstalls, and leaves nothing', so a machine without the SDK still
runs the other sixty-nine. That skip is fine here and not on a runner:
.github/ci-expected-skips.txt does not name that step, so a CI job that failed
to install wix goes red instead of quietly testing less.
'@
}

$msiName = "polylinker-$Version-windows-x64.msi"
$msiPath = Join-Path $outFull $msiName
$args = @(
    'build',
    '-arch', 'x64',
    '-d', "Version=$Version",
    '-bindpath', "payload=$distFull",
    '-bindpath', "msi=$outFull",
    '-ext', 'WixToolset.UI.wixext',
    '-culture', 'en-US',
    '-o', $msiPath,
    "$repo/tools/installer/Polylinker.wxs",
    $payloadWxs
)
Say "  wix build -> $msiName" Cyan
& $wix @args
if ($LASTEXITCODE -ne 0) { throw "wix build failed with exit code $LASTEXITCODE" }
if (-not (Test-Path $msiPath)) { throw "wix reported success but $msiPath is not there" }

if (-not $KeepIntermediate) {
    Remove-Item -LiteralPath $payloadWxs, $rtfPath -Force -ErrorAction SilentlyContinue
    Get-ChildItem $outFull -Filter '*.wixpdb' | Remove-Item -Force -ErrorAction SilentlyContinue
}

$size = (Get-Item $msiPath).Length
$sha = (Get-FileHash -LiteralPath $msiPath -Algorithm SHA256).Hash.ToLower()
Set-Content -LiteralPath "$msiPath.sha256" -Value "$sha  $msiName" -Encoding ASCII -NoNewline
Say ("  {0}  {1:N0} bytes" -f $msiName, $size) Green
Say "  $sha"
$global:LASTEXITCODE = 0
