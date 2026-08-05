<#
.SYNOPSIS
    Install Polylinker for the current user, or remove it again.

.DESCRIPTION
    This is the Windows installer. It is a readable script rather than a
    compiled `.msi` or `.exe`, and that is the whole design.

    WHY A SCRIPT AND NOT A COMPILED INSTALLER

    No code-signing certificate exists for this project (docs/RELEASING.md).
    Everything Polylinker ships is therefore unsigned, and an unsigned installer
    has exactly two things it can offer a cautious user: the checksum, and the
    ability to read it before running it. A compiled installer keeps the first
    and throws away the second -- it asks the user to execute several megabytes
    they cannot inspect, from a publisher Windows reports as unknown, and offers
    them a click-through as the only way forward. `docs/RELEASING.md:25-31`
    already calls that "teaching a bad habit"; wrapping it in a wizard does not
    make it a better habit.

    Readability is the only trust affordance left when the cryptographic one is
    unaffordable. So: this file, in plain text, beside the binaries it installs.

    The day a certificate exists, the right thing to add is an `.msi`, because
    an MSI is what Group Policy and Intune consume and what gives an
    administrator a product code instead of a detection script. The per-machine
    layout below (`-AllUsers`) is deliberately the layout an MSI would use, so
    that is a port and not a redesign. See docs/RELEASING.md.

    WHAT IT WILL NOT DO

    * It never contacts the network. Not to check a version, not to fetch a
      runtime, not to report an install. `tools/ci.ps1` greps this file for the
      names of every PowerShell facility that could, and fails if one appears.
      The product's central claim is that it sends nothing anywhere, and an
      installer is part of the product.
    * It installs no auto-updater, no service and no scheduled task: nothing it
      puts on this machine ever runs when you did not start it. See
      docs/RELEASING.md, "There is no auto-updater, on purpose". (Polylinker
      does have an update check as of 2026-08-06 -- `pl update`, which you type,
      and a switch in the editor that ships off. Neither is installed by this
      script, because neither is a thing that gets installed: they are part of
      the two programs you already have here.)
    * It takes no file association. Associations are opt-in (`-Associate`), are
      additive rather than destructive, and are reversible with one flag. See
      the ASSOCIATIONS section below.
    * It writes nothing outside the two places it names in the plan it prints
      before it does anything: the install directory, and a small number of
      registry values under this user's own hive.

    WHAT SURVIVES AN UNINSTALL

    Everything under `%LOCALAPPDATA%\Polylinker`. That directory holds four
    things with four different claims on being kept:

      layout              a window preference
      recovery\*.recover  UNSAVED USER WORK, from a crash
      session-*           the restore-tabs bench
      index\              a regenerable library cache, possibly hundreds of MB

    An uninstaller that deletes the second one has destroyed the only copy of
    somebody's afternoon. `bins/pl-gui/src/recover.rs:223-231` already documents
    that directory as somewhere "a user can find and delete them without
    touching anything else", which reads as an argument for never touching it
    programmatically -- so nothing here does. Only the cache is offered for
    deletion, behind `-RemoveCache`, off by default, and it prints the measured
    size first.

    There is deliberately no "also remove my settings?" checkbox. A checkbox at
    uninstall time is answered by muscle memory, and one of the two answers
    cannot be taken back.

.PARAMETER AllUsers
    Install to Program Files for every user on the machine. Requires an already
    elevated session; this script does not elevate itself, because a script that
    asks for administrator rights is a script whose plan you should have read
    first. Registers NO file associations -- see ASSOCIATIONS.

.PARAMETER AddToPath
    Add the install directory to PATH so `pl` works in a terminal. Off by
    default: PATH is persistent shell configuration and belongs to the user.

.PARAMETER Associate
    Offer Polylinker as a handler for sequence files. Additive only. Prompts
    once, and the prompt names whatever currently opens `.dna`.

.PARAMETER Unassociate
    Remove the associations and nothing else.

.PARAMETER Uninstall
    Remove what the install receipt records, and only that.

.PARAMETER RemoveCache
    With -Uninstall, also delete the regenerable library index cache. Never
    touches settings or crash-recovery drafts.

.PARAMETER DryRun
    Print the complete plan and stop. Changes nothing. This is the same plan the
    real run prints before asking for confirmation.

.PARAMETER Yes
    Skip the typed confirmation. For an administrator deploying to ten machines,
    and for the gate.

.PARAMETER Prefix
    Override the install directory.

.PARAMETER Source
    Where the payload is. Defaults to this script's own directory, which is
    where it sits inside the release zip.

.PARAMETER RegistryRoot
    Write registry values under this key instead of their real locations. Only
    tools/ci.ps1 passes this, so a round-trip test can run without touching the
    real Add/Remove Programs list or the real PATH.

.PARAMETER StateDir
    Override %LOCALAPPDATA%\Polylinker. Only the gate passes this, so the
    "uninstall must not touch user state" assertion can plant a sentinel
    somewhere harmless.

.PARAMETER SelfTest
    Run this file's own unit tests over the PATH-editing functions and exit.
    Those functions are the only pure logic here and they are the part whose
    bugs are destructive, so they are tested where they live.

.EXAMPLE
    .\Install-Polylinker.ps1 -DryRun
    .\Install-Polylinker.ps1
    .\Install-Polylinker.ps1 -AddToPath -Associate
    .\Install-Polylinker.ps1 -Uninstall
#>
[CmdletBinding()]
param(
    [switch]$AllUsers,
    [switch]$AddToPath,
    [switch]$Associate,
    [switch]$Unassociate,
    [switch]$Uninstall,
    [switch]$RemoveCache,
    [switch]$DryRun,
    [switch]$Yes,
    [string]$Prefix,
    [string]$Source,
    [string]$RegistryRoot,
    [string]$StateDir,
    [string]$StartMenuDir,
    [switch]$SelfTest,

    # Set when this script has already re-launched itself out of the install
    # directory it is about to delete. Not for humans.
    [switch]$NoRelaunch
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$AppName = 'Polylinker'
$ReceiptName = 'install-receipt.txt'
$ReceiptHeader = 'polylinker install receipt 1'

# ---------------------------------------------------------------------------
# Output
#
# No colour is load-bearing: every line reads correctly in a console that
# renders none, because a plan a user cannot read in their own terminal is a
# plan they will skip.
# ---------------------------------------------------------------------------
function Say  ($m, $c = 'Gray')   { Write-Host $m -ForegroundColor $c }
function Warn ($m)                { Write-Host $m -ForegroundColor Yellow }
function Head ($m)                { Write-Host ''; Write-Host $m -ForegroundColor Cyan }

# ---------------------------------------------------------------------------
# PATH editing
#
# THE ONLY PURE LOGIC IN THIS FILE, AND THE ONLY PART THAT CAN DESTROY
# SOMETHING THAT IS NOT OURS. A PATH is a user's shell, accumulated over years;
# a bug here is not "the install failed", it is "half their tools stopped
# resolving and they have no idea why". So both directions are pure functions
# of (current value, entry) with no registry access, and `-SelfTest` exercises
# them against the inputs that actually occur.
#
# Three rules, each of which exists because the naive version gets it wrong:
#
#   1. NEVER REFORMAT THE REST OF THE VALUE. A trailing semicolon stays, a
#      doubled semicolon stays, ordering stays. The temptation to "tidy" a PATH
#      is how an installer silently drops an empty segment that something else
#      depended on.
#   2. COMPARE WITHOUT THE TRAILING BACKSLASH AND WITHOUT CASE. Windows paths
#      are case-insensitive and `C:\x` and `C:\x\` are the same directory, so a
#      literal match re-adds an entry that is already there on every reinstall.
#   3. RETURN $null FOR "NOTHING TO DO". The caller must be able to tell a
#      no-op from a change, because a no-op must not rewrite the value at all --
#      rewriting is what converts a REG_EXPAND_SZ into a REG_SZ and bakes an
#      expanded %USERPROFILE% into somebody's roaming profile.
# ---------------------------------------------------------------------------

function Test-SamePathEntry {
    param([string]$A, [string]$B)
    if ($null -eq $A) { $A = '' }
    if ($null -eq $B) { $B = '' }
    return ($A.Trim().TrimEnd('\') -ieq $B.Trim().TrimEnd('\'))
}

function Add-PathEntryTo {
    param([AllowNull()][string]$Current, [string]$Entry)
    if ($null -eq $Current) { $Current = '' }
    foreach ($p in ($Current -split ';')) {
        if (Test-SamePathEntry $p $Entry) { return $null }   # already there
    }
    if ($Current.Trim() -eq '') { return $Entry }
    if ($Current.EndsWith(';'))  { return ($Current + $Entry) }
    return ($Current + ';' + $Entry)
}

function Remove-PathEntryFrom {
    param([AllowNull()][string]$Current, [string]$Entry)
    if ([string]::IsNullOrEmpty($Current)) { return $null }
    $keep = @()
    $found = $false
    foreach ($p in ($Current -split ';')) {
        if (Test-SamePathEntry $p $Entry) { $found = $true; continue }
        $keep += $p
    }
    if (-not $found) { return $null }
    return ($keep -join ';')
}

# The documented ceiling for the user PATH value is 2047 characters. Longer
# values do work in modern shells, but legacy tools truncate silently, and an
# installer that silently truncates a PATH has done the exact damage rule 1
# above is written to prevent.
$PathMax = 2047

function Invoke-SelfTest {
    $script:selfTestFailures = [System.Collections.ArrayList]::new()
    $script:selfTestRun = 0
    function T($name, $got, $want) {
        $script:selfTestRun++
        # -eq against $null is the trap here: `$null -eq ''` is false but
        # `'' -eq $null` is false too, so the comparison is written with the
        # expected value on the left and both sides stringified only when
        # neither is $null.
        $same = if ($null -eq $want) { $null -eq $got } else { $want -eq $got }
        if (-not $same) {
            $script:selfTestFailures.Add($name) | Out-Null
            Write-Host ("  FAIL {0}`n       got:  [{1}]`n       want: [{2}]" -f $name, $got, $want) -ForegroundColor Red
        } else {
            Write-Host ("  ok   {0}" -f $name) -ForegroundColor DarkGray
        }
    }

    $E = 'C:\Users\me\AppData\Local\Programs\Polylinker'

    T 'add to empty'            (Add-PathEntryTo ''            $E) $E
    T 'add to null'             (Add-PathEntryTo $null         $E) $E
    T 'add appends'             (Add-PathEntryTo 'C:\a;C:\b'   $E) "C:\a;C:\b;$E"
    # A trailing semicolon is somebody's deliberate (or accidental) formatting,
    # and either way it is not ours to normalise.
    T 'add keeps trailing ;'    (Add-PathEntryTo 'C:\a;'       $E) "C:\a;$E"
    T 'add keeps empty segment' (Add-PathEntryTo 'C:\a;;C:\b'  $E) "C:\a;;C:\b;$E"
    T 'add is idempotent'       (Add-PathEntryTo "C:\a;$E"     $E) $null
    T 'add ignores case'        (Add-PathEntryTo "C:\a;$($E.ToUpper())" $E) $null
    T 'add ignores trailing \'  (Add-PathEntryTo "C:\a;$E\"    $E) $null
    T 'add ignores whitespace'  (Add-PathEntryTo "C:\a; $E "   $E) $null
    # THE ONE THAT MATTERS ON THIS MACHINE. A user PATH that ends in an
    # unexpanded %USERPROFILE% is REG_EXPAND_SZ, and it must come back out with
    # the literal percent signs intact. If it does not, the value has been
    # expanded and re-written as REG_SZ, and it breaks the day the profile moves.
    $expandy = '%USERPROFILE%\AppData\Local\Microsoft\WindowsApps'
    T 'add preserves %VARS%'    (Add-PathEntryTo $expandy $E) "$expandy;$E"

    T 'remove absent'           (Remove-PathEntryFrom 'C:\a;C:\b' $E) $null
    T 'remove from null'        (Remove-PathEntryFrom $null       $E) $null
    T 'remove middle'           (Remove-PathEntryFrom "C:\a;$E;C:\b" $E) 'C:\a;C:\b'
    T 'remove last'             (Remove-PathEntryFrom "C:\a;$E"     $E) 'C:\a'
    T 'remove only'             (Remove-PathEntryFrom $E            $E) ''
    T 'remove keeps trailing ;' (Remove-PathEntryFrom "C:\a;$E;"    $E) 'C:\a;'
    T 'remove ignores case'     (Remove-PathEntryFrom "C:\a;$($E.ToUpper());C:\b" $E) 'C:\a;C:\b'
    T 'remove ignores trailing \' (Remove-PathEntryFrom "C:\a;$E\;C:\b" $E) 'C:\a;C:\b'
    T 'remove undoes add'       (Remove-PathEntryFrom (Add-PathEntryTo 'C:\a;C:\b' $E) $E) 'C:\a;C:\b'
    # ONE ASYMMETRY, ASSERTED SO IT IS A DECISION RATHER THAN A SURPRISE. Adding
    # to a value that ends in `;` reuses that semicolon as the separator, so the
    # trailing empty segment is spent and removing the entry cannot put it back.
    # `C:\a;` becomes `C:\a`. The alternative -- emitting `C:\a;;<entry>` to
    # preserve it -- trades a cosmetic difference for a doubled separator that
    # looks like a bug to every human who reads the value afterwards. Windows
    # ignores empty PATH segments, so nothing resolves differently either way.
    T 'add then remove spends a trailing ;' (Remove-PathEntryFrom (Add-PathEntryTo 'C:\a;' $E) $E) 'C:\a'
    T 'remove all copies'       (Remove-PathEntryFrom "$E;C:\a;$E" $E) 'C:\a'

    # A value already at the ceiling must be refused, not truncated. The check
    # itself lives in the caller; this asserts the arithmetic it depends on.
    $long = ('C:\padpadpadpad' * 145)
    $grown = Add-PathEntryTo $long $E
    T 'near-limit value grows'  ($grown.Length -gt $PathMax) $true

    Write-Host ''
    if ($script:selfTestFailures.Count -eq 0) {
        Write-Host ("PATH self-test passed ({0} cases)" -f $script:selfTestRun) -ForegroundColor Green
        return 0
    }
    Write-Host ("PATH self-test FAILED: {0}" -f ($script:selfTestFailures -join ', ')) -ForegroundColor Red
    return 1
}

if ($SelfTest) { exit (Invoke-SelfTest) }

# ---------------------------------------------------------------------------
# Registry access
#
# The provider (`HKCU:\...`) is used for ordinary creates and deletes because it
# reads clearly. The .NET API is used wherever the VALUE KIND matters, which the
# provider will not preserve: reading a PATH must not expand `%USERPROFILE%`,
# and writing one back must keep REG_EXPAND_SZ as REG_EXPAND_SZ.
# ---------------------------------------------------------------------------

function Split-RegPath {
    param([string]$Path)
    $i = $Path.IndexOf('\')
    if ($i -lt 0) { throw "not a registry path: $Path" }
    $hive = $Path.Substring(0, $i).TrimEnd(':').ToUpper()
    $sub = $Path.Substring($i + 1)
    $h = switch ($hive) {
        'HKCU' { [Microsoft.Win32.Registry]::CurrentUser }
        'HKLM' { [Microsoft.Win32.Registry]::LocalMachine }
        default { throw "this installer only writes to HKCU and HKLM, not $hive" }
    }
    return @{ Hive = $h; Sub = $sub }
}

function Get-RegValueRaw {
    <#
        Returns @{ Value; Kind } with environment variables UNEXPANDED, or $null
        if the key or value does not exist.
    #>
    param([string]$Key, [string]$Name)
    $p = Split-RegPath $Key
    $k = $p.Hive.OpenSubKey($p.Sub, $false)
    if (-not $k) { return $null }
    try {
        $v = $k.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        if ($null -eq $v) { return $null }
        return @{ Value = [string]$v; Kind = $k.GetValueKind($Name) }
    } finally { $k.Close() }
}

function Set-RegValueRaw {
    param([string]$Key, [string]$Name, $Value, [Microsoft.Win32.RegistryValueKind]$Kind)
    $p = Split-RegPath $Key
    $k = $p.Hive.CreateSubKey($p.Sub, $true)
    try { $k.SetValue($Name, $Value, $Kind) } finally { $k.Close() }
}

# ---------------------------------------------------------------------------
# Where things go
# ---------------------------------------------------------------------------

if (-not $Source) { $Source = $PSScriptRoot }
$Source = (Resolve-Path -LiteralPath $Source).Path

if (-not $StateDir) { $StateDir = Join-Path $env:LOCALAPPDATA $AppName }

if (-not $Prefix) {
    if ($AllUsers) {
        $Prefix = Join-Path $env:ProgramFiles $AppName
    } else {
        # DELIBERATELY NOT %LOCALAPPDATA%\Polylinker. That is the app's own state
        # root -- `recover.rs:243-256` and `pl-scan/src/store.rs:22-36` both land
        # there -- and program files sharing a directory with user state is
        # precisely how an uninstaller ends up unable to tell the two apart.
        # `%LOCALAPPDATA%\Programs\<App>` is where per-user installs go on
        # Windows and is what Explorer and Settings expect.
        $Prefix = Join-Path (Join-Path $env:LOCALAPPDATA 'Programs') $AppName
    }
}

$scope = if ($AllUsers) { 'per-machine' } else { 'per-user' }

# Registry destinations. `-RegistryRoot` redirects all of them at once so the
# gate can install, inspect and uninstall without going near the real ARP list,
# the real PATH, or the real class registrations.
if ($RegistryRoot) {
    $ArpKey     = "$RegistryRoot\Uninstall\$AppName"
    $ClassesKey = "$RegistryRoot\Classes"
    $EnvKey     = "$RegistryRoot\Environment"
} elseif ($AllUsers) {
    $ArpKey     = "HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\$AppName"
    $ClassesKey = "HKLM\Software\Classes"
    $EnvKey     = 'HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
} else {
    $ArpKey     = "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\$AppName"
    $ClassesKey = "HKCU\Software\Classes"
    $EnvKey     = 'HKCU\Environment'
}

function RegProvider([string]$p) { ($p -replace '^(HKCU|HKLM)\\', '$1:\') }

# ---------------------------------------------------------------------------
# The types this app opens
#
# Taken from the app's own open dialog (`bins/pl-gui/src/main.rs:3025-3034`),
# minus `.seq` and `.ape`. Both of those are claimed by many unrelated programs,
# and an "Open with" list is a place where being noisy costs the user something.
#
# `.plproj` is the one entry whose default handler is set rather than merely
# offered, and it is worth writing down why: Polylinker defines that format.
# Claiming the default for a file type you invented is not the thing
# `docs/PLAN.md:212` forbids. Taking `.dna` from an installed SnapGene is.
# ---------------------------------------------------------------------------
$FileTypes = @(
    @{ Ext = '.plproj';  Desc = 'Polylinker project';    Default = $true  }
    @{ Ext = '.dna';     Desc = 'SnapGene DNA file';     Default = $false }
    @{ Ext = '.gb';      Desc = 'GenBank sequence';      Default = $false }
    @{ Ext = '.gbk';     Desc = 'GenBank sequence';      Default = $false }
    @{ Ext = '.genbank'; Desc = 'GenBank sequence';      Default = $false }
    @{ Ext = '.fasta';   Desc = 'FASTA sequence';        Default = $false }
    @{ Ext = '.fa';      Desc = 'FASTA sequence';        Default = $false }
    @{ Ext = '.fna';     Desc = 'FASTA sequence';        Default = $false }
    @{ Ext = '.ab1';     Desc = 'Sanger trace';          Default = $false }
)
function ProgIdFor([string]$ext) { "$AppName$($ext.Replace('.', '_')).1" }

# ---------------------------------------------------------------------------
# Reading the payload
# ---------------------------------------------------------------------------

function Read-Manifest {
    <#
        Parse SHA256SUMS.txt into @{ Version; Commit; Files = @{name = hash} }.

        The manifest is the single source of truth for what a copy contains.
        That is deliberate and it is the reason this installer keeps no file
        list of its own: every compiled installer has a second list -- a WiX
        `<Component>` set, an Inno `[Files]` section -- which is a hand-copied
        duplicate of `tools/release.ps1`'s `$notices` array, and the comments in
        that array are a thirty-line record of exactly that copy drifting, twice,
        in two days. A list that cannot drift is one that does not exist.
    #>
    param([string]$Dir)
    $p = Join-Path $Dir 'SHA256SUMS.txt'
    if (-not (Test-Path -LiteralPath $p)) {
        throw "SHA256SUMS.txt is not in $Dir. Extract the whole release zip and run this script from inside it."
    }
    $lines = [System.IO.File]::ReadAllText($p) -split "`r?`n"
    $m = @{ Version = ''; Commit = ''; Files = [ordered]@{} }
    $past = $false
    foreach ($l in $lines) {
        if (-not $past) {
            if ($l -eq '--') { $past = $true; continue }
            if ($l -match '^version:\s*(.+)$') { $m.Version = $Matches[1].Trim() }
            if ($l -match '^commit:\s*(.+)$')  { $m.Commit  = $Matches[1].Trim() }
            continue
        }
        if (-not $l) { continue }
        if ($l -notmatch '^([0-9a-f]{64})  (.+)$') { throw "SHA256SUMS.txt has a line this installer cannot read: $l" }
        $m.Files[$Matches[2]] = $Matches[1]
    }
    if ($m.Files.Count -eq 0) { throw 'SHA256SUMS.txt lists no files' }
    return $m
}

function Test-Payload {
    <#
        Verify the payload before a single byte is copied, and refuse rather
        than warn.

        Two checks, and the second is the one that matters legally.

        1. Every file the manifest names exists and hashes to what it says.
        2. Every file on disk is in the manifest. This is set EQUALITY, not
           containment, and it is what makes the licence obligation structural:
           `tools/release.ps1` refuses to build a copy whose notices are
           missing, the manifest covers everything it produced, and this refuses
           to install a copy whose contents and manifest disagree. Nothing here
           enumerates the eleven notice files, so nothing here can fall out of
           step with them the way `dist/` did on 2026-08-03 and again on
           2026-08-04.

        The floor below is a floor and not a list, for the same reason
        `tools/ci.ps1` asserts a bench score rather than a list of cases: it
        catches a copy that was thinned out wholesale, without becoming a
        second inventory that has to be maintained.
    #>
    param([string]$Dir, $Manifest)

    # Forward slashes, to match the manifest. `sha256sum -c` wants them, so the
    # manifest has them, so the comparison has to speak the same dialect --
    # otherwise every file "on disk" looks like a file the manifest never saw
    # and this refuses to install a perfectly good copy.
    $onDisk = Get-ChildItem -LiteralPath $Dir -Recurse -File |
        ForEach-Object { $_.FullName.Substring($Dir.Length + 1).Replace('\', '/') }

    # Three files cannot be in the manifest, for one reason each, and none of
    # them is an exemption anybody chose:
    #
    #   SHA256SUMS.txt   is the manifest. A file cannot contain its own hash.
    #   Uninstall.cmd    is written by an install, so it does not exist in a
    #   install-receipt.txt   fresh copy and cannot have been hashed at build
    #                    time. Both appear only when re-running over an existing
    #                    install, which is a supported thing to do.
    #
    # This script, Install.cmd and README-WINDOWS.txt are NOT here: they ship
    # inside the payload and are hashed like everything else, so verifying the
    # zip verifies the installer.
    $notInManifest = @('SHA256SUMS.txt', $ReceiptName, 'Uninstall.cmd')
    $onDisk = @($onDisk | Where-Object {
        $notInManifest -notcontains $_ -and
        # The zip and its checksum sidecar are BUILT FROM the manifest, so they
        # cannot be in it. They are present when installing straight out of
        # `dist/` rather than out of an extracted zip, which is a supported
        # thing to do and used to fail here with "2 file(s) the manifest does
        # not cover" -- naming the zip the user had just extracted.
        $_ -notlike '*.zip' -and $_ -notlike '*.zip.sha256'
    })

    $missing = @($Manifest.Files.Keys | Where-Object { $onDisk -notcontains $_ })
    if ($missing) {
        throw ("this copy is incomplete -- {0} file(s) the manifest lists are not here:`n    {1}`nDownload the release zip again and extract all of it." -f $missing.Count, ($missing -join "`n    "))
    }
    $extra = @($onDisk | Where-Object { -not $Manifest.Files.Contains($_) })
    if ($extra) {
        throw ("this copy has {0} file(s) the manifest does not cover:`n    {1}`nA release is only as trustworthy as its manifest, so this will not install a file the manifest never saw." -f $extra.Count, ($extra -join "`n    "))
    }

    foreach ($name in $Manifest.Files.Keys) {
        $f = Join-Path $Dir $name
        $h = (Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash.ToLower()
        if ($h -ne $Manifest.Files[$name]) {
            throw "$name does not match its recorded checksum. This copy is damaged or was modified; download it again."
        }
    }

    foreach ($required in 'NOTICE.txt', 'LICENSE.txt') {
        if (-not $Manifest.Files.Contains($required)) {
            throw "$required is not in this copy. Four of the licences Polylinker's fonts are under require their text to accompany every copy, so this is not installable."
        }
    }
    $licences = @($Manifest.Files.Keys | Where-Object { $_ -like 'licences/*' })
    if ($licences.Count -lt 7) {
        throw ("only {0} licence text(s) in this copy; a complete one has at least 7. See NOTICE.txt." -f $licences.Count)
    }

    return @{ Count = $Manifest.Files.Count; Bytes = (Get-ChildItem -LiteralPath $Dir -Recurse -File | Measure-Object -Property Length -Sum).Sum }
}

# ---------------------------------------------------------------------------
# The plan
#
# Every mutation goes through this list. Nothing in this script writes anything
# that is not first an entry here, which is what makes `-DryRun` an honest
# preview rather than a separate code path that can drift from the real one --
# and what makes the receipt a transcript of what happened rather than a second
# guess at it.
# ---------------------------------------------------------------------------

function New-Plan { , [System.Collections.ArrayList]::new() }

function Add-Act {
    param([System.Collections.ArrayList]$Plan, [hashtable]$Act)
    $Plan.Add($Act) | Out-Null
}

function Show-Plan {
    param([System.Collections.ArrayList]$Plan)
    foreach ($a in $Plan) {
        switch ($a.Kind) {
            'mkdir'    { Say ("  create dir  {0}" -f $a.Path) }
            'copy'     { Say ("  copy        {0}  ({1:N0} bytes)" -f $a.To, $a.Bytes) }
            'write'    { Say ("  write       {0}  ({1})" -f $a.Path, $a.Note) }
            'shortcut' { Say ("  shortcut    {0}" -f $a.Path) }
            'reg'      { Say ("  registry    {0}\{1} = {2}" -f $a.Key, $(if ($a.Name) { $a.Name } else { '(Default)' }), $a.Value) }
            # Writes nothing, so it does not belong in a list of what will
            # change. It only tells the uninstaller what it may tidy up.
            'regcleanup' { }
            'path'     { Say ("  PATH        {0}: add {1}" -f $a.Key, $a.Entry) }
            'rmfile'   { Say ("  delete      {0}" -f $a.Path) }
            'rmdir'    { Say ("  delete dir  {0}" -f $a.Path) }
            'rmkey'    { Say ("  registry    delete {0}" -f $a.Key) }
            'rmvalue'  { Say ("  registry    delete {0}\{1}" -f $a.Key, $(if ($a.Name) { $a.Name } else { '(Default)' })) }
            'rmkeyifempty' { Say ("  registry    delete {0}  (only if nothing else is left in it)" -f $a.Key) }
            'rmpath'   { Say ("  PATH        {0}: remove {1}" -f $a.Key, $a.Entry) }
            default    { Say ("  {0} {1}" -f $a.Kind, ($a | Out-String)) }
        }
    }
}

function Invoke-Plan {
    param([System.Collections.ArrayList]$Plan)
    $receipt = [System.Collections.ArrayList]::new()
    foreach ($a in $Plan) {
        switch ($a.Kind) {
            'mkdir' {
                if (-not (Test-Path -LiteralPath $a.Path)) { New-Item -ItemType Directory -Force -Path $a.Path | Out-Null }
                $receipt.Add("dir $($a.Path)") | Out-Null
            }
            'copy' {
                $parent = Split-Path -Parent $a.To
                if (-not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
                Copy-Item -LiteralPath $a.From -Destination $a.To -Force
                # Copy-Item carries a source file's alternate data streams with
                # it, and a file extracted by Explorer from a downloaded zip has
                # a Zone.Identifier stream saying it came from the internet. It
                # is stripped here so an installed executable is not treated as
                # a fresh download every time it is launched.
                Unblock-File -LiteralPath $a.To -ErrorAction SilentlyContinue
                $receipt.Add("file $($a.To)") | Out-Null
            }
            'write' {
                # WriteAllText, not Copy-Item: a newly created file has no
                # Zone.Identifier stream at all, which is what keeps the
                # uninstall entry point runnable on a machine where the download
                # is still marked.
                [System.IO.File]::WriteAllText($a.Path, $a.Content, (New-Object System.Text.UTF8Encoding($false)))
                $receipt.Add("file $($a.Path)") | Out-Null
            }
            'shortcut' {
                $ws = New-Object -ComObject WScript.Shell
                $lnk = $ws.CreateShortcut($a.Path)
                $lnk.TargetPath = $a.Target
                $lnk.WorkingDirectory = Split-Path -Parent $a.Target
                $lnk.Description = $a.Description
                if ($a.Icon) { $lnk.IconLocation = $a.Icon }
                $lnk.Save()
                $receipt.Add("shortcut $($a.Path)") | Out-Null
            }
            'reg' {
                Set-RegValueRaw -Key $a.Key -Name $a.Name -Value $a.Data -Kind $a.Type
                # WHAT UNDOES THIS, recorded per action rather than inferred
                # later. Three different answers, because three different things
                # are being written:
                #
                #   key         a key that exists only because we made it, so
                #               the whole subtree goes.
                #   value       a single value inside a key that is NOT ours --
                #               an OpenWithProgids entry lives under `.dna`,
                #               which belongs to whoever else registered there.
                #               Deleting that key would delete their
                #               registration too.
                #   keyifempty  a key that may be ours or may not. Removed only
                #               if nothing is left in it once our values are
                #               gone, so an extension nobody else claimed does
                #               not leave an empty husk behind, and one somebody
                #               else claimed is untouched.
                #
                # Getting this wrong the other way is what the first version did:
                # it recorded the ProgId keys and not the OpenWithProgids values,
                # so an uninstall deleted `Polylinker_dna.1` and left `.dna`
                # pointing at it -- a dangling entry in the user's Open-with menu
                # that no longer resolved to anything.
                if ($a.Contains('Record')) {
                    switch ($a.Record) {
                        'key'   { $receipt.Add("regkey $($a.Key)") | Out-Null }
                        'value' { $receipt.Add("regvalue $($a.Key)|$($a.Name)") | Out-Null }
                    }
                }
            }
            'regcleanup' {
                $receipt.Add("regkeyifempty $($a.Key)") | Out-Null
            }
            'path' {
                Set-RegValueRaw -Key $a.Key -Name 'Path' -Value $a.New -Kind $a.Type
                $receipt.Add("pathentry $($a.Key)|$($a.Entry)") | Out-Null
            }
        }
    }
    return $receipt
}

function Confirm-Or-Stop {
    param([string]$What)
    if ($Yes) { return }
    Write-Host ''
    Write-Host "Type yes to $What, or anything else to stop: " -NoNewline -ForegroundColor Cyan
    $answer = Read-Host
    if ($answer.Trim().ToLower() -ne 'yes') {
        Say 'Nothing was changed.'
        exit 2
    }
}

# ---------------------------------------------------------------------------
# Associations
#
# ADDITIVE ONLY, AND THIS IS NOT A HEDGE.
#
# `docs/PLAN.md:212` says silently stealing `.dna` from an installed SnapGene
# "will enrage exactly the user you are courting -- ask, don't take". It costs
# nothing to honour, because since Windows 8 the default handler is not settable
# by an installer anyway: it lives under
# `Explorer\FileExts\<ext>\UserChoice` behind a per-user hash only the shell can
# compute, and writing there is both unsupported and detected. An installer that
# claims it "set the default" either overwrote a `(Default)` value the shell now
# ignores, or corrupted UserChoice and got the association reset to nothing.
#
# So what is written is what Windows actually intends an application to write:
# a ProgId under `Software\Classes`, referenced from the extension's
# `OpenWithProgids` list, plus an `Applications\polylinker.exe` entry. The
# effect is that Polylinker appears in "Open with" and in "Open with -> Choose
# another app", where the user can promote it themselves in the one dialog that
# is allowed to. What was already the default stays the default.
#
# `(Default)` under the extension key is NOT written -- that is the value that
# would displace another program's registration on a machine where UserChoice
# has never been set. `.plproj` is the exception, and only because Polylinker
# invented it.
# ---------------------------------------------------------------------------

function Get-CurrentHandler {
    <#
        What opens this extension right now, in the user's words. Read-only, and
        it reads the same two places the shell does, in the same order.
    #>
    param([string]$Ext)
    $progId = $null
    $uc = "Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\$Ext\UserChoice"
    if (Test-Path -LiteralPath $uc) {
        $progId = (Get-ItemProperty -LiteralPath $uc -ErrorAction SilentlyContinue).ProgId
    }
    if (-not $progId) {
        $cr = "Registry::HKEY_CLASSES_ROOT\$Ext"
        if (Test-Path -LiteralPath $cr) {
            $progId = (Get-ItemProperty -LiteralPath $cr -ErrorAction SilentlyContinue).'(default)'
        }
    }
    if (-not $progId) { return $null }
    $friendly = $progId
    $pk = "Registry::HKEY_CLASSES_ROOT\$progId"
    if (Test-Path -LiteralPath $pk) {
        $d = (Get-ItemProperty -LiteralPath $pk -ErrorAction SilentlyContinue).'(default)'
        if ($d) { $friendly = $d }
    }
    return $friendly
}

function Add-AssociationActs {
    param([System.Collections.ArrayList]$Plan, [string]$Exe, [string]$Icon)
    $cmd = '"{0}" "%1"' -f $Exe
    $S = [Microsoft.Win32.RegistryValueKind]::String
    $N = [Microsoft.Win32.RegistryValueKind]::None

    # The generic application registration. Without this, Polylinker does not
    # appear in "Open with" for a type it has no ProgId for at all.
    $app = "$ClassesKey\Applications\polylinker.exe"
    Add-Act $Plan @{ Kind='reg'; Key="$app\shell\open\command"; Name=''; Data=$cmd; Type=$S; Value=$cmd }
    Add-Act $Plan @{ Kind='reg'; Key=$app; Name='FriendlyAppName'; Data=$AppName; Type=$S; Value=$AppName; Record='key' }

    foreach ($t in $FileTypes) {
        $progId = ProgIdFor $t.Ext
        $key = "$ClassesKey\$progId"
        $extKey = "$ClassesKey\$($t.Ext)"
        Add-Act $Plan @{ Kind='reg'; Key=$key; Name=''; Data=$t.Desc; Type=$S; Value=$t.Desc; Record='key' }
        if ($Icon) {
            Add-Act $Plan @{ Kind='reg'; Key="$key\DefaultIcon"; Name=''; Data=$Icon; Type=$S; Value=$Icon }
        }
        Add-Act $Plan @{ Kind='reg'; Key="$key\shell\open\command"; Name=''; Data=$cmd; Type=$S; Value=$cmd }

        # The additive half: an entry in the extension's OpenWithProgids list.
        # REG_NONE with no data, which is the form the shell documents and the
        # form that carries no meaning beyond "this ProgId can open this".
        Add-Act $Plan @{ Kind='reg'; Key="$extKey\OpenWithProgids"; Name=$progId; Data=([byte[]]@()); Type=$N; Value='(listed in Open with)'; Record='value' }

        if ($t.Default) {
            Add-Act $Plan @{ Kind='reg'; Key=$extKey; Name=''; Data=$progId; Type=$S; Value="$progId  (default handler; Polylinker defines this format)"; Record='value' }
        }

        # Not mutations -- these write nothing. They only record that if these
        # two keys are empty once our values are removed, the empty husks may go
        # too. `.plproj` will be empty because nothing else claims it; `.dna` on
        # a machine with SnapGene will not be, and must survive.
        Add-Act $Plan @{ Kind='regcleanup'; Key="$extKey\OpenWithProgids" }
        Add-Act $Plan @{ Kind='regcleanup'; Key=$extKey }
    }
}

function Update-ShellAssociations {
    # Tell Explorer the class registrations changed, so the effect is visible
    # without a sign-out. Best effort: if it fails, the associations are still
    # correct and appear after the next logon.
    try {
        if (-not ('PL.Shell32' -as [type])) {
            Add-Type -Namespace PL -Name Shell32 -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("shell32.dll")]
public static extern void SHChangeNotify(int eventId, uint flags, System.IntPtr item1, System.IntPtr item2);
'@
        }
        [PL.Shell32]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)  # SHCNE_ASSOCCHANGED
    } catch { }
}

function Update-EnvironmentBroadcast {
    # Same idea for PATH: broadcast WM_SETTINGCHANGE so shells started from now
    # on pick it up. Already-open terminals keep their old PATH regardless --
    # that is Windows, not this script, and the caller says so.
    try {
        if (-not ('PL.User32' -as [type])) {
            Add-Type -Namespace PL -Name User32 -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Auto)]
public static extern System.IntPtr SendMessageTimeout(System.IntPtr hWnd, uint Msg, System.IntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out System.UIntPtr lpdwResult);
'@
        }
        $r = [UIntPtr]::Zero
        [PL.User32]::SendMessageTimeout([IntPtr]0xFFFF, 0x1A, [IntPtr]::Zero, 'Environment', 2, 5000, [ref]$r) | Out-Null
    } catch { }
}

# ---------------------------------------------------------------------------
# Uninstall
# ---------------------------------------------------------------------------

function Read-Receipt {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "no install receipt at $Path. This uninstaller removes exactly what an install recorded, so without the receipt it will not guess. If Polylinker was installed somewhere else, pass -Prefix."
    }
    $lines = [System.IO.File]::ReadAllText($Path) -split "`r?`n"
    if ($lines[0] -ne $ReceiptHeader) { throw "$Path is not a Polylinker install receipt" }
    $head = @{}
    $items = [System.Collections.ArrayList]::new()
    $past = $false
    foreach ($l in $lines) {
        if (-not $past) {
            if ($l -eq '--') { $past = $true; continue }
            if ($l -match '^([a-z]+):\s*(.*)$') { $head[$Matches[1]] = $Matches[2] }
            continue
        }
        if ($l) { $items.Add($l) | Out-Null }
    }
    return @{ Head = $head; Items = $items }
}

function Invoke-Uninstall {
    $receiptPath = Join-Path $Prefix $ReceiptName

    # Re-launch out of the directory about to be deleted.
    #
    # Add/Remove Programs runs this script from inside the install directory,
    # and a running script's own file cannot be removed from underneath it. So
    # the first thing an uninstall does is copy itself to TEMP and hand over.
    # The copy is written rather than copied, so it inherits no zone marking.
    if (-not $NoRelaunch -and $PSCommandPath -and
        $PSCommandPath.StartsWith($Prefix, [StringComparison]::OrdinalIgnoreCase)) {
        $tmp = Join-Path $env:TEMP ("polylinker-uninstall-{0}.ps1" -f $PID)
        [System.IO.File]::WriteAllText($tmp, [System.IO.File]::ReadAllText($PSCommandPath), (New-Object System.Text.UTF8Encoding($false)))
        $argv = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $tmp, '-Uninstall', '-NoRelaunch', '-Prefix', $Prefix)
        if ($Yes)          { $argv += '-Yes' }
        if ($RemoveCache)  { $argv += '-RemoveCache' }
        if ($DryRun)       { $argv += '-DryRun' }
        if ($RegistryRoot) { $argv += @('-RegistryRoot', $RegistryRoot) }
        if ($StateDir)     { $argv += @('-StateDir', $StateDir) }
        $p = Start-Process -FilePath (Get-Process -Id $PID).Path -ArgumentList $argv -Wait -PassThru -NoNewWindow
        exit $p.ExitCode
    }

    $r = Read-Receipt $receiptPath
    Head "Uninstalling $AppName $(if ($r.Head.Contains('version')) { $r.Head['version'] } else { '' })"
    Say  "  installed: $(if ($r.Head.Contains('installed')) { $r.Head['installed'] } else { 'unknown' })"
    Say  "  from:      $Prefix"

    Stop-IfRunning

    $plan = New-Plan
    $files = @(); $dirs = @(); $links = @(); $keys = @(); $paths = @(); $values = @(); $husks = @()
    foreach ($i in $r.Items) {
        $kind, $rest = $i -split ' ', 2
        switch ($kind) {
            'file'          { $files  += $rest }
            'dir'           { $dirs   += $rest }
            'shortcut'      { $links  += $rest }
            'regkey'        { $keys   += $rest }
            'regvalue'      { $values += $rest }
            'regkeyifempty' { $husks  += $rest }
            'pathentry'     { $paths  += $rest }
        }
    }
    foreach ($f in $links) { Add-Act $plan @{ Kind='rmfile'; Path=$f } }
    foreach ($f in $files) { Add-Act $plan @{ Kind='rmfile'; Path=$f } }
    # Deepest first, so a parent is only considered once its children are gone.
    foreach ($d in ($dirs | Sort-Object -Property Length -Descending)) { Add-Act $plan @{ Kind='rmdir'; Path=$d } }
    foreach ($k in ($keys | Sort-Object -Unique)) { Add-Act $plan @{ Kind='rmkey'; Key=$k } }
    foreach ($v in ($values | Sort-Object -Unique)) {
        $k, $n = $v -split '\|', 2
        Add-Act $plan @{ Kind='rmvalue'; Key=$k; Name=$n }
    }
    # After the values, and longest first so `.dna\OpenWithProgids` is considered
    # before `.dna` -- the parent can only be empty once the child is gone.
    #
    # De-duplicated FIRST and sorted second, in two passes. `Sort-Object -Unique
    # -Property Length` applies the uniqueness to the SORT KEY, not to the
    # values, so it kept one key per distinct string length: `.gb` and `.fa` are
    # both the same length, so one of them silently vanished from the cleanup
    # list and its empty husk survived the uninstall. Found by asserting the
    # husk was gone, which is the only reason it was found at all.
    $husks = @($husks | Sort-Object -Unique) | Sort-Object -Property Length -Descending
    foreach ($h in $husks) {
        Add-Act $plan @{ Kind='rmkeyifempty'; Key=$h }
    }
    foreach ($p in $paths) {
        $k, $e = $p -split '\|', 2
        Add-Act $plan @{ Kind='rmpath'; Key=$k; Entry=$e }
    }

    $cacheDir = Join-Path $StateDir 'index'
    $cacheBytes = 0
    if (Test-Path -LiteralPath $cacheDir) {
        $cacheBytes = (Get-ChildItem -LiteralPath $cacheDir -Recurse -File -ErrorAction SilentlyContinue |
                       Measure-Object -Property Length -Sum).Sum
        if (-not $cacheBytes) { $cacheBytes = 0 }
    }
    if ($RemoveCache -and (Test-Path -LiteralPath $cacheDir)) {
        Add-Act $plan @{ Kind='rmdir'; Path=$cacheDir }
    }

    Head 'This will remove:'
    Show-Plan $plan

    Head 'This will KEEP, because it is yours and not ours:'
    Say  "  $StateDir"
    if (Test-Path -LiteralPath $StateDir) {
        $lay = Join-Path $StateDir 'layout'
        $rec = Join-Path $StateDir 'recovery'
        if (Test-Path -LiteralPath $lay) { Say '    layout      your window and track settings' }
        if (Test-Path -LiteralPath $rec) {
            $n = @(Get-ChildItem -LiteralPath $rec -Filter '*.recover' -ErrorAction SilentlyContinue).Count
            Say ("    recovery\   {0} unsaved crash draft(s)" -f $n)
        }
        if ($cacheBytes -gt 0) {
            if ($RemoveCache) {
                Say ("    index\      {0:N1} MB library cache -- WILL BE DELETED (-RemoveCache)" -f ($cacheBytes / 1MB))
            } else {
                Say ("    index\      {0:N1} MB library cache. It regenerates; pass -RemoveCache to delete it." -f ($cacheBytes / 1MB))
            }
        }
    }

    if ($DryRun) { Head 'Dry run: nothing was changed.'; return 0 }
    Confirm-Or-Stop 'remove Polylinker'

    $problems = @()
    foreach ($a in $plan) {
        try {
            switch ($a.Kind) {
                'rmfile' { if (Test-Path -LiteralPath $a.Path) { Remove-Item -LiteralPath $a.Path -Force } }
                'rmdir'  { if (Test-Path -LiteralPath $a.Path) { Remove-Item -LiteralPath $a.Path -Recurse -Force } }
                'rmkey'  {
                    $pp = RegProvider $a.Key
                    if (Test-Path -LiteralPath $pp) { Remove-Item -LiteralPath $pp -Recurse -Force }
                }
                'rmvalue' {
                    $pp = RegProvider $a.Key
                    if (Test-Path -LiteralPath $pp) {
                        # An empty name is the key's own (Default) value, which
                        # Remove-ItemProperty addresses as '(default)'.
                        $n = if ($a.Name) { $a.Name } else { '(default)' }
                        Remove-ItemProperty -LiteralPath $pp -Name $n -Force -ErrorAction SilentlyContinue
                    }
                }
                'rmkeyifempty' {
                    $pp = RegProvider $a.Key
                    if (Test-Path -LiteralPath $pp) {
                        $k = Get-Item -LiteralPath $pp
                        # `ValueCount` counts a (Default) that was set; both it
                        # and SubKeyCount must be zero. On `.dna` with SnapGene
                        # installed neither is, and the key stays.
                        if ($k.SubKeyCount -eq 0 -and $k.ValueCount -eq 0) {
                            Remove-Item -LiteralPath $pp -Force
                        }
                    }
                }
                'rmpath' {
                    $cur = Get-RegValueRaw -Key $a.Key -Name 'Path'
                    if ($cur) {
                        $new = Remove-PathEntryFrom $cur.Value $a.Entry
                        if ($null -ne $new) { Set-RegValueRaw -Key $a.Key -Name 'Path' -Value $new -Kind $cur.Kind }
                    }
                }
            }
        } catch {
            $problems += ("{0} {1}: {2}" -f $a.Kind, $(if ($a.Contains('Path')) { $a.Path } else { $a.Key }), $_.Exception.Message)
        }
    }
    # The ARP entry is removed last: while anything else remains, the entry is
    # the only way a user finds this uninstaller again.
    $arpP = RegProvider $ArpKey
    if (Test-Path -LiteralPath $arpP) { Remove-Item -LiteralPath $arpP -Recurse -Force }

    Update-EnvironmentBroadcast
    Update-ShellAssociations

    Head 'Removed.'
    if ($problems) {
        Warn 'These could not be removed:'
        foreach ($p in $problems) { Warn "  $p" }
        Warn "Delete $Prefix by hand once nothing is using it."
    }
    Say "Your settings and any crash drafts are still in $StateDir."
    return 0
}

function Stop-IfRunning {
    <#
        Refuse while the app is running, and say which process and which PID.

        Windows will not replace a file that is mapped into a running process,
        so a half-finished upgrade is the alternative to this check -- and "the
        installer failed, some files were replaced" is a worse state than "close
        Polylinker and run this again".
    #>
    $live = @()
    foreach ($n in 'polylinker', 'pl', 'pl-mcp') {
        foreach ($p in @(Get-Process -Name $n -ErrorAction SilentlyContinue)) {
            $path = $null
            try { $path = $p.Path } catch { }
            if ($path -and $path.StartsWith($Prefix, [StringComparison]::OrdinalIgnoreCase)) {
                $live += ("{0} (PID {1})" -f $p.ProcessName, $p.Id)
            }
        }
    }
    if ($live) {
        throw ("Polylinker is running from $Prefix and its files cannot be replaced while it is:`n    {0}`nClose it and run this again." -f ($live -join "`n    "))
    }
}

# ---------------------------------------------------------------------------
# Unassociate
# ---------------------------------------------------------------------------

function Invoke-Unassociate {
    Head 'Removing file associations'
    $removed = 0
    foreach ($t in $FileTypes) {
        $pid_ = ProgIdFor $t.Ext
        $k = RegProvider "$ClassesKey\$pid_"
        if (Test-Path -LiteralPath $k) {
            if (-not $DryRun) { Remove-Item -LiteralPath $k -Recurse -Force }
            Say "  registry    delete $ClassesKey\$pid_"
            $removed++
        }
        $ow = RegProvider "$ClassesKey\$($t.Ext)\OpenWithProgids"
        if (Test-Path -LiteralPath $ow) {
            $props = Get-ItemProperty -LiteralPath $ow -ErrorAction SilentlyContinue
            if ($props -and $props.PSObject.Properties.Name -contains $pid_) {
                if (-not $DryRun) { Remove-ItemProperty -LiteralPath $ow -Name $pid_ -Force -ErrorAction SilentlyContinue }
                Say "  registry    delete $ClassesKey\$($t.Ext)\OpenWithProgids\$pid_"
                $removed++
            }
        }
    }
    $app = RegProvider "$ClassesKey\Applications\polylinker.exe"
    if (Test-Path -LiteralPath $app) {
        if (-not $DryRun) { Remove-Item -LiteralPath $app -Recurse -Force }
        Say "  registry    delete $ClassesKey\Applications\polylinker.exe"
        $removed++
    }
    if (-not $DryRun) { Update-ShellAssociations }
    if ($removed -eq 0) { Say '  nothing to remove; no Polylinker associations are registered.' }
    elseif ($DryRun)    { Head 'Dry run: nothing was changed.' }
    else                { Head 'Done. Whatever opened these files before still does.' }
    return 0
}

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------

function Invoke-Install {
    if ($AllUsers) {
        $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
                 ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
        if (-not $admin) {
            throw "-AllUsers writes to Program Files and to HKLM, which needs an elevated session. Open Terminal or PowerShell as administrator and run it there. Without -AllUsers this installs to your own profile and needs no elevation at all."
        }
        if ($Associate) {
            # A machine-wide install is being run by an administrator on behalf
            # of other people, and "ask, don't take" cannot be satisfied by
            # asking the wrong person. Ten users' Open-with menus are not the
            # deploying admin's to answer for.
            throw "-Associate is not available with -AllUsers. A per-machine install must not answer a per-user question for everybody on the machine; each user can run this script with -Associate for themselves."
        }
    }

    $manifest = Read-Manifest $Source
    $stats = Test-Payload -Dir $Source -Manifest $manifest
    $version = if ($manifest.Version) { $manifest.Version } else { 'unknown' }

    $exe = Join-Path $Prefix 'polylinker.exe'
    $iconFile = Join-Path $Prefix 'polylinker.ico'
    $hasIcon = $manifest.Files.Contains('polylinker.ico')
    $icon = if ($hasIcon) { $iconFile } else { $null }

    $existing = $null
    $receiptPath = Join-Path $Prefix $ReceiptName
    if (Test-Path -LiteralPath $receiptPath) {
        $existing = (Read-Receipt $receiptPath).Head
    }

    Head "$AppName $version"
    Say  "  scope:   $scope"
    Say  "  from:    $Source"
    Say  "  to:      $Prefix"
    Say  ("  payload: {0} file(s), {1:N1} MB, every one checked against SHA256SUMS.txt" -f $stats.Count, ($stats.Bytes / 1MB))
    if ($manifest.Commit) { Say "  commit:  $($manifest.Commit)" }
    if ($existing) {
        $was = if ($existing.Contains('version')) { $existing['version'] } else { 'unknown' }
        Say  ("  upgrade: {0} -> {1}, over the existing install" -f $was, $version)
    }
    if (-not $hasIcon) {
        Warn '  no icon: this build ships no polylinker.ico, so Windows will use its generic'
        Warn '           application icon for the Start Menu entry. Cosmetic only.'
    }

    if ($existing) { Stop-IfRunning }

    $plan = New-Plan
    Add-Act $plan @{ Kind='mkdir'; Path=$Prefix }

    # Copy EVERYTHING in the source. Not a curated subset: a curated subset is a
    # second file list, and a second file list is what drops a licence text.
    $dirsSeen = @{}
    foreach ($name in $manifest.Files.Keys) {
        # The manifest speaks forward slashes; the receipt, the plan and every
        # path the user is about to read should speak Windows.
        $native = $name.Replace('/', [System.IO.Path]::DirectorySeparatorChar)
        $from = Join-Path $Source $native
        $dest = Join-Path $Prefix $native
        $d = Split-Path -Parent $dest
        if ($d -ne $Prefix -and -not $dirsSeen.Contains($d)) {
            $dirsSeen[$d] = $true
            Add-Act $plan @{ Kind='mkdir'; Path=$d }
        }
        Add-Act $plan @{ Kind='copy'; From=$from; To=$dest; Bytes=(Get-Item -LiteralPath $from).Length }
    }
    # And the manifest itself, which by construction is not in its own list. It
    # goes with them so an installed copy can still be checked months later --
    # `Get-FileHash` against SHA256SUMS.txt in the install directory answers "is
    # this still what I installed?", which is a question an unsigned build gets
    # asked more often than a signed one.
    $manifestSrc = Join-Path $Source 'SHA256SUMS.txt'
    Add-Act $plan @{ Kind='copy'; From=$manifestSrc; To=(Join-Path $Prefix 'SHA256SUMS.txt'); Bytes=(Get-Item -LiteralPath $manifestSrc).Length }

    $uninstallCmdPath = Join-Path $Prefix 'Uninstall.cmd'
    $uninstallCmd = @"
@echo off
rem Removes Polylinker. The same thing the Settings -> Apps entry runs.
rem
rem The `& exit` matters: cmd.exe reads a batch file one LINE at a time and
rem seeks back to it between lines, so a batch file that deletes itself must
rem finish its work and exit within a single line or cmd reports that the batch
rem file cannot be found. The uninstaller re-launches itself out of %TEMP% and
rem then deletes this folder, including this file.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-Polylinker.ps1" -Uninstall %* & exit /b
"@
    Add-Act $plan @{ Kind='write'; Path=$uninstallCmdPath; Content=$uninstallCmd; Note='the uninstall entry point' }

    # Start Menu. One shortcut, loose, no folder of its own: a folder containing
    # a single entry is a folder nobody wanted.
    $startMenu = if ($StartMenuDir) {
        # The gate passes this. A round-trip test that leaves a shortcut in a
        # developer's real Start Menu is a test nobody runs twice.
        if (-not (Test-Path -LiteralPath $StartMenuDir)) { New-Item -ItemType Directory -Force -Path $StartMenuDir | Out-Null }
        $StartMenuDir
    } elseif ($AllUsers) {
        Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
    } else {
        Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
    }
    Add-Act $plan @{
        Kind='shortcut'; Path=(Join-Path $startMenu "$AppName.lnk"); Target=$exe
        Description='Offline plasmid editor'; Icon=$icon
    }

    # Add/Remove Programs. Without this an installer is a folder somebody has to
    # remember they created.
    $S = [Microsoft.Win32.RegistryValueKind]::String
    $D = [Microsoft.Win32.RegistryValueKind]::DWord
    $uninstallString = '"{0}"' -f $uninstallCmdPath
    $arp = @(
        @{ N='DisplayName';     V=$AppName;                                        T=$S }
        @{ N='DisplayVersion';  V=$version;                                        T=$S }
        @{ N='Publisher';       V=$AppName;                                        T=$S }
        @{ N='InstallLocation'; V=$Prefix;                                         T=$S }
        @{ N='UninstallString'; V=$uninstallString;                                T=$S }
        @{ N='QuietUninstallString'; V=($uninstallString + ' -Yes');               T=$S }
        @{ N='InstallDate';     V=(Get-Date -Format 'yyyyMMdd');                   T=$S }
        @{ N='NoModify';        V=1;                                               T=$D }
        @{ N='NoRepair';        V=1;                                               T=$D }
        @{ N='EstimatedSize';   V=([int]($stats.Bytes / 1KB));                     T=$D }
    )
    if ($icon) { $arp += @{ N='DisplayIcon'; V=$icon; T=$S } }
    # Only the first act records the key, because they all write into the same
    # one and deleting it once is enough.
    $first = $true
    foreach ($v in $arp) {
        $act = @{ Kind='reg'; Key=$ArpKey; Name=$v.N; Data=$v.V; Type=$v.T; Value=$v.V }
        if ($first) { $act.Record = 'key'; $first = $false }
        Add-Act $plan $act
    }

    # PATH, opt-in.
    if ($AddToPath) {
        $cur = Get-RegValueRaw -Key $EnvKey -Name 'Path'
        $curVal  = if ($cur) { $cur.Value } else { '' }
        $curKind = if ($cur) { $cur.Kind } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
        $new = Add-PathEntryTo $curVal $Prefix
        if ($null -eq $new) {
            Say "  PATH        already contains $Prefix; nothing to do"
        } elseif ($new.Length -gt $PathMax) {
            throw ("adding $Prefix would make your PATH {0} characters, past the {1}-character limit where older tools start truncating it. Install without -AddToPath and either call `"$Prefix\pl.exe`" by its full path, or shorten your PATH first." -f $new.Length, $PathMax)
        } else {
            Add-Act $plan @{ Kind='path'; Key=$EnvKey; Entry=$Prefix; New=$new; Type=$curKind }
        }
    }

    Head 'This will:'
    Show-Plan $plan

    # Associations: asked separately, and last, so the answer is not swept up in
    # the answer to everything else.
    $doAssoc = $false
    if ($Associate) {
        $doAssoc = $true
    } elseif (-not $Yes -and -not $DryRun) {
        $dnaOwner = Get-CurrentHandler '.dna'
        Head 'File types (optional)'
        if ($dnaOwner) {
            Say "  $dnaOwner will keep opening .dna files."
        } else {
            Say '  Nothing currently opens .dna files.'
        }
        Say '  Polylinker can be ADDED to the "Open with" menu for .dna, .gb, .gbk,'
        Say '  .genbank, .fasta, .fa, .fna and .ab1. Nothing is taken over; the'
        Say '  default handler for each of those is left exactly as it is.'
        Write-Host '  Add Polylinker to "Open with"? [y/N]: ' -NoNewline -ForegroundColor Cyan
        $a = Read-Host
        $doAssoc = ($a.Trim().ToLower() -eq 'y')
    }
    if ($doAssoc) {
        $assocPlan = New-Plan
        Add-AssociationActs $assocPlan $exe $icon
        Head 'And register these file types (additive; no default is changed):'
        Show-Plan $assocPlan
        foreach ($a in $assocPlan) { Add-Act $plan $a }
    }

    # The negative half of the plan, printed AFTER the association question so
    # that it can tell the truth about the answer. It used to be printed before,
    # where it cheerfully promised not to change any file association and was
    # then followed by thirty lines of file associations.
    Head 'It will NOT:'
    Say '  contact the network -- not to check for a version, not for anything else'
    # "install an updater" is the substring tools/ci.ps1 requires in this plan,
    # so it stays; the clause after the dash is what stops it being read as a
    # promise the product no longer keeps. `pl update` exists and the editor has
    # an off-by-default check, but neither is INSTALLED -- they are part of two
    # programs already in the folder, and neither ever runs unasked.
    Say '  install an updater, a service or a scheduled task -- nothing this puts on the machine ever runs on its own'
    Say '  touch anything outside the paths listed above'
    if (-not $doAssoc) {
        Say '  register any file type, or change what opens anything'
    } else {
        Say '  change the default handler for any file type; the entries above are additive'
    }
    if (-not $AddToPath) {
        Say '  put pl.exe on your PATH  (pass -AddToPath if you want that)'
    }

    if ($DryRun) { Head 'Dry run: nothing was changed.'; return 0 }
    Confirm-Or-Stop 'install Polylinker'

    $receipt = Invoke-Plan $plan

    # The receipt, written last, because until it exists nothing has been
    # recorded and an uninstall would have to guess.
    $lines = @(
        $ReceiptHeader
        "version: $version"
        "commit: $($manifest.Commit)"
        "installed: $(Get-Date -Format 'yyyy-MM-ddTHH:mm:ss')"
        "scope: $scope"
        "prefix: $Prefix"
        "state: $StateDir"
        '--'
    ) + $receipt + @("file $receiptPath")
    [System.IO.File]::WriteAllText($receiptPath, (($lines -join "`r`n") + "`r`n"), (New-Object System.Text.UTF8Encoding($false)))

    if ($AddToPath) { Update-EnvironmentBroadcast }
    if ($doAssoc)   { Update-ShellAssociations }

    Head 'Installed.'
    Say  "  Start Menu:  $AppName"
    Say  "  Files:       $Prefix"
    Say  "  Receipt:     $receiptPath  (every path and registry value this wrote)"
    Say  "  Uninstall:   Settings -> Apps, or $uninstallCmdPath"
    if ($AddToPath) {
        Say "  PATH:        `pl` works in shells opened from now on; already-open ones keep the old PATH."
    }
    Say ''
    Warn 'This build is not code-signed. The first time you run polylinker.exe,'
    Warn 'Windows may show "Windows protected your PC". See README-WINDOWS.txt for'
    Warn 'what that means and how to check this copy is the one that was published.'
    return 0
}

# ---------------------------------------------------------------------------

try {
    if ($Uninstall)        { exit (Invoke-Uninstall) }
    elseif ($Unassociate)  { exit (Invoke-Unassociate) }
    else                   { exit (Invoke-Install) }
} catch {
    Write-Host ''
    Write-Host $_.Exception.Message -ForegroundColor Red
    Write-Host ''
    exit 1
}
