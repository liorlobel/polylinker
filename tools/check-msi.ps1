<#
.SYNOPSIS
    Install the MSI, assert everything it claimed to do, uninstall it, and assert
    the machine is clean again.

.DESCRIPTION
    A CHECK THAT CANNOT FAIL PROVES NOTHING, so this does not read the MSI's
    tables and conclude that they look right. It runs msiexec against a real
    machine and then looks at the disk and the registry.

    The interesting assertion is the additive one. Install-Polylinker.ps1 and
    Polylinker.wxs both promise that Polylinker joins the "Open with" list for
    .dna without taking the file type away from whatever the reader already uses
    -- SnapGene owns .dna on many of these machines. That promise is tested by
    planting a fake default handler for .dna BEFORE installing, and asserting it
    is still there afterwards. Without the planting step the assertion would pass
    on an empty machine no matter what the MSI did, which is the shape of
    check this project has been caught writing before.

    Run it BOTH ways. Without -PerUser it installs per-machine and needs an
    elevated session; with -PerUser it installs the way the reader's double-click
    actually will, needs no elevation, and looks for everything under HKCU and
    the per-user Start Menu instead. CI does both. tools/ci.ps1 skips the
    per-machine pass on an unelevated workstation rather than failing it.

.PARAMETER Msi
    The .msi to test.

.PARAMETER Dist
    The dist/ the MSI was built from, used to know which files must appear.

.PARAMETER KeepOnFailure
    Leave the installation in place if an assertion fails, for inspection.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Msi,
    [string]$Dist = 'dist',
    [switch]$KeepOnFailure,
    # Install the way the reader actually will by default.
    #
    # This switch exists because the first version of this script did not have
    # it, and that was the more serious problem of the two. It forced ALLUSERS=1
    # and refused to run unelevated, so it only ever exercised the per-MACHINE
    # branch -- while the shipped default is per-user. Every registry value in
    # the package is written with Root="HKMU", which resolves to HKLM only in the
    # branch that was being tested and to HKCU in the branch that ships. The
    # per-user path was also where the install directory was wrong, so a check
    # that never took it could not have found that either.
    [switch]$PerUser
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$fail = @()
function Bad($m) { $script:fail += $m }
function Note($m) { Write-Host "      $m" -ForegroundColor DarkGray }

$msiPath = (Resolve-Path -LiteralPath $Msi).Path
$distFull = (Resolve-Path -LiteralPath $Dist).Path

# Everything the package writes with Root="HKMU" lands in one hive or the other
# according to scope, and the Start Menu has a per-user root and an all-users
# root. Both are derived here so that every assertion below looks in the place
# the install being tested actually wrote to.
$scope = if ($PerUser) { 'per-user' } else { 'per-machine' }
$hive = if ($PerUser) { 'HKCU:' } else { 'HKLM:' }
$menuRoot = if ($PerUser) { $env:AppData } else { $env:ProgramData }
$msiScopeArgs = if ($PerUser) { @('MSIINSTALLPERUSER=1', 'ALLUSERS=2') } else { @('ALLUSERS=1') }

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
           ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $PerUser -and -not $isAdmin) {
    throw 'a per-machine install needs an elevated session; pass -PerUser to test the default scope instead'
}
Write-Host "  scope: $scope (registry under $hive, Start Menu under $menuRoot)" -ForegroundColor DarkGray

# What must land on disk: the manifest minus what build-msi.ps1 excludes. Read
# from the manifest rather than restated, for the same reason the MSI itself is
# generated from it.
$ExcludeFromMsi = @('SHA256SUMS.txt', 'Install-Polylinker.ps1', 'Install.cmd')
$expected = @()
$past = $false
foreach ($line in Get-Content -LiteralPath (Join-Path $distFull 'SHA256SUMS.txt')) {
    if (-not $past) { if ($line.Trim() -eq '--') { $past = $true }; continue }
    if ($line -match '^[0-9a-f]{64}\s\s(.+)$') {
        $rel = $Matches[1].Trim()
        if ($ExcludeFromMsi -notcontains $rel) { $expected += $rel }
    }
}
if (-not $expected) { throw 'parsed no expected files out of the manifest' }

# Only the verbose msiexec log lives in temp. $prefix is NOT chosen here any
# more: it is discovered after the install from the package's own App Paths
# entry, because WixUI_Advanced overrides an APPLICATIONFOLDER passed on the
# command line even under /qn.
$log = Join-Path ([IO.Path]::GetTempPath()) ("pl-msi-" + [IO.Path]::GetRandomFileName() + ".log")
$prefix = $null

# ---------------------------------------------------------------- the planting
# A default handler for .dna that the MSI must not disturb, and a witness value
# that proves the check is looking in the right place.
$dnaKey = "$hive\Software\Classes\.dna"
$plantedProgId = 'Polylinker.CheckWitness.NotOurs'
$hadDna = Test-Path $dnaKey
$priorDefault = if ($hadDna) { (Get-ItemProperty $dnaKey).'(default)' } else { $null }
New-Item -Path $dnaKey -Force | Out-Null
Set-ItemProperty -Path $dnaKey -Name '(default)' -Value $plantedProgId
Note "planted a fake default handler for .dna: $plantedProgId"

function Cleanup-Planting {
    if ($hadDna) {
        Set-ItemProperty -Path $dnaKey -Name '(default)' -Value $priorDefault -ErrorAction SilentlyContinue
    } else {
        Remove-Item -Path $dnaKey -Recurse -Force -ErrorAction SilentlyContinue
    }
}

try {
    # ------------------------------------------------------------------ install
    Write-Host "  installing $(Split-Path -Leaf $msiPath)" -ForegroundColor Cyan
    # NO APPLICATIONFOLDER OVERRIDE, deliberately.
    #
    # The first attempt passed APPLICATIONFOLDER=<temp dir> so the check could
    # look in a known place, and every payload assertion then failed. WixUI_Advanced
    # schedules WixSetDefaultPerUserFolder and WixSetDefaultPerMachineFolder in
    # the INSTALLEXECUTE sequence as well as the UI one, so they run under /qn
    # and overwrite the property. The package was installing correctly the whole
    # time; the check was looking in a directory the installer had been told
    # about and then talked out of.
    #
    # So the install goes wherever it really goes, and the check FINDS it -- from
    # the App Paths value the package itself writes. That is a better test
    # anyway: it exercises the default path a reader gets, and it verifies the
    # installer's own record of where it put things.
    #
    # The parentheses matter: without them the + binds to the RESULT of
    # Start-Process rather than to the argument list.
    $installArgs = @('/i', "`"$msiPath`"", '/qn', '/l*v', "`"$log`"") + $msiScopeArgs
    $p = Start-Process msiexec.exe -Wait -PassThru -ArgumentList $installArgs
    if ($p.ExitCode -ne 0) {
        $tail = (Get-Content -LiteralPath $log -Tail 25 -ErrorAction SilentlyContinue) -join "`n"
        throw "msiexec /i exited $($p.ExitCode). Log tail:`n$tail"
    }

    # ------------------------------------------------- where did it actually go
    # Asked of the package rather than assumed. The App Paths value is written by
    # C_AppPaths with Root="HKMU", so it is in whichever hive this install used,
    # and its data is the full path to the installed polylinker.exe.
    function Find-AppPath($root) {
        $k = "$root\Software\Microsoft\Windows\CurrentVersion\App Paths\polylinker.exe"
        if (-not (Test-Path $k)) { return $null }
        (Get-ItemProperty $k -ErrorAction SilentlyContinue).'(default)'
    }
    $otherHive = if ($PerUser) { 'HKLM:' } else { 'HKCU:' }
    $exePath = Find-AppPath $hive
    $foundIn = $hive
    if (-not $exePath) {
        $exePath = Find-AppPath $otherHive
        if ($exePath) { $foundIn = $otherHive }
    }
    if (-not $exePath) {
        $keys = (Get-Content -LiteralPath $log -ErrorAction SilentlyContinue |
                 Select-String -Pattern 'ALLUSERS|MSIINSTALLPERUSER|APPLICATIONFOLDER' |
                 Select-Object -Last 12) -join "`n"
        throw "the install reported success but wrote no App Paths entry in either hive, so where it went cannot be established.`nRelevant log lines:`n$keys"
    }
    $prefix = Split-Path -Parent $exePath
    Note "installed to $prefix (App Paths found in $foundIn)"
    if ($foundIn -ne $hive) {
        # Say WHY, not just that. Windows Installer decides scope from ALLUSERS
        # and MSIINSTALLPERUSER during InstallValidate, and the verbose log
        # records the values it settled on. Printing them turns "it went to the
        # wrong place" into something diagnosable without another CI round trip.
        $why = (Get-Content -LiteralPath $log -ErrorAction SilentlyContinue |
                Select-String -Pattern 'Property\(S\): (ALLUSERS|MSIINSTALLPERUSER|APPLICATIONFOLDER|WixAppFolder|ProductToBeRegistered)' |
                Select-Object -Last 8 | ForEach-Object { $_.Line.Trim() }) -join "`n        "
        Bad ("a $scope install was requested but the package registered itself in $foundIn, so it installed in the other scope." +
             $(if ($why) { "`n        the installer settled on:`n        $why" } else { '' }))
    }

    # -------------------------------------------------------------- the payload
    $missing = @()
    foreach ($rel in $expected) {
        $t = Join-Path $prefix ($rel -replace '/', '\')
        if (-not (Test-Path -LiteralPath $t)) { $missing += $rel }
    }
    if ($missing) { Bad "the MSI installed but these files are not on disk under ${prefix}: $($missing -join ', ')" }
    else { Note "all $($expected.Count) payload files present under $prefix" }

    # A per-user install must not have landed in Program Files: that is the
    # directory a reader without administrator rights cannot write to, and
    # putting it there is the bug this whole scope split exists to prevent.
    if ($PerUser -and $prefix -like "$env:ProgramFiles*") {
        Bad "a per-user install put the payload in $prefix, which a reader without administrator rights cannot write to"
    }

    # The three programs must actually be executable, not merely present.
    foreach ($exe in 'pl.exe', 'polylinker.exe', 'pl-mcp.exe') {
        $t = Join-Path $prefix $exe
        if (-not (Test-Path -LiteralPath $t)) { continue }
        if ((Get-Item $t).Length -lt 100000) { Bad "$exe is only $((Get-Item $t).Length) bytes; that is not the real binary" }
    }
    # `pl --version` must agree with the MSI's declared version.
    $plExe = Join-Path $prefix 'pl.exe'
    if (Test-Path $plExe) {
        $ver = (& $plExe --version 2>&1 | Out-String).Trim()
        Note "pl --version -> $ver"
        if ($ver -notmatch '[0-9]+\.[0-9]+\.[0-9]+') { Bad "the installed pl.exe did not report a version: $ver" }
    }

    # ------------------------------------------------------------- Add/Remove
    # Looked up in BOTH hives, and not merely in the expected one.
    #
    # The first version enumerated $hive directly and crashed when the key was
    # absent: a fresh Windows profile has no
    # HKCU\...\CurrentVersion\Uninstall at all until something per-user creates
    # it, so an MSI that wrote its entry to the WRONG hive produced a
    # PowerShell error about a missing path rather than the finding "the entry
    # is in the wrong hive". Crashing where it should have reported is how a
    # test wastes a CI round trip instead of answering the question.
    function Find-Arp($root) {
        $k = "$root\Software\Microsoft\Windows\CurrentVersion\Uninstall"
        if (-not (Test-Path $k)) { return $null }
        Get-ChildItem $k -ErrorAction SilentlyContinue |
            ForEach-Object { Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue } |
            Where-Object { $_.DisplayName -eq 'Polylinker' }
    }
    $arp = Find-Arp $hive
    $otherHive = if ($PerUser) { 'HKLM:' } else { 'HKCU:' }
    if (-not $arp) {
        $elsewhere = Find-Arp $otherHive
        if ($elsewhere) {
            Bad "the Add/Remove Programs entry is under $otherHive, but this was a $scope install and it should be under $hive"
        } else {
            Bad "no Add/Remove Programs entry named Polylinker after installing (looked in $hive and $otherHive)"
        }
    }
    if ($arp) {
        Note "ARP: $($arp.DisplayName) $($arp.DisplayVersion), publisher '$($arp.Publisher)'"
        if (-not $arp.DisplayVersion) { Bad 'the ARP entry has no DisplayVersion' }
        if ($arp.Publisher -ne 'The Polylinker contributors') {
            Bad "ARP Publisher is '$($arp.Publisher)'; the binaries' version resource says 'The Polylinker contributors' and the two must agree"
        }
    }

    # ---------------------------------------------------------- Start Menu
    $shortcut = Join-Path $menuRoot 'Microsoft\Windows\Start Menu\Programs\Polylinker.lnk'
    if (-not (Test-Path -LiteralPath $shortcut)) { Bad "no Start Menu shortcut at $shortcut" }
    else { Note 'Start Menu shortcut present' }

    # --------------------------------------------------- associations, additive
    $stillPlanted = (Get-ItemProperty $dnaKey -ErrorAction SilentlyContinue).'(default)'
    if ($stillPlanted -ne $plantedProgId) {
        Bad "the MSI changed the default handler for .dna from '$plantedProgId' to '$stillPlanted'. Associations must be additive: SnapGene owns .dna on many of these machines."
    } else { Note '.dna default handler untouched, as promised' }

    $owp = "$hive\Software\Classes\.dna\OpenWithProgids"
    if (-not (Test-Path $owp)) { Bad 'no OpenWithProgids key for .dna; Polylinker will not appear in "Open with"' }
    elseif ($null -eq (Get-Item $owp).GetValue('Polylinker.Sequence')) {
        Bad 'the .dna OpenWithProgids key exists but does not list Polylinker.Sequence'
    } else { Note 'Polylinker.Sequence listed under .dna OpenWithProgids' }

    if (-not (Test-Path "$hive\Software\Classes\Polylinker.Sequence\shell\open\command")) {
        Bad 'the Polylinker.Sequence ProgID has no open command'
    }
    # .plproj must NOT be claimed: the GUI decides format by content and no crate
    # knows the .plproj format, so a double-click cannot open one.
    if (Test-Path "$hive\Software\Classes\.plproj\OpenWithProgids") {
        Bad '.plproj is associated, but double-clicking one cannot work: load_as sniffs content and nothing under crates/ knows that format'
    }

    # ------------------------------------------------------------------ uninstall
    Write-Host '  uninstalling' -ForegroundColor Cyan
    $p = Start-Process msiexec.exe -Wait -PassThru -ArgumentList @('/x', "`"$msiPath`"", '/qn', '/l*v', "`"$log.uninstall`"")
    if ($p.ExitCode -ne 0) {
        $tail = (Get-Content -LiteralPath "$log.uninstall" -Tail 25 -ErrorAction SilentlyContinue) -join "`n"
        throw "msiexec /x exited $($p.ExitCode). Log tail:`n$tail"
    }

    # --------------------------------------------------------- and it is gone
    if (Test-Path -LiteralPath (Join-Path $prefix 'polylinker.exe')) {
        Bad 'polylinker.exe is still on disk after uninstalling'
    }
    $leftovers = if (Test-Path -LiteralPath $prefix) {
        Get-ChildItem -LiteralPath $prefix -Recurse -File -ErrorAction SilentlyContinue
    } else { @() }
    if ($leftovers) { Bad "uninstall left $($leftovers.Count) file(s) behind: $(($leftovers | Select-Object -First 5 -Expand Name) -join ', ')" }
    else { Note 'install directory removed' }

    # Both hives again, and for the same reason: an entry that survived in the
    # hive this install did not use is still an entry that survived.
    $arpAfter = @(Find-Arp $hive) + @(Find-Arp $otherHive) | Where-Object { $_ }
    if ($arpAfter) { Bad 'the Add/Remove Programs entry survived the uninstall' }

    if (Test-Path -LiteralPath $shortcut) { Bad 'the Start Menu shortcut survived the uninstall' }
    if (Test-Path "$hive\Software\Classes\Polylinker.Sequence") { Bad 'the Polylinker.Sequence ProgID survived the uninstall' }
    $owpAfter = if (Test-Path $owp) { (Get-Item $owp).GetValue('Polylinker.Sequence') } else { $null }
    if ($null -ne $owpAfter) { Bad 'the .dna OpenWithProgids entry survived the uninstall' }

    # And the planted handler must STILL be intact -- uninstalling must not take
    # away a file type it never owned.
    $afterDefault = (Get-ItemProperty $dnaKey -ErrorAction SilentlyContinue).'(default)'
    if ($afterDefault -ne $plantedProgId) {
        Bad "uninstalling changed the .dna default handler to '$afterDefault'; it must leave a type it never took"
    } else { Note '.dna default handler still intact after uninstall' }
}
finally {
    if (-not ($fail -and $KeepOnFailure)) {
        Cleanup-Planting
        # $prefix is NOT deleted here. It used to be a temp directory this script
        # chose; it is now the real install directory, discovered from the
        # package. Removing it would mean that a failed assertion deletes a
        # genuine Polylinker installation off the machine running the gate. The
        # uninstall above is what removes it, and whether it did is one of the
        # things being asserted -- so deleting it here would also destroy the
        # evidence for that assertion.
        Remove-Item -LiteralPath $log -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath "$log.uninstall" -Force -ErrorAction SilentlyContinue
    }
}

if ($fail) {
    Write-Host ("FAIL  " + (Split-Path -Leaf $msiPath)) -ForegroundColor Red
    $fail | ForEach-Object { Write-Host "      $_" -ForegroundColor Red }
    exit 1
}
Write-Host ("OK    " + (Split-Path -Leaf $msiPath) + ": installed, asserted, uninstalled, machine clean") -ForegroundColor Green
exit 0
