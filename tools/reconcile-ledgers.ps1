<#
.SYNOPSIS
    Compare the step ledgers `tools/ci.ps1 -Ledger` wrote on each platform.

.DESCRIPTION
    THE QUESTION NO SINGLE LEG CAN ANSWER: was every step in the gate observed
    to RUN, somewhere?

    `tools/ci.ps1` checks, on the machine it is running on, that every skip
    carries a declared reason and that a platform reason agrees with the
    platform. Neither of those can see the case that matters most once the gate
    runs in three places: a step that skips on ALL THREE. Under the old
    single-runner design that case did not exist -- there was one leg and a skip
    on it was a skip, full stop. Under three legs, 'not windows' on two of them
    is only honest if the third one ran it, and nothing inside a leg knows what
    the other two did.

    That is the same defect this project has now recorded several times in
    different clothes: a check that cannot fail. A step made conditional at file
    level, or deleted on one platform, or excused everywhere by a precondition
    that returns a reason on every branch, is green on every leg and green in
    aggregate.

    Four rules, over the ledgers themselves rather than over any list:

      X1  Three ledgers, one per platform, carrying the SAME step names in the
          SAME order. Catches a step deleted or made conditional at file level
          on one platform, and catches two legs of the same platform being
          uploaded by mistake.
      X2  Every step RAN on at least one platform. This is the anti-gaming rule.
      X3  The steps that skipped for want of a corpus are the same set on all
          three legs. A corpus step that ran on one runner means somebody put
          lab plasmids on a runner, which is news; a corpus step skipping on one
          leg and not the others means the legs were not given the same inputs.
      X4  Every declared reason is in the vocabulary, and 'not windows' appears
          on the two non-Windows legs and on neither anything else.

.PARAMETER Ledger
    The ledger files, one per platform. Three are expected.

.PARAMETER LedgerDir
    A directory to search recursively for `ledger.tsv`, which is what
    `actions/download-artifact` produces: one subdirectory per uploaded
    artifact, each holding a file of that name.

    THIS EXISTS BECAUSE `-Ledger` CANNOT BE PASSED THROUGH `pwsh -File`.
    Under `-File`, PowerShell binds an array parameter to the FIRST argument
    only and then rejects the rest with "a positional parameter cannot be
    found" -- measured, with three real ledgers, before this parameter was
    added. The workflow uses `pwsh -File` deliberately, for the exit-code path
    its own two control steps prove, so the fix belongs here and not there.

.PARAMETER SelfTest
    Plant a ledger set violating each rule in turn and assert that each is
    caught. A reconciler that has stopped matching reports the same clean as a
    clean set of ledgers, and this file is the only thing standing behind X2.

.EXAMPLE
    ./tools/reconcile-ledgers.ps1 -Ledger a.tsv b.tsv c.tsv
    ./tools/reconcile-ledgers.ps1 -LedgerDir ledgers
    ./tools/reconcile-ledgers.ps1 -SelfTest
#>
[CmdletBinding()]
param([string[]]$Ledger, [string]$LedgerDir, [switch]$SelfTest)

# The same two words `tools/ci.ps1` allows a precondition to return. Repeated
# here rather than imported, because this script runs in a different job on a
# different machine from any leg and has no ci.ps1 beside it to dot-source; X4
# below is what makes the duplication safe, since a reason ci.ps1 emits and this
# file does not know is a failure rather than silence.
$Vocabulary = @('not windows', 'corpus')
$Platforms = @('linux', 'macos', 'windows')

function Read-Ledger([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { throw "no ledger at $Path" }
    $platform = $null
    $rows = @()
    foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
        if (-not $line) { continue }
        if ($line.StartsWith('#')) {
            if ($line -match '^#\s*platform\t(\S+)$') { $platform = $Matches[1] }
            continue
        }
        # A row is name, state, reason. The third field is empty for a step
        # that ran, and `-split` keeps it, so `$f.Count` is 3 either way -- but
        # it is read defensively below rather than indexed blind, because a
        # truncated upload is a thing that happens and reading past the end
        # would give $null and be mistaken for "ran".
        $f = $line -split "`t"
        if ($f.Count -lt 2) { throw "$Path has a row with $($f.Count) field(s): $line" }
        $rows += [pscustomobject]@{
            Name   = $f[0]
            State  = $f[1]
            Reason = if ($f.Count -ge 3) { $f[2] } else { '' }
        }
    }
    if (-not $platform) { throw "$Path declares no platform; ci.ps1 writes it as the first line" }
    if ($rows.Count -eq 0) { throw "$Path names no steps at all" }
    return [pscustomobject]@{ Path = $Path; Platform = $platform; Rows = $rows }
}

function Test-Ledgers {
    param($Ledgers)
    $problems = @()

    # X1a: one ledger per platform, and all three platforms.
    $seen = @($Ledgers | ForEach-Object { $_.Platform } | Sort-Object)
    if (($seen -join ',') -ne ($Platforms -join ',')) {
        $problems += ("the ledgers cover [$($seen -join ', ')] and should cover exactly " +
                      "[$($Platforms -join ', ')]. A missing leg is a leg that did not report, " +
                      'and a duplicate is two runs of one platform standing in for three.')
        # Everything below compares the legs against each other, and with the
        # wrong set of legs those comparisons would describe a fiction.
        return $problems
    }

    # X1b: the same names in the same order.
    $ref = $Ledgers[0]
    foreach ($l in $Ledgers[1..($Ledgers.Count - 1)]) {
        $a = @($ref.Rows | ForEach-Object { $_.Name })
        $b = @($l.Rows | ForEach-Object { $_.Name })
        if ($a.Count -ne $b.Count) {
            $problems += ("$($l.Platform) ran $($b.Count) step(s) and $($ref.Platform) ran $($a.Count). " +
                          'The gate is one list; a leg with a different number of steps has had one made ' +
                          'conditional at file level or deleted.')
            continue
        }
        for ($i = 0; $i -lt $a.Count; $i++) {
            if ($a[$i] -cne $b[$i]) {
                $problems += ("step $($i + 1) is `"$($a[$i])`" on $($ref.Platform) and `"$($b[$i])`" on " +
                              "$($l.Platform); the legs are not running the same gate")
                break
            }
        }
    }

    # X4: reasons, and the platform they claim.
    foreach ($l in $Ledgers) {
        foreach ($row in $l.Rows) {
            if ($row.State -eq 'ran') {
                if ($row.Reason) { $problems += "$($l.Platform): `"$($row.Name)`" ran and carries a reason" }
                continue
            }
            if ($row.State -ne 'skipped') {
                $problems += "$($l.Platform): `"$($row.Name)`" has state `"$($row.State)`", which is neither ran nor skipped"
                continue
            }
            if (-not $row.Reason) {
                $problems += ("$($l.Platform): `"$($row.Name)`" skipped with no declared reason. That is a " +
                              'failure inside the leg itself under -ExpectedSkips, so a ledger carrying one ' +
                              'means the leg was run without it.')
                continue
            }
            if ($Vocabulary -notcontains $row.Reason) {
                $problems += "$($l.Platform): `"$($row.Name)`" skipped with the unknown reason `"$($row.Reason)`""
                continue
            }
            if ($row.Reason -eq 'not windows' -and $l.Platform -eq 'windows') {
                $problems += "windows: `"$($row.Name)`" skipped saying 'not windows'"
            }
        }
    }

    # X3: the corpus set is the same everywhere.
    $corpusSets = @{}
    foreach ($l in $Ledgers) {
        $corpusSets[$l.Platform] = @($l.Rows |
            Where-Object { $_.State -eq 'skipped' -and $_.Reason -eq 'corpus' } |
            ForEach-Object { $_.Name } | Sort-Object) -join '|'
    }
    $distinct = @($corpusSets.Values | Sort-Object -Unique)
    if ($distinct.Count -gt 1) {
        $problems += ("the corpus skips differ between legs: " +
                      (@($corpusSets.Keys | Sort-Object | ForEach-Object {
                          "$_ = [$($corpusSets[$_] -replace '\|', ', ')]" }) -join '; ') +
                      '. Either a runner has been given lab plasmids, or the legs were not given the same inputs.')
    }

    return $problems
}

# X2: EVERY STEP RAN ON AT LEAST ONE PLATFORM. THE ANTI-GAMING RULE, and the
# only one of the four that no leg could possibly check for itself.
#
# IT EXEMPTS THE CORPUS STEPS, and says so out loud rather than quietly not
# checking them. Those five run on no runner on any platform, by design and for
# a legal reason -- `.github/ci-expected-skips.txt` sets it out -- so requiring
# them to have run somewhere would be requiring CI to hold files it must not
# hold. X3 covers them instead: the same five on all three legs, or the
# difference is reported.
function Test-RanSomewhere {
    param($Ledgers)
    $problems = @()
    $exempt = @{}
    $ranSomewhere = @{}
    foreach ($l in $Ledgers) {
        foreach ($row in $l.Rows) {
            if ($row.Reason -eq 'corpus') { $exempt[$row.Name] = $true }
            if ($row.State -eq 'ran') { $ranSomewhere[$row.Name] = $true }
        }
    }
    $names = @()
    foreach ($l in $Ledgers) { foreach ($row in $l.Rows) { if ($names -notcontains $row.Name) { $names += $row.Name } } }
    foreach ($name in $names) {
        if ($exempt.Contains($name) -or $ranSomewhere.Contains($name)) { continue }
        $where = @($Ledgers | ForEach-Object {
            $r = @($_.Rows | Where-Object { $_.Name -eq $name })[0]
            $why = if ($r) { $r.Reason } else { 'absent' }
            "$($_.Platform): $why"
        })
        $problems += ("`"$name`" ran on NO platform -- $($where -join '; '). A step that skips everywhere is a " +
                      'step nothing is checking, which is the shape of the defect that let six releases ship ' +
                      'behind a gate no workflow invoked.')
    }
    return [pscustomobject]@{ Problems = $problems; Exempt = $exempt.Count }
}

if ($SelfTest) {
    # THE CONTROL. Each case is a ledger set that must be rejected, and the
    # clean set that must be accepted. Without this, a reconciler whose parser
    # stopped matching would report every push clean.
    # Rows are written `name|state|reason`, because `@(@(..),@(..))` flattens in
    # PowerShell and an array of triples silently becomes a list of strings --
    # which is how the first version of this self-test reported a step called
    # "a ran " with an empty state.
    function Make([string[]]$Rows, [string]$Platform) {
        [pscustomobject]@{ Path = "<$Platform>"; Platform = $Platform; Rows = @($Rows | ForEach-Object {
            $f = $_ -split '\|'
            [pscustomobject]@{ Name = $f[0]; State = $f[1]; Reason = $f[2] } }) }
    }
    # `b` is the Windows-only step: it runs on Windows and declares 'not
    # windows' on the other two. `c` is the corpus step, which skips everywhere.
    $clean = {
        @(
            (Make @('a|ran|', 'b|skipped|not windows', 'c|skipped|corpus') 'linux'),
            (Make @('a|ran|', 'b|skipped|not windows', 'c|skipped|corpus') 'macos'),
            (Make @('a|ran|', 'b|ran|', 'c|skipped|corpus') 'windows')
        )
    }
    # Each case names the message it must produce, not merely "something went
    # wrong": a set that trips a rule for the wrong reason would otherwise count
    # as evidence that the right rule works. `Expect = ''` is the clean set.
    $cases = @(
        @{ Name = 'a clean set'; Expect = '' ; Build = $clean }
        # X2, and it is a BACKSTOP rather than an independently reachable rule
        # with today's two-word vocabulary: on Windows the only declarable
        # reasons are 'corpus' (exempt from X2) and 'not windows' (rejected by
        # X4), so a step that skips everywhere necessarily trips a second rule
        # too. That is worth stating rather than implying otherwise -- what X2
        # adds today is the diagnosis, and what it will add the day the
        # vocabulary grows a 'not linux' is the rule itself.
        @{ Name = 'a step that skipped everywhere'; Expect = 'ran on NO platform'; Build = {
            $l = & $clean
            foreach ($x in $l) { ($x.Rows | Where-Object { $_.Name -eq 'b' })[0].State = 'skipped' }
            ($l[2].Rows | Where-Object { $_.Name -eq 'b' })[0].Reason = ''
            $l } }
        @{ Name = "'not windows' claimed on Windows"; Expect = "skipped saying 'not windows'"; Build = {
            $l = & $clean
            ($l[2].Rows | Where-Object { $_.Name -eq 'b' })[0].State = 'skipped'
            ($l[2].Rows | Where-Object { $_.Name -eq 'b' })[0].Reason = 'not windows'
            $l } }
        @{ Name = 'a step missing from one leg'; Expect = 'has had one made'; Build = {
            $l = & $clean
            $l[1].Rows = @($l[1].Rows | Where-Object { $_.Name -ne 'b' })
            $l } }
        @{ Name = 'a step renamed on one leg'; Expect = 'not running the same gate'; Build = {
            $l = & $clean
            ($l[1].Rows | Where-Object { $_.Name -eq 'b' })[0].Name = 'b-renamed'
            $l } }
        @{ Name = 'only two platforms reporting'; Expect = 'should cover exactly'; Build = {
            $l = & $clean
            @($l[0], $l[1]) } }
        @{ Name = 'two legs of the same platform'; Expect = 'should cover exactly'; Build = {
            $l = & $clean
            $l[1].Platform = 'linux'
            $l } }
        @{ Name = 'a corpus step that ran on one leg'; Expect = 'corpus skips differ between legs'; Build = {
            $l = & $clean
            ($l[0].Rows | Where-Object { $_.Name -eq 'c' })[0].State = 'ran'
            ($l[0].Rows | Where-Object { $_.Name -eq 'c' })[0].Reason = ''
            $l } }
        @{ Name = 'a skip with no reason'; Expect = 'skipped with no declared reason'; Build = {
            $l = & $clean
            ($l[0].Rows | Where-Object { $_.Name -eq 'a' })[0].State = 'skipped'
            $l } }
        @{ Name = 'a skip with an invented reason'; Expect = 'unknown reason'; Build = {
            $l = & $clean
            ($l[0].Rows | Where-Object { $_.Name -eq 'a' })[0].State = 'skipped'
            ($l[0].Rows | Where-Object { $_.Name -eq 'a' })[0].Reason = 'too slow today'
            $l } }
    )
    $bad = 0
    foreach ($case in $cases) {
        $set = & $case.Build
        $found = @(@(Test-Ledgers -Ledgers $set) + @((Test-RanSomewhere -Ledgers $set).Problems) | Sort-Object -Unique)
        $hit = if ($case.Expect) { @($found | Where-Object { $_ -like "*$($case.Expect)*" }).Count -gt 0 }
               else { $found.Count -eq 0 }
        if (-not $hit) {
            $want = if ($case.Expect) { "a problem matching '$($case.Expect)'" } else { 'no problems' }
            Write-Host ("  FAIL  {0}: expected {1}" -f $case.Name, $want) -ForegroundColor Red
            $found | ForEach-Object { Write-Host "        $_" }
            $bad++
        } else {
            Write-Host ("  ok    {0}" -f $case.Name) -ForegroundColor DarkGray
        }
    }
    if ($bad) { Write-Host "SELF-TEST FAILED: $bad case(s)" -ForegroundColor Red; exit 1 }
    Write-Host ("self-test passed ({0} cases)" -f $cases.Count) -ForegroundColor Green
    exit 0
}

if ($LedgerDir) {
    if ($Ledger) { Write-Host '::error::pass -Ledger or -LedgerDir, not both'; exit 1 }
    if (-not (Test-Path -LiteralPath $LedgerDir)) {
        Write-Host "::error::$LedgerDir does not exist, so no leg uploaded anything"
        exit 1
    }
    $Ledger = @(Get-ChildItem -Path $LedgerDir -Recurse -File -Filter 'ledger.tsv' |
                ForEach-Object { $_.FullName } | Sort-Object)
}

# THREE, EXACTLY, AND NAMED HERE RATHER THAN LEFT TO X1. A leg that never
# finished uploads nothing, and "the ledgers cover [linux, macos]" would report
# that as a disagreement between platforms rather than as a job that did not
# report. They are different problems with different fixes.
if (-not $Ledger -or $Ledger.Count -ne 3) {
    Write-Host "::error::reconcile-ledgers.ps1 was given $(@($Ledger).Count) ledger(s); three are expected, one per platform. A missing one means a gate leg did not report at all."
    exit 1
}

$ledgers = @($Ledger | ForEach-Object { Read-Ledger $_ })
$r2 = Test-RanSomewhere -Ledgers $ledgers
$problems = @(@(Test-Ledgers -Ledgers $ledgers) + @($r2.Problems) | Sort-Object -Unique)

# A FLOOR, because the failure mode of a comparison is comparing nothing. Three
# ledgers of one step each would satisfy every rule above.
if ($ledgers[0].Rows.Count -lt 60) {
    $problems += ("each ledger names only $($ledgers[0].Rows.Count) step(s); the gate has over seventy, so " +
                  'these ledgers are not the gate and this reconciliation proved nothing')
}

if ($problems) {
    foreach ($p in $problems) { Write-Host "::error::$p" }
    Write-Host ("RECONCILE FAILED: {0} problem(s)" -f $problems.Count) -ForegroundColor Red
    exit 1
}
Write-Host ("reconciled {0} legs, {1} steps each; every step ran on at least one platform ({2} corpus steps exempt)" -f
            $ledgers.Count, $ledgers[0].Rows.Count, $r2.Exempt) -ForegroundColor Green
exit 0
