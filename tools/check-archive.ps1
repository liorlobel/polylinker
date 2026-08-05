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

      * exactly one top-level directory, named for the version and platform
      * SHA256SUMS.txt is inside, and EVERY file it lists is in the archive with
        the recorded hash -- computed from the archive's own bytes, not from
        whatever happens to be on disk beside it
      * nothing is in the archive that the manifest does not list
      * the licence set: seven texts under licences/, plus NOTICE.txt,
        LICENSE.txt and features/NOTICE.txt by name
      * entries are sorted and timestamps are pinned, so the bytes are a
        function of the contents
      * on a tar, that the three programs and the extension module carry mode
        0755 and that no build machine's uid, gid or username is recorded

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
    binaries, the Python extension and all seven font licences was rejected
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
if ($root -notmatch '^polylinker-\d+\.\d+\.\d+-(windows|linux|macos)-\S+$') {
    Bad "'$root' is not a <name>-<version>-<platform> directory"
}

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
        # nine font files and cannot be redistributed without their texts.
        'LICENSE.txt', 'NOTICE.txt', 'TRADEMARKS.md', 'features/NOTICE.txt'
        'licences/Hack-MIT-and-BitstreamVera.txt', 'licences/IBMPlex-OFL.txt'
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
    foreach ($r in $required) {
        if ($listed -notcontains $r) { Bad "$r is required in a $(if ($isWinArchive) { 'Windows' } elseif ($isMacArchive) { 'macOS' } else { 'Linux' }) archive and is not in the manifest" }
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
    $floor = if ($MinimumFiles -gt 0) { $MinimumFiles } else { $required.Count }
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
    $lic = @($listed | Where-Object { $_ -like 'licences/*' })
    if ($lic.Count -lt 7) { Bad "only $($lic.Count) font licence text(s) in the archive; NOTICE requires 7" }
    foreach ($required in 'NOTICE.txt', 'LICENSE.txt', 'features/NOTICE.txt') {
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
    if ($progs.Count -lt 4) { Bad "found $($progs.Count) program(s) in the archive; expected 4" }
    foreach ($p in $progs) {
        if ($rel[$p].Mode -ne '755') {
            Bad "$p has mode 0$($rel[$p].Mode); it would extract without the executable bit and the user would have to chmod it"
        }
    }
    foreach ($k in $rel.Keys) {
        if ($progs -notcontains $k -and $rel[$k].Mode -ne '644') { Bad "$k has mode 0$($rel[$k].Mode), expected 0644" }
    }
}

if ($fail) {
    Write-Host "FAIL  $([System.IO.Path]::GetFileName($Archive))" -ForegroundColor Red
    $fail | ForEach-Object { Write-Host "      $_" -ForegroundColor Red }
    exit 1
}
Write-Host ("ok    {0}: {1} file(s), {2} licence text(s), manifest verified against the archive's own bytes" -f
    [System.IO.Path]::GetFileName($Archive), $rel.Count, @($rel.Keys | Where-Object { $_ -like 'licences/*' }).Count) -ForegroundColor Green
exit 0
