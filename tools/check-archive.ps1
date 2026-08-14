<#
.SYNOPSIS
    Verify a release archive from the outside, the way a downloader would.

.DESCRIPTION
    `tools/release.ps1` asserts a great deal about `dist/`. This asserts things
    about the ARCHIVE, which is a different object and the only one anybody
    actually receives. The distinction is not academic: a zip is built from a
    list, and the licence texts have gone missing from a packaging step in this
    repository once already.

    What it checks:

      * exactly one top-level directory, named for the version and platform,
        and the manifest's own `platform:` line agrees with that name
      * SHA256SUMS.txt is inside, and EVERY file it lists is in the archive with
        the recorded hash -- computed from the archive's own bytes, not from
        whatever happens to be on disk beside it
      * nothing is in the archive that the manifest does not list
      * the licence set: eight texts under licences/, plus NOTICE.txt,
        LICENSE.txt, LICENSE-MIT.txt and features/NOTICE.txt by name -- both
        halves of `MIT OR Apache-2.0`, not whichever one the packager reached
        for first
      * entries are sorted and timestamps are pinned, so the bytes are a
        function of the contents
      * on a tar, that the three programs and the extension module carry mode
        0755 and that no build machine's uid, gid or username is recorded

    ONE FILE MAY BE MISSING, ON ONE PLATFORM, AND ONLY IF THE MANIFEST SAYS SO.
    See `$WaivableOmissions` below. Nothing here is skipped quietly: the waiver
    has to be declared in the manifest header, it is refused on every platform
    but the one it names, it is refused if the file turns up in the archive
    anyway, and it is printed on the way out whether the archive passes or
    fails. A reader of the output can always tell which of the two shapes the
    archive has.

    It reads both containers itself rather than shelling out to `tar` or
    `Expand-Archive`, because it has to run identically on three runners and on
    a Windows machine with no `tar` guaranteed -- and because a checker that
    unpacks to disk first is checking the disk again.

.PARAMETER Archive
    The .zip or .tar.gz to check.

.PARAMETER MinimumFiles
    Floor on the number of files the manifest lists. Set equality with the
    archive is checked regardless; this exists because an archive of nothing
    agrees perfectly with a manifest of nothing.

    Left at 0 the floor is DERIVED FROM THE PLATFORM in the archive's name,
    because the platforms legitimately ship different numbers of files. The
    fixed 19 that used to sit here was measured on Windows and failed both
    other legs of the first release: tools/release.ps1 ships four
    Windows-only files -- Install-Polylinker.ps1, Install.cmd,
    README-WINDOWS.txt and polylinker.ico -- and says at its installer block
    that they are deliberately not ported. A Linux archive carrying all three
    binaries, the Python extension and all eight font licences was rejected
    for being four files short of Windows.

    The floor is the weaker half of the check. `RequiredMembers` below is the
    half that matters: a count cannot tell a missing binary from a missing
    licence, and lowering a count to make a red gate green is how a check
    stops being one.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Archive,
    [int]$MinimumFiles = 0
)

$ErrorActionPreference = 'Stop'
$fail = @()
function Bad($msg) { $script:fail += $msg }

# WHAT AN ARCHIVE IS ALLOWED TO BE SHORT OF, AND WHERE.
#
# The table is deliberately one row long, and the row is deliberately keyed on
# the platform rather than on the file. An entry here is not "this file is
# optional"; it is "this exact file, on this exact platform, may be absent IF
# the manifest declares the absence and gives a reason that names the platform".
# Every other file on every platform is required as it always was, and the same
# file on windows-x64 is still a hard failure.
#
# `polylinker.pyd` on `windows-arm64` is the row, and the reason it exists is a
# question nobody could answer when ARM64 support was written: `crates/pl-py`
# links against CPython through pyo3, so the extension module needs an ARM64
# CPython on the build machine to link against, and whether GitHub's
# `windows-11-arm` image has one WAS NOT ESTABLISHED. It could not be -- the
# ARM64 MSVC linker is not installed on the machine this was written on, so no
# ARM64 binary of any kind had ever been produced there. Both answers therefore
# had to be handled: if the runner can build it, no waiver is declared and this
# table is never consulted; if it cannot, the archive is one file short and says
# so in the manifest a downloader already reads.
#
# THE THIRD OUTCOME IS THE ONE THIS SHAPE EXISTS TO PREVENT. Making the
# requirement conditional on the platform alone -- `if not arm64, skip` -- would
# have produced a check that passes over an ARM64 archive without looking at
# anything, which is the defect this project has now shipped fixes for nine
# times. A waiver that has to be written down, that is refused when it is
# written down wrongly, and that is printed either way, can still fail. A silent
# `if` cannot.
$WaivableOmissions = @{ 'polylinker.pyd' = @('windows-arm64') }

# Filled from the manifest header if it declares any. Initialised here rather
# than inside the manifest block below, because the tar section reads it and an
# archive with no manifest at all must not turn a reported failure into a
# property lookup on $null.
$omitted = @{}
$rootPlatform = ''

if (-not (Test-Path -LiteralPath $Archive)) { throw "no such archive: $Archive" }
$Archive = (Resolve-Path -LiteralPath $Archive).Path

# ---------------------------------------------------------------------------
# Read the container into { Name -> bytes }, plus the metadata each format
# carries. One shape out, so everything below is written once.
# ---------------------------------------------------------------------------
$members = [System.Collections.Generic.List[object]]::new()
$isTar = $Archive.EndsWith('.tar.gz')

if ($isTar) {
    # ustar, as written by release.ps1. Header block, payload padded to 512,
    # two zero blocks at the end.
    $gzBytes = [System.IO.File]::ReadAllBytes($Archive)
    $inMs = [System.IO.MemoryStream]::new($gzBytes)
    $outMs = [System.IO.MemoryStream]::new()
    $gz = [System.IO.Compression.GZipStream]::new($inMs, [System.IO.Compression.CompressionMode]::Decompress)
    try { $gz.CopyTo($outMs) } finally { $gz.Dispose(); $inMs.Dispose() }
    $tar = $outMs.ToArray()
    $outMs.Dispose()

    if ($tar.Length % 512 -ne 0) { Bad "the tar is $($tar.Length) bytes, which is not a whole number of 512-byte blocks" }

    $str = {
        param($off, $len)
        $s = [System.Text.Encoding]::ASCII.GetString($tar, $off, $len)
        $z = $s.IndexOf([char]0)
        if ($z -ge 0) { $s = $s.Substring(0, $z) }
        $s.Trim()
    }
    $oct = { param($off, $len) $t = & $str $off $len; if ($t) { [Convert]::ToInt64($t, 8) } else { 0 } }

    $pos = 0
    while ($pos + 512 -le $tar.Length) {
        # A header of NULs is the end-of-archive marker.
        $allZero = $true
        for ($i = 0; $i -lt 512; $i++) { if ($tar[$pos + $i] -ne 0) { $allZero = $false; break } }
        if ($allZero) { break }

        if ((& $str ($pos + 257) 6) -ne 'ustar') { Bad "the header at offset $pos has no ustar magic" ; break }

        # The checksum, recomputed. This is the field that cannot be inferred
        # from a hex dump: the sum is taken with the checksum field itself read
        # as eight spaces.
        $recorded = & $oct ($pos + 148) 8
        $sum = 0
        for ($i = 0; $i -lt 512; $i++) {
            $sum += if ($i -ge 148 -and $i -lt 156) { 32 } else { $tar[$pos + $i] }
        }
        if ($sum -ne $recorded) { Bad "the header at offset $pos has checksum $recorded but sums to $sum" }

        $name = & $str $pos 100
        $size = & $oct ($pos + 124) 12
        $obj = [pscustomobject]@{
            Name  = $name
            Size  = $size
            Mode  = ([Convert]::ToString((& $oct ($pos + 100) 8), 8)).PadLeft(3, '0')
            Uid   = & $oct ($pos + 108) 8
            Gid   = & $oct ($pos + 116) 8
            Uname = & $str ($pos + 265) 32
            Gname = & $str ($pos + 297) 32
            Mtime = & $oct ($pos + 136) 12
            Type  = [char]$tar[$pos + 156]
            Bytes = $null
        }
        $pos += 512
        if ($obj.Type -eq '0') {
            $obj.Bytes = $tar[$pos..($pos + $size - 1)]
            $pos += [int](([Math]::Ceiling($size / 512.0)) * 512)
        }
        $members.Add($obj)
    }
} else {
    Add-Type -AssemblyName System.IO.Compression.FileSystem | Out-Null
    $zip = [System.IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        foreach ($e in $zip.Entries) {
            $ms = [System.IO.MemoryStream]::new()
            $s = $e.Open()
            try { $s.CopyTo($ms) } finally { $s.Dispose() }
            $members.Add([pscustomobject]@{
                Name  = $e.FullName
                Size  = $e.Length
                Mode  = $null
                Uid   = 0; Gid = 0; Uname = ''; Gname = ''
                # A zip stores an MS-DOS wall clock with no timezone, so this is
                # read as a wall clock. Comparing the instant fails everywhere
                # except UTC, which would be a bug in the checker.
                Mtime = $e.LastWriteTime.DateTime
                Type  = if ($e.FullName.EndsWith('/')) { '5' } else { '0' }
                Bytes = $ms.ToArray()
            })
            $ms.Dispose()
        }
    } finally { $zip.Dispose() }
}

$files = @($members | Where-Object { $_.Type -eq '0' })
if (-not $files) { throw "$Archive contains no files at all" }

# ---------------------------------------------------------------------------
# One root, named for the version and platform.
# ---------------------------------------------------------------------------
$roots = @($members | ForEach-Object { ($_.Name -split '/')[0] } | Sort-Object -Unique)
if ($roots.Count -ne 1) { Bad "the archive has $($roots.Count) top-level entries: $($roots -join ', ')" }
$root = $roots[0]
$expectedRoot = [System.IO.Path]::GetFileName($Archive) -replace '\.tar\.gz$|\.zip$', ''
if ($root -ne $expectedRoot) { Bad "the archive is named for '$expectedRoot' but unpacks into '$root'" }
# CAPTURED, not merely matched. The platform label is what decides which files
# are required, which read-me is banned, and whether an omission is waivable at
# all, and it is read from the directory the archive actually unpacks into --
# the string the recipient sees. It is compared with the manifest's own
# `platform:` line below, so a mislabelled archive is caught by the two halves
# disagreeing rather than by both being wrong in the same direction.
if ($root -match '^polylinker-\d+\.\d+\.\d+-((windows|linux|macos)-\S+)$') {
    $rootPlatform = $Matches[1]
} else {
    Bad "'$root' is not a <name>-<version>-<platform> directory"
}
# For messages. Never empty, so a complaint about a malformed root still names
# something the reader can find on disk.
$platformName = if ($rootPlatform) { $rootPlatform } else { [System.IO.Path]::GetFileName($Archive) }

$rel = @{}
foreach ($f in $files) {
    if (-not $f.Name.StartsWith("$root/")) { Bad "$($f.Name) is outside the root directory"; continue }
    $rel[$f.Name.Substring($root.Length + 1)] = $f
}

# ---------------------------------------------------------------------------
# THE MANIFEST, CHECKED AGAINST THE ARCHIVE'S OWN BYTES.
#
# This is the check the whole script exists for. `tools/ci.ps1` verifies the
# manifest against `dist/`; this verifies it against what was actually packed,
# which is the only thing a downloader has. Between the two lies the packaging
# step, and the packaging step is where the licences went missing.
# ---------------------------------------------------------------------------
if (-not $rel.ContainsKey('SHA256SUMS.txt')) {
    Bad 'the archive has no SHA256SUMS.txt, so nothing in it can be verified'
} else {
    $mb = $rel['SHA256SUMS.txt'].Bytes
    if ($mb[0] -eq 0xEF) { Bad 'the manifest has a BOM and will not verify with sha256sum' }
    if ($mb -contains 0x0D) { Bad 'the manifest has CRLF and will not verify with sha256sum' }
    foreach ($b in $mb) { if ($b -gt 0x7F) { Bad 'the manifest is not pure ASCII'; break } }

    $lines = [System.Text.Encoding]::UTF8.GetString($mb) -split "`n"
    $sep = [Array]::IndexOf($lines, '--')
    if ($sep -lt 4) { Bad 'the manifest has no header' }
    foreach ($k in 'version:', 'platform:', 'commit:', 'rustc:') {
        if (-not ($lines[0..$sep] | Where-Object { $_.StartsWith($k) })) { Bad "the manifest header has no '$k' line" }
    }

    $header = @($lines[0..$sep])

    # THE TWO PLACES THE PLATFORM IS WRITTEN MUST AGREE.
    #
    # `tools/release.ps1` puts one label into the archive's name and the same
    # label into this header, from one variable, so they cannot differ by
    # accident today. They can differ by EDIT -- a file renamed by hand, a
    # manifest patched, an archive repacked -- and the two readers that act on
    # the platform read different ones: this script takes it from the directory
    # name, and anything parsing the manifest takes it from here. Two sources
    # for one fact, unchecked, is the shape every drift in this repository has
    # had.
    $manifestPlatform = ''
    foreach ($l in $header) { if ($l -match '^platform:\s*(\S+)\s*$') { $manifestPlatform = $Matches[1] } }
    if ($manifestPlatform -and $rootPlatform -and $manifestPlatform -ne $rootPlatform) {
        Bad ("the archive unpacks into a directory named for '$rootPlatform' but its manifest says " +
             "'platform: $manifestPlatform'. One of the two is a lie to whoever downloaded this, and " +
             "pl update selects a download by exactly this string.")
    }

    # THE DECLARED OMISSIONS. `omitted: <file> -- <reason>`, one per line.
    #
    # A malformed line is a failure, not an ignored line. The alternative is a
    # waiver that silently does not parse: the file would then be reported
    # missing with no explanation, which is at least loud -- but a typo in the
    # other direction, a line this reader half-understood, would be worse. So
    # anything starting `omitted:` either parses or fails here.
    foreach ($l in $header) {
        if ($l -match '^omitted:\s*(\S+)\s+--\s+(\S.*?)\s*$') {
            $omitted[$Matches[1]] = $Matches[2]
        } elseif ($l -match '^omitted:') {
            Bad "the manifest has an 'omitted:' line this checker cannot read: '$l'. The form is 'omitted: <file> -- <reason>'."
        }
    }

    $sha = [System.Security.Cryptography.SHA256]::Create()
    $listed = @()
    foreach ($line in $lines[($sep + 1)..($lines.Length - 1)]) {
        if (-not $line) { continue }
        if ($line -notmatch '^[0-9a-f]{64}  \S+$') { Bad "not a checksum line: $line"; continue }
        $parts = $line -split '  ', 2
        $listed += $parts[1]
        if (-not $rel.ContainsKey($parts[1])) {
            Bad "the manifest lists $($parts[1]), which is NOT IN THE ARCHIVE"
            continue
        }
        $got = ($sha.ComputeHash($rel[$parts[1]].Bytes) | ForEach-Object { $_.ToString('x2') }) -join ''
        if ($got -ne $parts[0]) { Bad "$($parts[1]) in the archive does not match its recorded hash" }
    }
    $sha.Dispose()

    # The converse, so it is set equality rather than a one-way inclusion. The
    # manifest cannot list itself.
    foreach ($k in $rel.Keys) {
        if ($k -ne 'SHA256SUMS.txt' -and $listed -notcontains $k) {
            Bad "$k is in the archive but not in the manifest, so it has no integrity record"
        }
    }
    # WHAT THE ARCHIVE MUST CONTAIN, BY NAME. The platform comes from the
    # archive's own file name, which tools/release.ps1 builds from the label it
    # was given, so a mislabelled archive is checked against the wrong set and
    # says so loudly rather than passing quietly.
    $isWinArchive = $Archive -match 'windows'
    $isMacArchive = $Archive -match 'macos'
    $x = if ($isWinArchive) { '.exe' } else { '' }

    $required = @(
        # The three programs, and the Python extension module. CPython loads it
        # as .pyd on Windows and .so everywhere else -- including macOS, where
        # cargo emits a .dylib and the name must still be .so.
        "pl$x", "polylinker$x", "pl-mcp$x"
        if ($isWinArchive) { 'polylinker.pyd' } else { 'polylinker.so' }

        # Licensing. These are an obligation, not a courtesy: the GUI embeds
        # ten font files and cannot be redistributed without their texts.
        # LICENSE.txt is Apache-2.0 and LICENSE-MIT.txt is the other half of
        # the `MIT OR Apache-2.0` the manifest offers. Both, because an archive
        # carrying one of two alternatives is an archive that made the choice
        # for the recipient.
        'LICENSE.txt', 'LICENSE-MIT.txt', 'NOTICE.txt', 'TRADEMARKS.md', 'features/NOTICE.txt'
        'licences/Hack-MIT-and-BitstreamVera.txt', 'licences/IBMPlex-OFL.txt'
        'licences/Inter-OFL.txt'
        'licences/Liberation-OFL.txt', 'licences/NotoEmoji-OFL.txt'
        'licences/Phosphor-MIT.txt', 'licences/Ubuntu-UFL.txt'
        'licences/emoji-icon-font-MIT.txt'

        # The read-me that tells this platform's user how to get past its own
        # quarantine or SmartScreen prompt. Shipping the wrong one is worse than
        # shipping none, so exactly one is required and the others are banned.
        if ($isWinArchive) { 'README-WINDOWS.txt' }
        elseif ($isMacArchive) { 'README-MACOS.txt' } else { 'README-LINUX.txt' }

        # Windows alone gets an installer. release.ps1 says at its installer
        # block that this is deliberate and not an oversight.
        if ($isWinArchive) { 'Install-Polylinker.ps1', 'Install.cmd', 'polylinker.ico' }
    )
    # THE REQUIREMENT IS UNCHANGED. A file that is required and absent is a
    # failure unless the manifest declared its absence, and the declaration is
    # then judged on its own terms in the block below -- so a bogus declaration
    # does not buy the archive anything, it just moves where it fails.
    foreach ($r in $required) {
        if ($listed -contains $r) { continue }
        if ($omitted.ContainsKey($r)) { continue }
        Bad "$r is required in a $platformName archive and is not in the manifest"
    }

    # EVERY DECLARED OMISSION, HELD TO $WaivableOmissions.
    #
    # Five ways this can fail, which is the point of writing it out rather than
    # skipping a requirement behind an `if`: a file nothing may ever omit, the
    # right file on the wrong platform, a declaration contradicted by the
    # archive's own contents, a declaration of something this platform never
    # required in the first place, and a reason that does not name the platform
    # it is excusing. The last one is not pedantry -- a reason that does not say
    # which platform is short of the file is a sentence a reader cannot act on,
    # and `tools/release.ps1` refuses to write one for the same reason.
    $waived = @()
    foreach ($name in @($omitted.Keys)) {
        $reason = $omitted[$name]
        if (-not $WaivableOmissions.ContainsKey($name)) {
            Bad ("the manifest declares '$name' omitted. No release may omit that file on any " +
                 "platform; only $(($WaivableOmissions.Keys | Sort-Object) -join ', ') has a waiver at all.")
            continue
        }
        $where = $WaivableOmissions[$name]
        if ($where -notcontains $rootPlatform) {
            Bad ("the manifest declares '$name' omitted, and that is waivable only on " +
                 "$($where -join ', ') -- not on '$platformName'.")
            continue
        }
        if ($listed -contains $name) {
            Bad ("the manifest declares '$name' omitted and also lists it. One of the two is wrong, " +
                 "and an archive that misdescribes its own contents cannot be checked against itself.")
            continue
        }
        if ($required -notcontains $name) {
            Bad ("the manifest declares '$name' omitted, but a $platformName archive never required " +
                 "it, so the declaration excuses nothing and describes nothing.")
            continue
        }
        if ($reason -notmatch [regex]::Escape($rootPlatform)) {
            Bad ("the manifest's reason for omitting '$name' does not name the platform it applies " +
                 "to: '$reason' does not contain '$rootPlatform'.")
            continue
        }
        $waived += [pscustomobject]@{ Name = $name; Reason = $reason }
    }
    # And the read-mes for the other platforms must NOT be here.
    foreach ($w in 'README-WINDOWS.txt', 'README-MACOS.txt', 'README-LINUX.txt') {
        if ($required -notcontains $w -and $listed -contains $w) {
            Bad "$w is in a $(Split-Path -Leaf $Archive) archive, which will tell the reader to do the wrong thing"
        }
    }

    # The floor, derived when the caller did not set one. See the comment on the
    # parameter: this is the weak half, kept only because an empty archive
    # agrees perfectly with an empty manifest.
    #
    # MINUS THE WAIVED FILES, and only the ones that got past every test above.
    # Without the subtraction a legitimately waived archive fails the floor as
    # well, which would report a count when the cause is a declared omission --
    # and the temptation then is to lower the number by hand, which is exactly
    # how a floor stops being one. `$waived` cannot be inflated to buy slack: a
    # declaration that is not in $WaivableOmissions, is on the wrong platform,
    # is contradicted by the archive, or has a reason that does not name the
    # platform never reaches it.
    $floor = if ($MinimumFiles -gt 0) { $MinimumFiles } else { $required.Count - $waived.Count }
    if ($listed.Count -lt $floor) {
        Bad "the manifest lists $($listed.Count) file(s); at least $floor are expected"
    }

    # THE LICENCE OBLIGATION, at the last point it can be dropped.
    #
    # SIL OFL 1.1 clause 2, the Bitstream Vera licence reached through Hack, and
    # MIT for emoji-icon-font and for Phosphor all require their text to
    # accompany every copy. An archive is a copy. Counted rather than enumerated
    # -- a list here would be a fourth copy of release.ps1's `$notices` and would
    # drift from it exactly as dist/ did, twice, in August 2026.
    #
    # Eight since 2026-08-09, with Inter's OFL for the heading face.
    $lic = @($listed | Where-Object { $_ -like 'licences/*' })
    if ($lic.Count -lt 8) { Bad "only $($lic.Count) font licence text(s) in the archive; NOTICE requires 8" }
    foreach ($required in 'NOTICE.txt', 'LICENSE.txt', 'LICENSE-MIT.txt', 'features/NOTICE.txt') {
        if ($listed -notcontains $required) { Bad "$required is not in the archive; shipping without it is a licence violation" }
    }
}

# ---------------------------------------------------------------------------
# The archive is a function of its contents, not of the day it was built.
# ---------------------------------------------------------------------------
$names = @($members | ForEach-Object { $_.Name })
$sorted = @($names | Sort-Object)
for ($i = 0; $i -lt $names.Count; $i++) {
    if ($names[$i] -ne $sorted[$i]) { Bad "entries are not in sorted order (first difference: $($names[$i]))"; break }
}
$pinned = if ($isTar) { 946684800 } else { [DateTime]::new(2000, 1, 1, 0, 0, 0) }
foreach ($m in $members) {
    if ($m.Mtime -ne $pinned) { Bad "$($m.Name) carries a live timestamp ($($m.Mtime)); the archive hash would change on every build" }
}

# ---------------------------------------------------------------------------
# Tar only: the executable bit, and no trace of who built it.
# ---------------------------------------------------------------------------
if ($isTar) {
    foreach ($m in $members) {
        if ($m.Uid -ne 0 -or $m.Gid -ne 0) { Bad "$($m.Name) records uid $($m.Uid)/gid $($m.Gid) from the build machine" }
        if ($m.Uname -or $m.Gname) { Bad "$($m.Name) records the build account '$($m.Uname)/$($m.Gname)'" }
    }
    # The programs. Named by stem, because the tar may have been produced on
    # Windows by the gate, where they carry .exe.
    $progs = @($rel.Keys | Where-Object { $_ -match '^(pl|pl-mcp|polylinker)(\.exe)?$|^polylinker\.(so|pyd)$' })
    # FOUR, MINUS ANY EXTENSION MODULE THE MANIFEST DECLARED ABSENT. The three
    # executables are never waivable, so this number can only ever be 4 or 3,
    # and it is 3 only for an archive that said in its own header why. This is
    # reachable on Windows: `tools/ci.ps1` runs `release.ps1 -ArchiveFormat
    # tar.gz` on the gate machine on purpose, so a Windows tar is checked here
    # even though no Windows release ships one.
    $wantProgs = 4 - @($omitted.Keys | Where-Object { $_ -eq 'polylinker.pyd' -or $_ -eq 'polylinker.so' }).Count
    if ($progs.Count -lt $wantProgs) { Bad "found $($progs.Count) program(s) in the archive; expected $wantProgs" }
    foreach ($p in $progs) {
        if ($rel[$p].Mode -ne '755') {
            Bad "$p has mode 0$($rel[$p].Mode); it would extract without the executable bit and the user would have to chmod it"
        }
    }
    foreach ($k in $rel.Keys) {
        if ($progs -notcontains $k -and $rel[$k].Mode -ne '644') { Bad "$k has mode 0$($rel[$k].Mode), expected 0644" }
    }
}

# WHAT WAS DECLARED ABSENT, PRINTED BEFORE THE VERDICT AND REGARDLESS OF IT.
#
# Above the FAIL/ok split on purpose. An archive that is short of a file is a
# fact about the download whether or not the rest of it checked out, and it is
# printed even when the run fails so that a reader debugging a red gate is not
# looking at a smaller archive with no explanation for why.
foreach ($w in $omitted.Keys) {
    Write-Host ("NOTE  {0} is NOT IN {1}. Declared reason: {2}" -f
        $w, [System.IO.Path]::GetFileName($Archive), $omitted[$w]) -ForegroundColor Yellow
}

if ($fail) {
    Write-Host "FAIL  $([System.IO.Path]::GetFileName($Archive))" -ForegroundColor Red
    $fail | ForEach-Object { Write-Host "      $_" -ForegroundColor Red }
    exit 1
}
# The extension module is named in the success line, not merely absent from the
# failures. "It did not complain" and "it looked and found it" read the same in
# a log, and this is the one member of the required set that a platform is now
# allowed to be without -- so the line says which of the two happened rather
# than leaving a reader to infer it from a file count.
$pyState = if ($omitted.ContainsKey('polylinker.pyd') -or $omitted.ContainsKey('polylinker.so')) {
    'the Python extension module ABSENT BY DECLARATION'
} else {
    'the Python extension module present'
}
Write-Host ("ok    {0} [{1}]: {2} file(s), {3} licence text(s), {4}, manifest verified against the archive's own bytes" -f
    [System.IO.Path]::GetFileName($Archive), $platformName, $rel.Count,
    @($rel.Keys | Where-Object { $_ -like 'licences/*' }).Count, $pyState) -ForegroundColor Green
exit 0
