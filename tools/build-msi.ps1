<#
.SYNOPSIS
    Build polylinker-<version>-<platform>.msi from a dist/ that tools/release.ps1
    has already produced.

    The platform is not a parameter and not this machine's. It is read out of
    dist/SHA256SUMS.txt, which is the same file the payload comes from -- so the
    installer is targeted at the architecture of the binaries it contains, by
    construction, and cannot be aimed somewhere else by an argument. See 'the
    architecture' below.

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

# NORMALISED, not `Resolve-Path`. These two strings are compared for equality
# below, and `Resolve-Path` returns whatever spelling it was handed: given
# `-Dist C:\PROGRA~1\d -Out "C:\Program Files\d"` it produced two different
# strings for one directory, and the guard that stops the MSI being written into
# dist/ passed while pointing both at the same place. Measured on this machine:
# `Resolve-Path 'C:\PROGRA~1'` is `C:\PROGRA~1`, `[IO.Path]::GetFullPath` of the
# same is `C:\Program Files`. This is the same normaliser mismatch that broke
# `tools/release.ps1`'s manifest on CI run 31325886841; see the long note on
# `Get-DirectoryPrefix` there. `GetUnresolvedProviderPathFromPSPath` first,
# because `GetFullPath` alone would resolve a relative `-Dist dist` against the
# .NET current directory rather than the PowerShell location.
function Get-NormalPath([string]$Path) {
    $abs = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
    return [System.IO.Path]::GetFullPath($abs).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
}

$distFull = Get-NormalPath $Dist
$manifestPath = Join-Path $distFull 'SHA256SUMS.txt'
if (-not (Test-Path $manifestPath)) {
    throw "there is no SHA256SUMS.txt in $distFull. Run tools/release.ps1 first."
}
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$outFull = Get-NormalPath $Out
# The check is worth having: writing the MSI into dist/ is the single mistake
# that would corrupt the archive rather than merely fail. Compared case-
# insensitively because Windows paths are, and the two spellings can differ in
# case without differing in meaning.
if ($outFull.Equals($distFull, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'the MSI must not be written into dist/; see the comment at the top of this script'
}

# ---------------------------------------------------------------- the manifest
# Format: a header, a '--' line, then '<sha256>  <relative path>' per file.
#
# THE HEADER IS READ NOW TOO, for the `platform:` line. It used to be skipped
# wholesale, which was fine while there was one Windows architecture and the
# only thing this script needed from dist/ was the file list.
$members = @()
$platform = ''
$past = $false
foreach ($line in Get-Content -LiteralPath $manifestPath) {
    if (-not $past) {
        if ($line.Trim() -eq '--') { $past = $true }
        elseif ($line -match '^platform:\s*(\S+)\s*$') { $platform = $Matches[1] }
        continue
    }
    if ($line -match '^[0-9a-f]{64}\s\s(.+)$') { $members += $Matches[1].Trim() }
}
if (-not $members) { throw "no file lines parsed out of $manifestPath" }
if (-not $platform) {
    throw ("$manifestPath has no 'platform:' line, so there is nothing here that says which " +
           "architecture these binaries are. tools/release.ps1 has written that line since the first " +
           "three-platform release; a dist/ without it did not come from it.")
}

# --------------------------------------------------------------- the architecture
#
# WHY THIS IS NOT `x64` ANY MORE, AND WHY IT IS NOT A PARAMETER EITHER.
#
# `-arch x64` was hard-coded here from the day the MSI was authored, which was
# correct for exactly as long as `windows-x64` was the only Windows artifact.
# `-arch` is not cosmetic: WiX writes it into the package's Template summary
# property, and that value is what decides how `ProgramFiles6432Folder`
# resolves and whether Windows Installer will accept the package on the machine
# at all. An ARM64 payload in a package that says x64 is the failure mode this
# whole block exists to make impossible -- it would install into the wrong
# Program Files where it installed at all, and both outcomes look like a
# working build right up until somebody double-clicks it.
#
# It comes from the manifest rather than from a parameter or from this machine.
# A parameter is a second place to state a fact that dist/ already states, and
# this project's whole MSI design is "one file list, read twice, never copied";
# the architecture is part of that same fact. `RuntimeInformation` is worse
# still: the machine that BUILDS the MSI need not be the machine the payload
# targets -- `wix` is a .NET tool that runs anywhere -- so asking the host would
# be asking something that does not know.
#
# The manifest's label is trustworthy because `tools/release.ps1` no longer
# merely asserts it: since ARM64 was added it reads the COFF machine field out
# of every binary it ships and refuses to write a `platform:` line the bytes
# disagree with. So the chain is bytes -> label -> manifest -> this table ->
# `-arch`, and every link but the last is checked upstream. The last link is
# checked after the build, below.
#
# An unknown label is a THROW and not a default. Defaulting to x64 here is
# precisely the bug this replaces, one indirection further back.
$WixArchOfPlatform = @{ 'windows-x64' = 'x64'; 'windows-arm64' = 'arm64' }
if (-not $WixArchOfPlatform.ContainsKey($platform)) {
    throw ("this dist/ says 'platform: $platform', and an MSI is a Windows Installer package -- the " +
           "only platforms it can be built for are $(($WixArchOfPlatform.Keys | Sort-Object) -join ' and '). " +
           "If a new Windows architecture is being added, it needs an entry here, an arm in " +
           "crates/pl-update's PLATFORM_ARTIFACT cascade, and a published file to go with it; an arm " +
           "without a file turns pl update's clean decline into a 404.")
}
$wixArch = $WixArchOfPlatform[$platform]
# Said here and not only at the wix invocation, so that `-GenerateOnly` -- the
# mode a machine with no .NET SDK runs, which is the machine this project is
# developed on -- still reports which architecture this dist/ resolved to.
Say "  platform: $platform (from the manifest header) -> wix -arch $wixArch"

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
runs the other seventy-six. That skip is fine here and not on a runner: without
wix that precondition returns $false, which is a skip with no declared reason,
and under -ExpectedSkips a skip with no declared reason FAILS. So a CI job that
failed to install wix goes red instead of quietly testing less. (Off Windows the
same step declares 'not windows' and skips legitimately, which is a different
thing and is checked against $IsWindows rather than believed.)
'@
}

$msiName = "polylinker-$Version-$platform.msi"
$msiPath = Join-Path $outFull $msiName
$args = @(
    'build',
    '-arch', $wixArch,
    '-d', "Version=$Version",
    '-bindpath', "payload=$distFull",
    '-bindpath', "msi=$outFull",
    '-ext', 'WixToolset.UI.wixext',
    '-culture', 'en-US',
    '-o', $msiPath,
    "$repo/tools/installer/Polylinker.wxs",
    $payloadWxs
)
Say "  wix build -> $msiName (-arch $wixArch, from 'platform: $platform')" Cyan
& $wix @args
if ($LASTEXITCODE -ne 0) { throw "wix build failed with exit code $LASTEXITCODE" }
if (-not (Test-Path $msiPath)) { throw "wix reported success but $msiPath is not there" }

# ------------------------------------------------- did -arch actually land?
#
# THE ARGUMENT WAS PASSED IS NOT THE SAME CLAIM AS THE PACKAGE IS TARGETED.
# Everything above proves this script computed `arm64` and put it in an array.
# It proves nothing about the .msi on disk, and the gap between those two is
# where the interesting failure lives: a flag WiX quietly ignores, a flag whose
# spelling changed between toolset versions, an `-arch` overridden by something
# in the .wxs. Every one of those produces a package that builds cleanly,
# installs on the developer's x64 machine, passes the install/uninstall oracle
# there, and is wrong. A check that stopped at "we passed the flag" would be a
# check that cannot fail.
#
# So the produced file is read back. An MSI records its target in the Template
# summary property, as `<platform>;<language codes>` -- `x64;1033`,
# `Arm64;1033` -- and that property is precisely what `-arch` sets. It is the
# one field that answers the question, so it is the one field this reads.
#
# READ BY SCANNING THE BYTES, not through the WindowsInstaller COM automation.
# Two reasons, and the second is the one that decided it. Firstly this script is
# already written to run where `wix` runs rather than only where `msiexec` does
# -- `-GenerateOnly` exists so a machine with no .NET SDK still gets the file
# set checked -- and COM would tie a step to Windows that need not be. Secondly
# the summary information stream is stored in the compound file uncompressed,
# with its strings as codepage bytes, so the value appears literally: no parser,
# no interop, nothing between the assertion and the file. The .cab of binaries
# alongside it is compressed noise, and the chance of a four-to-seven character
# platform token followed by `;` and digits appearing in it by accident is
# small enough to prefer over an automation dependency -- and if it ever did,
# the two-candidate case below refuses rather than picking one.
#
# THREE WAYS THIS FAILS, all of them wanted:
#   * no candidate at all -- the reader found nothing, so it proved nothing, and
#     an unverified package is not accepted;
#   * more than one distinct candidate -- ambiguous evidence is not evidence;
#   * a candidate naming an architecture other than the one asked for.
# The comparison is case-insensitive because the spelling is WiX's to choose
# (`Arm64` today) and this is checking WHICH ARCHITECTURE, not which capital.
$msiBytes = [System.IO.File]::ReadAllBytes($msiPath)
# Latin-1 maps every byte to the character of the same value, so the search is
# over the bytes and not over a decoding that could drop or merge any of them.
# UTF-16LE as well, purely so that a future toolset writing the summary stream
# as wide characters is found rather than reported as absent; ASCII bytes cannot
# decode to the pattern under UTF-16 and wide bytes cannot under Latin-1, so the
# union cannot double-count one real value.
$rx = '(?<![A-Za-z0-9])(Intel64|Arm64|Intel|x64|Arm);([0-9]+)(?![0-9])'
$candidates = @()
foreach ($enc in @([System.Text.Encoding]::GetEncoding(28591), [System.Text.Encoding]::Unicode)) {
    foreach ($m in [regex]::Matches($enc.GetString($msiBytes), $rx)) { $candidates += $m.Value }
}
$candidates = @($candidates | Sort-Object -Unique)
if ($candidates.Count -eq 0) {
    throw ("$msiName was built, but no Template summary value could be found anywhere in it, so " +
           "nothing here established that -arch $wixArch reached WiX. An MSI always carries one. " +
           "Refusing the package rather than reporting a check that looked and saw nothing.")
}
if ($candidates.Count -gt 1) {
    throw ("$msiName contains $($candidates.Count) candidate Template summary values -- " +
           "$($candidates -join ', ') -- and this check will not choose between them. One of them is " +
           "the package's real target and the rest are coincidence in the compressed payload; either " +
           "way the evidence is ambiguous and the package is not accepted on it.")
}
$templatePlatform = ($candidates[0] -split ';')[0]
if (-not $templatePlatform.Equals($wixArch, [StringComparison]::OrdinalIgnoreCase)) {
    throw ("$msiName was built with -arch $wixArch but its Template summary property says " +
           "'$($candidates[0])', so the package targets $templatePlatform. The payload in $distFull is " +
           "$platform. A package whose target disagrees with its payload installs into the wrong " +
           "Program Files where Windows Installer accepts it at all, and it does both silently.")
}
Say "  Template summary: $($candidates[0]) -- -arch $wixArch reached WiX" Green

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
