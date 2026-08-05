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

# The version, read out of the workspace manifest rather than typed here.
#
# It reaches the user in three places -- the zip's name, the manifest header and
# the Add/Remove Programs entry the installer writes -- and the installer's
# upgrade comparison ("0.1.0 -> 0.2.0, over the existing install") is the first
# thing that reads it back. Three copies of a version string is three chances
# for two of them to disagree, so there is one copy and it is Cargo.toml's.
$version = ''
foreach ($line in (Get-Content 'Cargo.toml')) {
    if ($line -match '^\s*version\s*=\s*"([^"]+)"') { $version = $Matches[1]; break }
}
if (-not $version) { throw 'could not read the version out of Cargo.toml' }

if ($dirty) {
    Say '  WARNING: the working tree has uncommitted changes.' Yellow
    Say '  The commit recorded below does not describe these binaries.' Yellow
}

New-Item -ItemType Directory -Force $Out | Out-Null

# START FROM AN EMPTY DIRECTORY.
#
# This was not necessary while the manifest hashed a hard-coded list of four
# binaries. It is necessary now that it hashes whatever is on disk: a second run
# would otherwise hash the first run's zip, and a file dropped from `$notices`
# would linger in `dist/` and keep shipping. The failure mode this closes is the
# one that actually happened in reverse -- the `dist/` on this machine on
# 2026-08-04 was two required notices SHORT of what `$notices` demanded, because
# it was left over from a build predating both, and nothing ever noticed.
#
# Only a directory that is empty or is plainly a previous release gets cleared.
# Anything else is somebody's data and this refuses rather than guesses.
$existing = @(Get-ChildItem -LiteralPath $Out -Force)
if ($existing) {
    $looksLikeRelease = (Test-Path (Join-Path $Out 'SHA256SUMS.txt')) -or
                        (Test-Path (Join-Path $Out 'polylinker.exe'))
    if (-not $looksLikeRelease) {
        throw "$Out is not empty and does not look like a previous release. Point -Out somewhere else, or empty it yourself."
    }
    # -Path, not -LiteralPath: the whole point of the argument is the wildcard,
    # and -LiteralPath would look for a directory entry actually named "*".
    Remove-Item -Path (Join-Path $Out '*') -Recurse -Force
    Say '  cleared the previous contents of the output directory'
}

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

# The notices have to travel with the binaries, and until 2026-07-30 they did
# not: dist/ held four executables and a checksum file.
#
# This is an obligation, not a courtesy. polylinker.exe embeds NINE font files
# under four licences, and four of those require their text to accompany every
# copy: SIL OFL 1.1 clause 2 ("provided that each copy contains the above
# copyright notice and this license"), the Bitstream Vera licence reached through
# Hack, MIT for emoji-icon-font, and MIT for Phosphor. Shipping the exe by itself
# put the project
# out of step with licences it had correctly recorded in the repository and then
# left behind at the packaging step. The failure mode is that the record looks
# complete from inside the source tree, which is the only place anyone looked.
#
# The IBM Plex faces are what forced the issue rather than what caused it: they
# took the count of unaccompanied OFL faces from one to three.
#
# NINE, AND COUNTED OFF THE BINARY RATHER THAN OFF THIS ARRAY. That number read
# "seven" until 2026-08-04, and seven is the GUI's own font chain, not the exe:
# the two Plex faces, Phosphor, and the four that arrive through
# epaint_default_fonts. It missed the two faces pl-draw embeds, which is the
# same blind spot that shipped Liberation without its licence -- a count taken
# from the file you are editing sees only the crate you are thinking about.
# Measured instead by searching a release target/release/polylinker.exe for a
# 256-byte mid-file slice of each candidate .ttf -- mid-file so that no two
# faces can collide on a shared sfnt header. All nine are present, including
# LiberationSans-Regular.ttf (410,712 bytes) and LiberationSans-Bold (414,456),
# which the app reaches through its PNG export menu via pl_draw::png_at, and
# Phosphor-Bold.ttf (495,308) which comes from the egui-phosphor crate and is
# not on disk here at all. The same probe over pl.exe finds those two Liberation
# faces and none of the other seven, so the count is a property of the artifact
# and not of the workspace. No exe byte-count is quoted: it moves on any commit,
# and a number that churns is a number that goes stale unnoticed.
#
# EVERY FACE, NOT JUST THE TWO THIS REPOSITORY CHOSE -- six of them when this was
# written, seven since Phosphor, and nine since Liberation. The first version of
# this block copied IBMPlex-OFL.txt alone and left the four faces that arrive
# through epaint_default_fonts with no licence text at all -- including the two
# the paragraph above names by licence. Their texts are now vendored under
# bins/pl-gui/fonts/ and hashed in NOTICE, rather than read out of a Cargo
# registry: a release has to be cuttable from a checkout, and a populated
# %USERPROFILE%\.cargo is not part of one.
#
# Noto Emoji gets its OWN copy of the OFL rather than sharing the Plex one. OFL
# clause 2 asks for "the above copyright notice and this license", and the
# copyright above the Plex text is IBM's, with the reserved name "Plex".
#
# SIX FONT LICENCE TEXTS SINCE 2026-07-30, not five: Phosphor Icons 2.1 Bold is
# now embedded as the icon face and is MIT, and like emoji-icon-font it carries
# no copyright in its own `name` table -- ID 0 holds the family name and ID 7 is
# absent -- so a copy that ships without Phosphor-MIT.txt is a copy with no
# Phosphor permission notice anywhere in it. Unlike the four texts above, this
# one does NOT come out of a crate: egui-phosphor ships a licence for its own
# Rust wrapper and none for the typeface. See NOTICE.
$notices = @(
    @{ From = 'NOTICE';        To = 'NOTICE.txt' }
    @{ From = 'LICENSE';       To = 'LICENSE.txt' }
    @{ From = 'TRADEMARKS.md'; To = 'TRADEMARKS.md' }
    @{ From = 'bins/pl-gui/fonts/IBMPlex-OFL.txt'
       To   = 'licences/IBMPlex-OFL.txt' }                 # Plex Mono + Plex Sans
    @{ From = 'bins/pl-gui/fonts/Hack-MIT-and-BitstreamVera.txt'
       To   = 'licences/Hack-MIT-and-BitstreamVera.txt' }  # both notices, one file
    @{ From = 'bins/pl-gui/fonts/Ubuntu-UFL.txt'
       To   = 'licences/Ubuntu-UFL.txt' }                  # Ubuntu Light
    @{ From = 'bins/pl-gui/fonts/NotoEmoji-OFL.txt'
       To   = 'licences/NotoEmoji-OFL.txt' }               # NOT the Plex OFL
    @{ From = 'bins/pl-gui/fonts/emoji-icon-font-MIT.txt'
       To   = 'licences/emoji-icon-font-MIT.txt' }         # emoji-icon-font
    @{ From = 'bins/pl-gui/fonts/Phosphor-MIT.txt'
       To   = 'licences/Phosphor-MIT.txt' }                # Phosphor Icons Bold
    # SEVEN SINCE 2026-08-03, and this is the first entry NOT under
    # `bins/pl-gui/fonts/` -- which is exactly why it was missed, because the
    # array had been a list of everything in one directory.
    # `crates/pl-draw/src/font.rs` embeds 825,168 bytes of Liberation Sans under
    # SIL OFL 1.1, and BOTH shipped executables reach it: `pl` through `--png`
    # and the app through its export menu. So both carried the face and neither
    # carried its licence. NOTICE already called this text "committed and
    # shipped"; until now only the first half was true.
    @{ From = 'crates/pl-draw/fonts/Liberation-OFL.txt'
       To   = 'licences/Liberation-OFL.txt' }              # Liberation Sans + Bold
    # THE FIRST ENTRY THAT IS NOT ABOUT A FONT, and it was missing for the same
    # reason the one above it was: the array had become a list of licence texts
    # for typefaces, so a data obligation had no shape that fitted it. All ten
    # entries above are code notices -- three top-level files and seven font
    # licences -- and none of them had anything to do with the database.
    #
    # `crates/pl-features` `include_str!`s features.tsv and provenance.tsv, and
    # ALL FOUR artifacts this script ships depend on that crate, so all four
    # carry the CC BY 4.0 dataset and all four carry its attribution
    # obligations. NOTICE ends by saying `features/NOTICE` "must be packaged
    # with any distribution that includes the database"; this is that packaging.
    #
    # PARTLY COVERED IS WHY IT LASTED. NOTICE.txt ships and carries the UniProt
    # statement of changes, the NLM courtesy line and the Rfam CC0 note, and
    # `pl licences` prints the same subset out of the compiled-in table -- so
    # dist/ looked attributed. What only this file has is the per-family Rfam
    # primary-source credit table (24 rows, PLF:2000-PLF:2023, each carrying the
    # PMID from that family's own `#=GF RM` line) and the list of sources
    # deliberately not used, which records FPbase as HOLD and UniVec as NO-GO.
    # Rfam asks for per-family credit; a pointer to the credit is not the credit.
    #
    # It lands at `features/NOTICE.txt` rather than beside NOTICE.txt: the path
    # mirrors the repository so that the sentence in NOTICE.txt and the closing
    # line of `pl licences`, both of which name "features/NOTICE in the source
    # distribution", resolve inside a dist/ that has no source tree. `.txt`
    # because an extensionless file does not open on a double-click on Windows,
    # which is the same reason NOTICE and LICENSE gain one above.
    @{ From = 'features/NOTICE'
       To   = 'features/NOTICE.txt' }                      # the CC BY 4.0 dataset
)
foreach ($n in $notices) {
    if (-not (Test-Path $n.From)) {
        throw "the notice $($n.From) is missing; refusing to ship a binary without it"
    }
    $dest = Join-Path $Out $n.To
    $parent = Split-Path $dest -Parent
    if (-not (Test-Path $parent)) { New-Item -ItemType Directory -Force $parent | Out-Null }
    Copy-Item $n.From $dest -Force
}
Say "  notices: $($notices.Count) file(s)"

# The installer.
#
# Three text files and, if anybody has ever drawn one, an icon. They ship INSIDE
# the payload rather than beside it, which is the point: the installer's source
# of truth for what a copy contains is SHA256SUMS.txt, and SHA256SUMS.txt covers
# everything in this directory including the installer itself. So the installer
# is checksummed by the same manifest it verifies, and a user who checks the zip
# hash has checked the installer too.
#
# There is no compiled setup program here and that is a decision. See
# docs/RELEASING.md: without a code-signing certificate, an opaque `.exe` or
# `.msi` asks a user to execute megabytes they cannot inspect, from a publisher
# Windows reports as unknown. A readable script keeps the one trust affordance
# an unsigned build still has.
$installer = @(
    @{ From = 'tools/installer/Install-Polylinker.ps1'; To = 'Install-Polylinker.ps1'; Required = $true }
    @{ From = 'tools/installer/Install.cmd';            To = 'Install.cmd';            Required = $true }
    @{ From = 'tools/installer/README-WINDOWS.txt';     To = 'README-WINDOWS.txt';     Required = $true }
    # The icon, taken from where it is DRAWN rather than from a copy kept beside
    # the installer. `bins/pl-gui/build.rs` links this same file into
    # `polylinker.exe` as its application icon, so the Start Menu shortcut, the
    # Add/Remove Programs entry and the running window all show one picture with
    # one source. A second copy under tools/installer/ would be a second thing to
    # forget to regenerate; `bins/pl-gui/icon/build-icon.py` rebuilds this one
    # from polylinker.svg.
    #
    # Required as of 2026-08-05, where it was optional-and-absent before. It is
    # no longer cosmetic: `tools/ci.ps1` asserts that the frames in this file are
    # byte-identical to the RT_ICON resources inside the shipped .exe, so a
    # release that quietly dropped it would ship an installer whose shortcut has
    # no icon while the binary it points at has one.
    @{ From = 'bins/pl-gui/icon/polylinker.ico';        To = 'polylinker.ico';         Required = $true }
)
$installed = 0
foreach ($i in $installer) {
    if (-not (Test-Path $i.From)) {
        if ($i.Required) { throw "the installer file $($i.From) is missing" }
        continue
    }
    Copy-Item $i.From (Join-Path $Out $i.To) -Force
    $installed++
}
Say "  installer: $installed file(s)"

# Signing. Absent credentials this reports and continues; it never silently
# produces something that looks signed.
$signed = @()
if ($WindowsCert) {
    # Find signtool.exe rather than assuming it is on PATH.
    #
    # It is not. The Windows SDK installs it under `Windows Kits\10\bin\<ver>\x64`
    # and puts nothing on PATH, so the bare `& signtool` that stood here would
    # have failed with "not recognized" -- and it would have failed for the first
    # time on the day a certificate finally arrived, which is the worst possible
    # day to be debugging the release script. This path had never been executed
    # end to end on any machine here, because without a certificate it never
    # runs at all. Resolved the same way `.cargo\bin` is resolved above, and for
    # the same reason.
    $signtool = (Get-Command signtool.exe -ErrorAction SilentlyContinue).Source
    if (-not $signtool) {
        $kits = 'C:\Program Files (x86)\Windows Kits\10\bin'
        if (Test-Path $kits) {
            # Newest SDK first: an old signtool cannot produce a modern
            # dual-signature and its /tr support predates RFC 3161 timestamping.
            $signtool = Get-ChildItem $kits -Directory |
                Where-Object { $_.Name -match '^10\.' } |
                Sort-Object -Property Name -Descending |
                ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } |
                Where-Object { Test-Path $_ } |
                Select-Object -First 1
        }
    }
    if (-not $signtool) {
        throw 'signtool.exe was not found. Install the Windows SDK (the "Windows SDK Signing Tools" component is enough), or put signtool.exe on PATH.'
    }
    Say "  signing with '$WindowsCert'..."
    Say "  signtool: $signtool"
    foreach ($a in $artifacts) {
        $f = Join-Path $Out $a
        # The platform's own tool, using an identity already in the platform's
        # own store. Nothing here handles key material.
        & $signtool sign /n $WindowsCert /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $f
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
#
# EVERY FILE, NOT JUST THE EXECUTABLES. Until 2026-08-05 this loop iterated
# `$artifacts`, so the manifest covered four files out of sixteen: the eleven
# licence and notice texts -- the ones four licences require to travel with
# every copy -- had no integrity record at all, and neither would the installer.
# A manifest documented as "the only integrity guarantee an unsigned build has"
# that guarantees a quarter of the build is a manifest with a hole in the middle
# of its own claim.
#
# Enumerating the directory rather than a list is the other half. A list here
# would be a third copy of `$artifacts` and `$notices` and would drift from them
# exactly as `dist/` drifted from `$notices` twice in two days. What is on disk
# is what gets hashed, so nothing can ship unhashed; `tools/ci.ps1` asserts the
# converse, that nothing in the manifest is missing from disk, which together
# make it set equality.
$outFull = (Resolve-Path -LiteralPath $Out).Path
$shipped = Get-ChildItem -LiteralPath $Out -Recurse -File |
    Where-Object { $_.Name -ne 'SHA256SUMS.txt' } |
    # Forward slashes, so `sha256sum -c SHA256SUMS.txt` resolves these names on
    # the machine of whoever is checking them. This is the first release whose
    # manifest has any subdirectories in it at all, so it is the first one where
    # the separator was a choice.
    ForEach-Object { $_.FullName.Substring($outFull.Length + 1).Replace('\', '/') } |
    Sort-Object

$manifest = @()
$manifest += "polylinker release manifest 1"
$manifest += "version: $version"
$manifest += "built: $stamp"
$manifest += "commit: $commit$(if ($dirty) { ' (WORKING TREE DIRTY - not reproducible)' })"
$manifest += "rustc: $rustc"
$manifest += "signed: $(if ($signed) { $signed -join ', ' } else { 'no' })"
$manifest += '--'
foreach ($a in $shipped) {
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

# The Windows download.
#
# A zip is the primary Windows artifact, and the installer is a file inside it
# rather than a separate download. That ordering is the product decision: the
# zip needs no elevation, no trust decision and no installer at all -- unzip it
# and run polylinker.exe -- and the script inside is for the user who wants a
# Start Menu entry and an Add/Remove Programs row. Nobody is required to run
# anything they did not choose to.
#
# WRITTEN BY HAND RATHER THAN WITH Compress-Archive, so the bytes are a function
# of the contents. `Compress-Archive` stores each file's current mtime, so
# zipping the same sixteen files twice produces two different hashes, and a hash
# that changes when nothing changed is a hash nobody bothers to compare. Entries
# are sorted, timestamps are pinned to the build stamp, and compression is
# fixed. `docs/RELEASING.md` does not claim byte-for-byte reproducibility across
# MACHINES -- the binaries embed absolute paths -- but the packaging step should
# not be a second, avoidable source of drift on top of that.
$zipName = "polylinker-$version-windows-x64.zip"
$zipPath = Join-Path $outFull $zipName
$zipRoot = "polylinker-$version-windows-x64"
Add-Type -AssemblyName System.IO.Compression | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem | Out-Null
if (Test-Path -LiteralPath $zipPath) { Remove-Item -LiteralPath $zipPath -Force }

# Everything the manifest covers, plus the manifest, so a copy extracted from
# the zip can verify itself and so the installer inside it has the file it needs.
$zipEntries = @($shipped) + @('SHA256SUMS.txt') | Sort-Object
# A zip stores an MS-DOS date and time with no timezone, so what lands in the
# bytes is this wall clock regardless of where the build machine is. Constructed
# field by field rather than parsed, so no local-time conversion can creep in.
$fixedTime = [System.DateTimeOffset]::new(2000, 1, 1, 0, 0, 0, [TimeSpan]::Zero)

$fs = [System.IO.File]::Open($zipPath, [System.IO.FileMode]::CreateNew)
try {
    $zip = [System.IO.Compression.ZipArchive]::new($fs, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($rel in $zipEntries) {
            $entry = $zip.CreateEntry("$zipRoot/$rel", [System.IO.Compression.CompressionLevel]::Optimal)
            $entry.LastWriteTime = $fixedTime
            $es = $entry.Open()
            try {
                $bytes = [System.IO.File]::ReadAllBytes((Join-Path $outFull ($rel.Replace('/', [System.IO.Path]::DirectorySeparatorChar))))
                $es.Write($bytes, 0, $bytes.Length)
            } finally { $es.Dispose() }
        }
    } finally { $zip.Dispose() }
} finally { $fs.Dispose() }

# The zip cannot contain its own hash, so the hash goes beside it. This is the
# ONE number a user is asked to compare by hand, and it is what the three
# numbered steps in README-WINDOWS.txt begin with.
$zipHash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLower()
[System.IO.File]::WriteAllText(
    "$zipPath.sha256",
    "$zipHash  $zipName`n",
    (New-Object System.Text.UTF8Encoding($false))
)
Say ("  zip: {0}  ({1:N1} MB, {2} entries)" -f $zipName, ((Get-Item -LiteralPath $zipPath).Length / 1MB), $zipEntries.Count)

Say ''
if (-not $Quiet) { Get-Content $manifestPath | ForEach-Object { Write-Host "  $_" } }
Say ''
Say "  $zipHash  $zipName"
Say ''
if (-not $signed) {
    Say 'This build is UNSIGNED.' Yellow
    Say 'Publish SHA256SUMS.txt and the .sha256 sidecar beside the zip so it can' Yellow
    Say 'be verified anyway. docs/RELEASING.md says what that does and does not prove.' Yellow
}
