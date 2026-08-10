<#
.SYNOPSIS
    Build the release artifacts and record exactly what they are.

.DESCRIPTION
    Produces the binaries, a SHA-256 manifest, and a build record naming the
    toolchain and commit. Signs them if — and only if — credentials are
    supplied; otherwise it says loudly that the output is unsigned and keeps
    going, because an unsigned build that says so is useful and an unsigned
    build that pretends otherwise is not.

    Nothing here supplies them, and nothing is going to. Code signing came off
    the roadmap on 2026-08-06 — see docs/RELEASING.md, "What is done, and what
    is not" — so every release this script cuts is unsigned. -WindowsCert and
    -MacIdentity are kept as a hook for anyone who ever reverses that decision,
    not as a step waiting for a certificate.

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

.PARAMETER BinDir
    Where to find the compiled binaries. Default: target/release. The macOS
    universal build is the reason this is a parameter: `lipo` writes its output
    somewhere cargo never does, and the alternative was a second script.

.PARAMETER SkipBuild
    Do not run cargo; package what is already in -BinDir. Only legal together
    with an explicit -BinDir, because "skip the build" with the default
    directory is how a release gets cut from yesterday's binaries.

.PARAMETER PlatformLabel
    Overrides the platform half of the archive name -- `windows-x64`,
    `linux-x64`, `macos-universal`. Defaults to this machine's OS and
    architecture. The one case that needs it is a universal macOS binary, which
    is not the architecture of the machine that built it.

.PARAMETER ArchiveFormat
    `zip` or `tar.gz`. Defaults to zip on Windows and tar.gz elsewhere, which is
    what a release wants. It is a parameter so that `tools/ci.ps1` can exercise
    the tar writer on the Windows machine that is the only one that ever runs
    the gate -- an archive format whose only test is a green CI job on a runner
    is an archive format nobody has looked at.
#>
param(
    [string]$WindowsCert = '',
    [string]$MacIdentity = '',
    [string]$Out = 'dist',
    [string]$BinDir = '',
    [string]$PlatformLabel = '',
    [ValidateSet('', 'zip', 'tar.gz')]
    [string]$ArchiveFormat = '',
    [switch]$SkipBuild,

    # For the gate, which checks that the script runs and that its manifest
    # verifies. Nothing is suppressed that changes what is produced.
    [switch]$Quiet
)

$ErrorActionPreference = 'Stop'
function Say($msg, $colour = 'Gray') { if (-not $Quiet) { Write-Host $msg -ForegroundColor $colour } }

# A DIRECTORY PATH IN THE SAME SPELLING `FileInfo.FullName` USES, with exactly
# one trailing separator. Anything that subtracts a directory path from a
# `FullName` -- or asks whether a `FullName` sits under one -- must go through
# this, or it is comparing two strings that two different normalisers produced.
#
# There are three ways those normalisers disagree, and this repository has now
# been bitten by two of them. All four facts below were measured on this machine
# rather than assumed:
#
#   1. 8.3 ALIASES. `Resolve-Path` returns the string it was handed;
#      `Get-ChildItem` reports the name the volume actually holds. For
#      `C:\PROGRA~1` those are `C:\PROGRA~1` (11 chars) and `C:\Program Files`
#      (16). This is what broke CI run 31325886841: a GitHub runner's `$env:TEMP`
#      is `C:\Users\RUNNER~1\AppData\Local\Temp` while the profile is really
#      `runneradmin`, so the base string was 3 characters short, `Substring` cut
#      3 too few, and every manifest name arrived with the tail of the output
#      directory welded to its front -- `84/features/NOTICE.txt` for an `-Out`
#      ending `pl-release-check-6584`. It could not fire on the author's machine:
#      no component of `C:\Users\alf22\AppData\Local\Temp` is long enough to
#      carry an alias, so the subtraction was correct by accident.
#   2. A TRAILING SEPARATOR. `Resolve-Path 'C:\dir\'` keeps it, and the old
#      `Substring($base.Length + 1)` then cut one character too many: measured,
#      `a.txt` came back as `.txt`. This one needs no 8.3 alias and reproduces on
#      any machine, which is why `tools/ci.ps1` now hands this script an `-Out`
#      with a trailing separator on purpose.
#   3. CASE. The provider normalises it (`c:\users\...` comes back
#      `C:\Users\...`) and `GetFullPath` does not. Case cannot change a length,
#      so it does not affect the subtraction -- but it is why every comparison
#      below is OrdinalIgnoreCase and must stay that way.
#
# `GetFullPath` is what expands the alias, on Windows PowerShell 5.1 and on
# pwsh 7 alike, and it does so even for components that do not exist yet
# (`C:\PROGRA~1\nope\deeper` -> `C:\Program Files\nope\deeper`), which is why
# this works on a directory that has not been created. It is NOT enough on its
# own: it resolves a relative path against the .NET current directory, which is
# not the PowerShell location -- measured, .NET said `C:\Users\alf22\Zotero`
# while the session was in `%TEMP%`. Since this script is invoked as
# `release.ps1 -Out dist` by .github/workflows/ci.yml, that would have silently
# pointed at a different `dist`. So the path is made absolute the way PowerShell
# resolves it first, and expanded second.
#
# `tools/ci.ps1` and `tools/installer/Install-Polylinker.ps1` carry copies -- the
# installer because it ships alone inside the release zip with nothing to
# dot-source. If you edit this function, edit those: the gate step
# 'Get-DirectoryPrefix is one function copied, not three functions drifting'
# finds every copy under tools/ by parsing and fails the build when they differ.
function Get-DirectoryPrefix([string]$Path) {
    $abs = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
    $abs = [System.IO.Path]::GetFullPath($abs)
    $sep = [System.IO.Path]::DirectorySeparatorChar
    return $abs.TrimEnd($sep, [System.IO.Path]::AltDirectorySeparatorChar) + $sep
}

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# WHICH PLATFORM THIS IS, AND WHY THERE IS STILL ONE SCRIPT.
#
# Everything genuinely platform-specific below is a name or a container format:
# the `.exe` suffix, what CPython will load an extension module under, whether
# there is an installer to ship, and zip versus tar.gz. Everything that is NOT
# platform-specific is the part that has gone wrong twice -- `$notices`, the
# version read out of Cargo.toml, and the rule that the manifest hashes whatever
# is on disk. A second script would be a second copy of those three, and the
# comments on `$notices` are a thirty-line record of what a second copy does.
#
# So: one script, three `if`s. `tools/ci.ps1` runs THIS file, so the licence set
# that ships on Linux and macOS is the one the gate already checks, rather than
# a parallel list nobody exercises.
#
# `$IsWindows`, `$IsLinux` and `$IsMacOS` are pwsh 7 automatic variables and do
# not exist in Windows PowerShell 5.1 at all -- where they read as $null. 5.1
# only runs on Windows, so $null means Windows.
$onWindows = if ($null -eq $IsWindows) { $true } else { [bool]$IsWindows }
$onMac     = [bool]$IsMacOS
$onLinux   = [bool]$IsLinux
$exe       = if ($onWindows) { '.exe' } else { '' }

if ($SkipBuild -and -not $BinDir) {
    throw '-SkipBuild needs an explicit -BinDir. Skipping the build against the default target/release is how a release gets cut from binaries nobody rebuilt.'
}
if (-not $BinDir) { $BinDir = 'target/release' }

if (-not $PlatformLabel) {
    $arch = switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture) {
        'X64'   { 'x64' }
        'Arm64' { 'arm64' }
        default { throw "no archive name is defined for $_" }
    }
    $os = if ($onWindows) { 'windows' } elseif ($onMac) { 'macos' } elseif ($onLinux) { 'linux' }
          else { throw 'this is not Windows, Linux or macOS, and the packaging rules below do not cover it' }
    $PlatformLabel = "$os-$arch"
}

# Same as tools/ci.ps1: a shell that has not sourced the profile does not have
# the toolchain on PATH, and a release script failing on that is noise.
#
# GUARDED, and the guard is the whole point. The unguarded form of these two
# lines is what broke the first three-platform release: `$env:USERPROFILE` does
# not exist off Windows, `Join-Path` refuses a null `-Path`, and the
# `$ErrorActionPreference = 'Stop'` at the top of this file turns that refusal
# into a terminating error. The Linux job died in 41 seconds, before it had
# compiled anything, on a line whose only purpose is convenience on a
# workstation.
#
# It was read during the port and called harmless, on the reasoning that
# prepending an empty string is a no-op. That is true of the assignment and
# irrelevant to the call that produces it. Both hosted runners put cargo on
# PATH themselves, so off Windows the whole block is unnecessary and is skipped.
if ($env:USERPROFILE) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo/bin'
    if (Test-Path $cargoBin) { $env:PATH = "$cargoBin$([IO.Path]::PathSeparator)$env:PATH" }
}

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
    # `polylinker$exe`, not `polylinker.exe`: on a Unix run neither sentinel
    # existed, so a second run into a non-empty dist/ threw instead of clearing
    # it -- a Windows-shaped test standing in for "is this ours".
    $looksLikeRelease = (Test-Path (Join-Path $Out 'SHA256SUMS.txt')) -or
                        (Test-Path (Join-Path $Out "polylinker$exe"))
    if (-not $looksLikeRelease) {
        throw "$Out is not empty and does not look like a previous release. Point -Out somewhere else, or empty it yourself."
    }
    # -Path, not -LiteralPath: the whole point of the argument is the wildcard,
    # and -LiteralPath would look for a directory entry actually named "*".
    Remove-Item -Path (Join-Path $Out '*') -Recurse -Force
    Say '  cleared the previous contents of the output directory'
}

if ($SkipBuild) {
    Say "  skipping the build; packaging $BinDir as given"
} else {
    Say '  building...'
    # `$ErrorActionPreference = 'Stop'` turns *any* stderr from a native command
    # into a terminating error, and cargo writes warnings there. A warning must not
    # abort a release — it did, on an "output filename collision" note about a PDB —
    # so the exit code is what decides, which is the only thing that actually says
    # whether the build worked.
    #
    # `--locked` since 2026-08-05, matching every cargo invocation in ci.yml. A
    # release that let cargo rewrite Cargo.lock would be a release of a dependency
    # graph nobody chose and nobody tested.
    $prevEAP = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    cargo build --release --workspace --locked 2>&1 | Out-Null
    $buildOk = ($LASTEXITCODE -eq 0)
    $ErrorActionPreference = $prevEAP
    if (-not $buildOk) { throw 'the release build failed' }
}

# THE THREE EXECUTABLES ARE REQUIRED, NOT OPTIONAL.
#
# This loop used to be `if (Test-Path $p)`, over the hardcoded names `pl.exe`,
# `polylinker.exe` and `pl-mcp.exe`, with a single `if (-not $artifacts) { throw }`
# underneath. On Windows that is indistinguishable from a required list. On Linux
# it was a silent hole: none of the three names matched, the `.so` fallback below
# made `$artifacts` non-empty anyway, and the throw never fired -- so a Linux run
# produced a "release" consisting of the Python extension, eleven correct licence
# texts, the four Windows installer files and a zip named `windows-x64`, with none
# of the three programs in it. Nothing anywhere asserted their presence.
#
# Named and required, so a missing binary is a named failure rather than a
# smaller archive.
$artifacts = @()
$missing = @()
foreach ($stem in 'pl', 'polylinker', 'pl-mcp') {
    $name = "$stem$exe"
    $p = Join-Path $BinDir $name
    if (Test-Path $p) {
        Copy-Item $p (Join-Path $Out $name) -Force
        $artifacts += $name
    } else {
        $missing += $name
    }
}

# The Python extension has to be *named* for the platform or it cannot be
# imported at all. CPython on Windows loads `.pyd`, not `.dll`, however
# correctly the DLL was built — and cargo has no say in the matter, so the
# rename belongs here. Shipping `polylinker.dll` and expecting the user to
# rename it is a papercut that reads as "the wheel is broken".
#
# macOS is the case that had no branch at all. cargo writes a cdylib there as
# `libpolylinker.dylib`, and CPython on macOS loads extension modules named
# `.so` -- NOT `.dylib`, which is the one intuition to distrust here. Before
# this table the mac path fell through to the `.so` fallback, found nothing,
# and shipped no extension.
$pyMap = if ($onWindows) { @{ Built = 'polylinker.dll';     Shipped = 'polylinker.pyd' } }
         elseif ($onMac)  { @{ Built = 'libpolylinker.dylib'; Shipped = 'polylinker.so' } }
         else             { @{ Built = 'libpolylinker.so';    Shipped = 'polylinker.so' } }
$pyBuilt = Join-Path $BinDir $pyMap.Built
if (Test-Path $pyBuilt) {
    Copy-Item $pyBuilt (Join-Path $Out $pyMap.Shipped) -Force
    $artifacts += $pyMap.Shipped
} else {
    $missing += $pyMap.Built
}

if ($missing) {
    throw "the build produced nothing at $BinDir for: $($missing -join ', '). A release missing a binary is not a smaller release, it is a broken one."
}

# The notices have to travel with the binaries, and until 2026-07-30 they did
# not: dist/ held four executables and a checksum file.
#
# This is an obligation, not a courtesy. polylinker.exe embeds TEN font files
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
# TEN, AND COUNTED OFF THE BINARY RATHER THAN OFF THIS ARRAY. That number read
# "seven" until 2026-08-04, and seven is the GUI's own font chain, not the exe:
# the two Plex faces, Phosphor, and the four that arrive through
# epaint_default_fonts. It missed the two faces pl-draw embeds, which is the
# same blind spot that shipped Liberation without its licence -- a count taken
# from the file you are editing sees only the crate you are thinking about.
# Measured instead by searching a release target/release/polylinker.exe for a
# 256-byte mid-file slice of each candidate .ttf -- mid-file so that no two
# faces can collide on a shared sfnt header. All ten are present, including
# LiberationSans-Regular.ttf (410,712 bytes) and LiberationSans-Bold (414,456),
# which the app reaches through its PNG export menu via pl_draw::png_at, and
# Phosphor-Bold.ttf (495,308) which comes from the egui-phosphor crate and is
# not on disk here at all. The same probe over pl.exe finds those two Liberation
# faces and none of the other eight, so the count is a property of the artifact
# and not of the workspace. No exe byte-count is quoted: it moves on any commit,
# and a number that churns is a number that goes stale unnoticed.
#
# The tenth is Inter-SemiBold.ttf (279,408 bytes), added 2026-08-09 with the
# design-system port. Re-probed that day rather than reasoned about: the same
# mid-file slice search over a freshly built polylinker.exe found it and the
# four other on-disk faces, five for five.
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
#
# BOTH HALVES OF THE OFFERED LICENCE SINCE 2026-08-06, and this is the same
# failure as the two above wearing the project's own licence rather than
# somebody else's. `Cargo.toml`, `tools/release-notes.md` and
# `packages/circular-map/package.json` have all said `MIT OR Apache-2.0` since
# the beginning, and only the Apache text existed -- so a recipient who chose
# the MIT half received a permission notice they could not read, which is
# precisely the position the eight font texts above are here to avoid. The MIT
# text is now `LICENSE-MIT` at the root and ships beside `LICENSE.txt`.
#
# It is `LICENSE-MIT.txt` here for the same reason `NOTICE` and `LICENSE` gain
# an extension: an extensionless file does not open on a double-click on
# Windows.
$notices = @(
    @{ From = 'NOTICE';        To = 'NOTICE.txt' }
    @{ From = 'LICENSE';       To = 'LICENSE.txt' }      # Apache-2.0
    @{ From = 'LICENSE-MIT';   To = 'LICENSE-MIT.txt' }  # the other half of the choice
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
       To   = 'licences/phosphor-mit.txt' }                # Phosphor Icons Bold
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
    # EIGHT SINCE 2026-08-09, with Inter SemiBold: the heading face the design
    # system port brought in. A THIRD copy of the SIL OFL, and its DIFFERENCES
    # from the two above are the reason it is not deduplicated against them.
    # Inter declares NO Reserved Font Name, where Plex reserves "Plex" and
    # Liberation reserves four -- and that absence is the clause the shipped
    # file actually depends on, because the vendored copy is a PUA-stripped
    # subset and therefore a Modified Version, which clause 3 would otherwise
    # forbid from keeping its name. A recipient handed only the Plex OFL cannot
    # check the one claim that makes the file lawful. See NOTICE.
    @{ From = 'bins/pl-gui/fonts/Inter-OFL.txt'
       To   = 'licences/Inter-OFL.txt' }                   # Inter SemiBold
    # THE FIRST ENTRY THAT IS NOT ABOUT A FONT, and it was missing for the same
    # reason the one above it was: the array had become a list of licence texts
    # for typefaces, so a data obligation had no shape that fitted it. All eleven
    # entries above are code notices -- three top-level files and eight font
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

# The installer. WINDOWS ONLY, and deliberately not ported.
#
# `tools/installer/Install-Polylinker.ps1` is registry, Start Menu, PATH,
# ProgIds and an Add/Remove Programs row: every one of those is a Win32 concept
# and none has a counterpart to port. The Unix answer to "how do I install this"
# is `tar xf` and, if you want it on PATH, move it somewhere already on PATH --
# which is what README-LINUX.txt and README-MACOS.txt say, in those words. There
# is no .deb, no .rpm, no Homebrew formula and no `curl | sh`, and inventing one
# would be inventing a second file list for the licences to fall out of.
#
# What each platform DOES get is one short text file. Windows has had
# README-WINDOWS.txt since the installer existed; the other two now have the
# equivalent, because the thing a downloader most needs to be told -- the
# Gatekeeper quarantine flag, and the glibc floor -- has to travel in the
# archive. A release page can be read once and then the tarball gets copied to
# a cluster by somebody who never saw it.
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
$installer = if (-not $onWindows) {
    @(
        @{ From = if ($onMac) { 'tools/readme/README-MACOS.txt' } else { 'tools/readme/README-LINUX.txt' }
           To   = if ($onMac) { 'README-MACOS.txt' }            else { 'README-LINUX.txt' }
           Required = $true }
    )
} else { @(
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
) }
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
    # have failed with "not recognized" -- and it would have failed on the first
    # day this branch was ever run, which is the worst possible day to be
    # debugging the release script. That day is not scheduled: signing is off
    # the roadmap and this branch has never been executed end to end on any
    # machine here. It is fixed anyway, because a dormant branch nobody can run
    # is not a hook, and this one is kept as a hook. Resolved the same way
    # `.cargo\bin` is resolved above, and for the same reason.
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
} elseif ($onWindows) {
    Say '  NOT SIGNED: no -WindowsCert given.' Yellow
    Say '  Windows SmartScreen will warn on first run. See docs/RELEASING.md.' Yellow
} elseif ($onMac) {
    Say '  NOT SIGNED and NOT NOTARISED: no Developer ID identity.' Yellow
    Say '  Gatekeeper will quarantine this download. README-MACOS.txt says what to do.' Yellow
} else {
    Say '  NOT SIGNED. Linux has no equivalent expectation; the checksum is the guarantee.' Yellow
}
if ($MacIdentity -and -not $onMac) {
    Say '  macOS signing must run on macOS; skipping here.' Yellow
} elseif ($MacIdentity) {
    # Deliberately not automated. The codesign / notarytool / stapler recipe in
    # docs/RELEASING.md needs an Apple ID and an app-specific password, and a
    # release script that can hold those is a release script that can leak them.
    Say '  -MacIdentity is not wired up; run the recipe in docs/RELEASING.md by hand.' Yellow
}

# SHA-256 over every artifact. This is what a user or an IT department can check
# against, and for the archive this script writes it is the only integrity
# guarantee there is: this manifest is not signed. The one on the release page
# is. `.github/workflows/release.yml` builds a second, cross-platform
# SHA256SUMS.txt over the three archives and the MSI once all three legs have
# finished, and signs THAT with the Ed25519 release key whose public half is
# compiled into pl and polylinker (pl-mcp and the Python module do not depend
# on pl-update and carry no key). The distinction is worth keeping straight, because
# both files have the same name: a checksum says the bytes match what was
# published, and only the signature says who published it.
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
# The base for that subtraction comes from `Get-DirectoryPrefix` and not from
# `Resolve-Path`, for the reasons set out in full where that function is defined:
# `$_.FullName` below is whatever the FileSystemProvider says, and until
# 2026-08-09 this line asked a different normaliser for the string it subtracted.
#
# The `StartsWith` is not decoration. The arithmetic is only valid if
# `$_.FullName` really does sit under `$outFull`, and that assumption used to be
# made silently -- which is why the failure presented as `Get-FileHash` refusing
# a path nobody could see the origin of, several lines later, instead of as an
# error naming the two strings that disagreed.
$outFull = Get-DirectoryPrefix $Out
$shipped = Get-ChildItem -LiteralPath $Out -Recurse -File |
    Where-Object { $_.Name -ne 'SHA256SUMS.txt' } |
    ForEach-Object {
        if (-not $_.FullName.StartsWith($outFull, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "$($_.FullName) is not under $outFull, so its name in the manifest cannot be computed"
        }
        # Forward slashes, so `sha256sum -c SHA256SUMS.txt` resolves these names
        # on the machine of whoever is checking them. This is the first release
        # whose manifest has any subdirectories in it at all, so it is the first
        # one where the separator was a choice.
        $_.FullName.Substring($outFull.Length).Replace('\', '/')
    } |
    Sort-Object

$manifest = @()
$manifest += "polylinker release manifest 1"
$manifest += "version: $version"
# Which platform's copy this is. Three archives now carry a file of this name
# and they are not interchangeable: the same line that tells a user their
# download is intact should tell them which download it was.
$manifest += "platform: $PlatformLabel"
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
#
# An ABSOLUTE path, because `WriteAllText` resolves a relative one against the
# .NET current directory rather than the PowerShell location, and those are not
# the same thing -- measured on this machine, .NET reported `C:\Users\alf22\
# Zotero` while the session's location was `%TEMP%`. `$outFull` is the absolute
# form already, and reusing it rather than calling `Resolve-Path` again also
# stops this line producing `dir\\SHA256SUMS.txt` when `-Out` was given with a
# trailing separator, which `tools/ci.ps1` now does deliberately.
[System.IO.File]::WriteAllText(
    $outFull + 'SHA256SUMS.txt',
    ($manifest -join "`n") + "`n",
    (New-Object System.Text.UTF8Encoding($false))
)

# The download.
#
# ONE ARCHIVE PER PLATFORM AND NOTHING ELSE. On Windows the installer is a file
# inside it rather than a separate download, and that ordering is the product
# decision: the zip needs no elevation, no trust decision and no installer at
# all -- unzip it and run polylinker.exe -- and the script inside is for the
# user who wants a Start Menu entry and an Add/Remove Programs row. Nobody is
# required to run anything they did not choose to.
#
# ZIP ON WINDOWS, TAR.GZ ON THE UNIXES. That is the convention, and on the Unix
# side it is also the only one of the two that works: a zip has no portable
# place to record a file mode, so `unzip` clears the executable bit and the
# first thing the user does after extracting is `chmod +x`. A tar carries the
# mode in the header. Windows keeps the zip because Explorer opens one without
# any additional program and does not open a .tar.gz at all.
#
# BOTH ARE WRITTEN BY HAND, so the bytes are a function of the contents.
# `Compress-Archive` stores each file's current mtime, so zipping the same
# nineteen files twice produces two different hashes, and a hash that changes
# when nothing changed is a hash nobody bothers to compare. The same argument
# rules out shelling out to `tar`, which additionally would put the build
# machine's uid, gid and username into the archive. Entries are sorted,
# timestamps and ownership are pinned, and compression is fixed.
# `docs/RELEASING.md` does not claim byte-for-byte reproducibility across
# MACHINES -- the binaries embed absolute paths -- but the packaging step should
# not be a second, avoidable source of drift on top of that.
if (-not $ArchiveFormat) { $ArchiveFormat = if ($onWindows) { 'zip' } else { 'tar.gz' } }
$archiveRoot = "polylinker-$version-$PlatformLabel"
$archiveName = "$archiveRoot.$ArchiveFormat"
$archivePath = Join-Path $outFull $archiveName
if (Test-Path -LiteralPath $archivePath) { Remove-Item -LiteralPath $archivePath -Force }

# Everything the manifest covers, plus the manifest, so a copy extracted from
# the archive can verify itself and so the installer inside it has the file it
# needs.
$entries = @($shipped) + @('SHA256SUMS.txt') | Sort-Object
# 2000-01-01T00:00:00Z, spelled twice because the two containers count
# differently: a zip stores an MS-DOS date and time with no timezone at all, and
# a tar stores seconds since the Unix epoch. Constructed field by field rather
# than parsed, so no local-time conversion can creep into either.
$fixedTime = [System.DateTimeOffset]::new(2000, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
$fixedUnix = [long]$fixedTime.ToUnixTimeSeconds()

# Which files are programs. The three executables and the extension module get
# 0755; everything else -- licences, notices, the readme -- gets 0644. Only the
# tar can express this, and it is the whole reason the Unixes get a tar.
$executable = @($artifacts)

function Read-Payload($rel) {
    [System.IO.File]::ReadAllBytes(
        (Join-Path $outFull ($rel.Replace('/', [System.IO.Path]::DirectorySeparatorChar))))
}

if ($ArchiveFormat -eq 'zip') {
    Add-Type -AssemblyName System.IO.Compression | Out-Null
    Add-Type -AssemblyName System.IO.Compression.FileSystem | Out-Null
    $fs = [System.IO.File]::Open($archivePath, [System.IO.FileMode]::CreateNew)
    try {
        $zip = [System.IO.Compression.ZipArchive]::new($fs, [System.IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($rel in $entries) {
                $entry = $zip.CreateEntry("$archiveRoot/$rel", [System.IO.Compression.CompressionLevel]::Optimal)
                $entry.LastWriteTime = $fixedTime
                $es = $entry.Open()
                try {
                    $bytes = Read-Payload $rel
                    $es.Write($bytes, 0, $bytes.Length)
                } finally { $es.Dispose() }
            }
        } finally { $zip.Dispose() }
    } finally { $fs.Dispose() }
} else {
    # A ustar archive, written field by field.
    #
    # The format is a 512-byte header per member followed by the payload padded
    # to a 512-byte boundary, and two zero blocks at the end. Every numeric
    # field is octal ASCII. The checksum is the sum of the header's bytes with
    # the checksum field itself read as eight spaces -- which is the one rule
    # that cannot be inferred from a hex dump and the one that, got wrong,
    # produces an archive GNU tar reads happily and bsdtar refuses.
    #
    # uid/gid 0 and empty uname/gname, so the archive does not record who built
    # it; mtime pinned like the zip's. `tar` would have supplied all four from
    # the build machine.
    function Add-TarEntry {
        # $Mode is the octal permission bits AS A STRING -- '755', '644'. Not an
        # integer: PowerShell has no C-style octal literal, so `0755` is seven
        # hundred and fifty-five, and pwsh 7's `0o755` is a PARSE error under
        # Windows PowerShell 5.1, which would take the whole script down on the
        # one platform that never reaches this branch. The field is written as
        # octal ASCII anyway, so a string needs no conversion at all.
        param([System.IO.Stream]$Stream, [string]$Name, [byte[]]$Data, [string]$Mode, [char]$Type)
        # 100 bytes is ustar's name field. The longest name this ever produces is
        # about seventy characters, but a silently truncated path in an archive
        # nobody unpacks until release day is not a failure worth discovering then.
        $nameBytes = [System.Text.Encoding]::ASCII.GetBytes($Name)
        if ($nameBytes.Length -gt 100) { throw "the tar entry name '$Name' is longer than ustar's 100-byte field" }

        $h = New-Object byte[] 512
        $put = {
            param($off, $text)
            $b = [System.Text.Encoding]::ASCII.GetBytes([string]$text)
            [Array]::Copy($b, 0, $h, $off, $b.Length)
        }
        # An octal field of width n holds n-1 digits and a NUL.
        $oct = { param($v, $width) ([Convert]::ToString([long]$v, 8)).PadLeft($width - 1, '0') }

        & $put 0   $Name
        & $put 100 $Mode.PadLeft(7, '0')
        & $put 108 (& $oct 0 8)              # uid
        & $put 116 (& $oct 0 8)              # gid
        & $put 124 (& $oct $Data.Length 12)
        & $put 136 (& $oct $fixedUnix 12)
        & $put 148 '        '                # checksum field, as spaces, for the sum below
        & $put 156 $Type
        & $put 257 'ustar'                   # magic, NUL-terminated
        & $put 263 '00'                      # version
        # uname/gname left empty: a tarball should not carry the build account's name.
        & $put 329 (& $oct 0 8)              # devmajor
        & $put 337 (& $oct 0 8)              # devminor

        $sum = 0
        foreach ($b in $h) { $sum += $b }
        # Six octal digits, a NUL, then a space -- the layout every extractor
        # accepts. The width-8 helper above would emit seven digits and no space.
        & $put 148 (([Convert]::ToString($sum, 8)).PadLeft(6, '0') + "`0 ")

        $Stream.Write($h, 0, 512)
        if ($Data.Length -gt 0) {
            $Stream.Write($Data, 0, $Data.Length)
            $pad = (512 - ($Data.Length % 512)) % 512
            if ($pad) { $Stream.Write((New-Object byte[] $pad), 0, $pad) }
        }
    }

    $empty = New-Object byte[] 0
    # Directory members, so an extractor never has to invent a mode for a
    # directory it created implicitly. Every distinct parent, plus the root.
    #
    # ONE SORT OVER DIRECTORIES AND FILES TOGETHER, not directories first. The
    # first version emitted every directory and then every file, which is a
    # perfectly good tar and is not sorted -- so the archive's entry order was
    # a function of which member happened to be a directory, and the "entries
    # are sorted" property the zip has and `tools/check-archive.ps1` asserts did
    # not hold. A directory name is a prefix of everything inside it, so a
    # single sort still places each directory ahead of its own contents.
    $tarMembers = @(
        @($archiveRoot) + @(
            $entries | ForEach-Object {
                $p = Split-Path $_ -Parent
                if ($p) { "$archiveRoot/$($p.Replace('\', '/'))" }
            }
        ) | Sort-Object -Unique | ForEach-Object {
            [pscustomobject]@{ Name = "$_/"; Rel = $null; Mode = '755'; Type = '5' }
        }
    ) + @(
        $entries | ForEach-Object {
            [pscustomobject]@{
                Name = "$archiveRoot/$_"
                Rel  = $_
                Mode = if ($executable -contains $_) { '755' } else { '644' }
                Type = '0'
            }
        }
    ) | Sort-Object -Property Name

    $fs = [System.IO.File]::Open($archivePath, [System.IO.FileMode]::CreateNew)
    try {
        # `$true` as the third argument leaves $fs open for the `finally` to
        # close; without it the gzip stream disposes it first and the outer
        # Dispose throws.
        $gz = [System.IO.Compression.GZipStream]::new(
            $fs, [System.IO.Compression.CompressionLevel]::Optimal, $true)
        try {
            foreach ($m in $tarMembers) {
                $data = if ($m.Type -eq '5') { $empty } else { Read-Payload $m.Rel }
                Add-TarEntry $gz $m.Name $data $m.Mode $m.Type
            }
            # Two zero blocks: the end-of-archive marker. Without them GNU tar
            # reads the members and then reports an unexpected end of file.
            $gz.Write((New-Object byte[] 1024), 0, 1024)
        } finally { $gz.Dispose() }
    } finally { $fs.Dispose() }
}

# The archive cannot contain its own hash, so the hash goes beside it. This is
# the ONE number a user is asked to compare by hand, and it is what the numbered
# steps in each README begin with.
$archiveHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLower()
[System.IO.File]::WriteAllText(
    "$archivePath.sha256",
    "$archiveHash  $archiveName`n",
    (New-Object System.Text.UTF8Encoding($false))
)
Say ("  archive: {0}  ({1:N1} MB, {2} entries)" -f $archiveName, ((Get-Item -LiteralPath $archivePath).Length / 1MB), $entries.Count)

Say ''
if (-not $Quiet) { Get-Content $manifestPath | ForEach-Object { Write-Host "  $_" } }
Say ''
Say "  $archiveHash  $archiveName"
Say ''
if (-not $signed) {
    Say 'This build is UNSIGNED.' Yellow
    Say 'Publish SHA256SUMS.txt and the .sha256 sidecar beside the zip so it can' Yellow
    Say 'be verified anyway. docs/RELEASING.md says what that does and does not prove.' Yellow
}
