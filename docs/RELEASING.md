# Releasing

## What is done, and what is not

`tools/release.ps1` builds the binaries, records the commit and toolchain, and
writes `SHA256SUMS.txt`. It signs if given an identity and says loudly that it
did not if it was not.

**The builds are unsigned, and I cannot change that.** Signing needs
credentials that are issued to a person or an organisation:

| Platform | What is needed | Roughly | Who must obtain it |
|---|---|---|---|
| Windows | An OV or EV code-signing certificate. EV or an [Azure Trusted Signing] subscription is what actually clears SmartScreen quickly; an OV certificate builds reputation slowly | £200–400/yr, or ~$120/yr for Azure Trusted Signing | Lior, or Bar-Ilan |
| macOS | Apple Developer Program membership, a *Developer ID Application* certificate, and an app-specific password for `notarytool` | $99/yr | Lior |
| Linux | Nothing. A `.tar.gz` with a checksum is the norm | — | — |

[Azure Trusted Signing]: https://learn.microsoft.com/azure/trusted-signing/

Until then, `SHA256SUMS.txt` is the integrity guarantee. Publish it beside the
binaries. It is weaker than a signature — it proves the file matches what the
release page says, not who built it — and saying which of the two you have is
the point.

### What an unsigned build costs the user

Windows SmartScreen shows "Windows protected your PC" on first run and needs
*More info → Run anyway*. macOS Gatekeeper refuses outright and needs a
right-click → Open, or a trip to System Settings. Neither is fatal; both look
exactly like what malware looks like, which is the real cost. An academic tool
asking a labmate to click past a security warning is teaching a bad habit.

## The Windows install path

`tools/release.ps1` produces `dist/polylinker-<version>-windows-x64.zip`. That
zip is the download. Inside it, beside the binaries and the licence texts, are
three files:

| File | What it is |
|---|---|
| `Install-Polylinker.ps1` | the installer, in plain text |
| `Install.cmd` | four lines, so double-clicking works |
| `README-WINDOWS.txt` | verify, read, unblock, install — in that order |

### Why a script and not an MSI

Because there is no certificate, and every argument for a compiled installer
quietly assumes one.

With a signature an MSI is clearly right: Group Policy and Intune consume it, an
administrator gets a product code instead of a detection script, and the "one
person tries it, then asks IT for ten machines" path has a real second half.
**Without a signature that second half does not exist.** An unsigned per-machine
MSI hands the departmental administrator the yellow-banded UAC consent dialog
reading `Publisher: Unknown` — the most alarming thing Windows shows — and lands
in exactly the environments most likely to run an AppLocker or WDAC policy that
refuses unsigned packages outright. The format aimed at IT is the format IT's
policy blocks.

So the question is what to ship in the interval, and it follows from what an
unsigned artifact can still offer. Unsigned-and-opaque is the worst of the
options: an Inno or NSIS stub asks the user to execute several megabytes they
cannot inspect, from a publisher Windows says is unknown, and the only
affordance it offers is the click-through that this document already calls
teaching a bad habit. Unsigned-and-readable is defensible: a zip whose contents
can be listed before anything runs, a checksum that can be checked before
anything is extracted, and an installer that is text.

Readability is the only trust affordance left when the cryptographic one is
unaffordable, and a compiled installer is precisely the choice that discards it.

Three supporting reasons, in case the first one is ever solved and this needs
re-deciding:

1. **The licences travel structurally rather than diligently.** Every compiled
   installer has a second file list — a WiX `<Component>` set, an Inno `[Files]`
   section — that is a hand-maintained copy of `release.ps1`'s `$notices` array.
   The comments in that array are a thirty-line record of exactly that copy
   drifting, twice, on 2026-08-03 and 2026-08-04. The zip has no file list: it
   *is* `dist/`. The obligation becomes impossible to drop rather than
   remembered.
2. **Nothing has to be installed on the build machine.** WiX v6 is a `dotnet
   tool` and there is no .NET SDK here, so it is two network installs; WiX v3 is
   a frozen 30 MB toolchain; Inno is another install; MSIX has its compiler
   already (`makeappx`) and cannot ship unsigned at all. Zip plus PowerShell
   costs nothing.
3. **PowerShell is the language the gate is already written in.** `tools/ci.ps1`
   can exercise the installer in its own language, against a scratch registry
   root and a temp prefix — and it does. An Inno `[Code]` section would be a
   third language the gate cannot reach, holding the PATH and registry surgery
   whose bugs are the destructive kind.

**When a certificate arrives, add an MSI and nothing else.** The `-AllUsers`
layout below is deliberately the layout an MSI would use, so that is a port and
not a redesign.

### What it does

Per-user by default. No elevation, no UAC prompt at any point.

| | Per-user (default) | `-AllUsers` |
|---|---|---|
| Program files | `%LOCALAPPDATA%\Programs\Polylinker` | `C:\Program Files\Polylinker` |
| Start Menu | one `Polylinker.lnk`, no folder | same, in the common menu |
| Uninstall entry | `HKCU\...\Uninstall\Polylinker` | `HKLM\...` |
| PATH (`-AddToPath`) | `HKCU\Environment` | machine environment |
| File associations | opt-in, per-user | **refused** |
| Needs elevation | no | yes, and it will not elevate itself |

The install directory is `%LOCALAPPDATA%\Programs\Polylinker` and deliberately
**not** `%LOCALAPPDATA%\Polylinker`. The latter is the app's own state root —
`recover.rs:243-256` and `pl-scan/src/store.rs:22-36` both land there — and
program files sharing a directory with user state is how an uninstaller ends up
unable to tell them apart.

`-Associate` is refused with `-AllUsers`, on purpose. A machine-wide install is
being run by an administrator on behalf of other people, and "ask, don't take"
cannot be satisfied by asking the wrong person.

Before anything happens the installer prints the complete plan — every file,
every registry value, every PATH change — and waits for a typed `yes`. `-DryRun`
prints the plan and stops.

### What it will not do

* **Contact the network.** Not to check a version, not for anything else. The
  gate greps the installer for `Invoke-WebRequest`, `Invoke-RestMethod`,
  `Start-BitsTransfer`, `System.Net`, `WebClient`, `HttpClient`, `curl.exe`,
  `schtasks`, `Register-ScheduledTask`, `New-Service`, `DownloadFile` and
  `DownloadString`, and fails if any of them appears. The section below is a
  decision; this makes it a fact.
* **Install an updater, a service or a scheduled task.**
* **Take a file association.** See below.
* **Put `pl.exe` on PATH** unless asked with `-AddToPath`.

### File associations

`docs/PLAN.md:212` — *ask, don't take*. It costs nothing to honour, because
since Windows 8 an installer cannot legitimately set a default anyway: the
default lives under `Explorer\FileExts\<ext>\UserChoice` behind a per-user hash
only the shell can compute.

So `-Associate` writes what Windows actually intends an application to write: a
ProgId under `Software\Classes`, referenced from the extension's
`OpenWithProgids` list, plus an `Applications\polylinker.exe` entry. Polylinker
appears in *Open with*; whatever was the default stays the default. The prompt
reads the current handler out of the registry and names it — *"SnapGene Viewer
will keep opening .dna files"* — which is true rather than a hedge.

`.plproj` is the one exception, and only because Polylinker defines that format.
Claiming the default for a file type you invented is not what PLAN.md forbids.

Reversible with `-Unassociate`, by the uninstaller, or by deleting one visible
HKCU subtree.

The app side already worked: `App::open_argv` (`bins/pl-gui/src/main.rs:2405`)
takes every argument as a path and says so in its own doc comment.

### Upgrade

Run it again. It reads `install-receipt.txt`, prints `old -> new`, refuses while
`polylinker.exe` or `pl.exe` is running (naming the process and PID, because
Windows will not replace a mapped file and a half-finished upgrade is worse than
a refusal), replaces the files, and leaves PATH, associations and all of
`%LOCALAPPDATA%\Polylinker` alone.

### Uninstall

Settings → Apps, or `Uninstall.cmd` in the install folder. It removes exactly
what `install-receipt.txt` records — the receipt is a transcript of the install,
not a second guess at it — and **keeps everything in `%LOCALAPPDATA%\Polylinker`**.

Four things live there with four different claims:

| | | Uninstall |
|---|---|---|
| `layout` | a window preference | kept |
| `recovery\*.recover` | **unsaved user work**, rescued from a crash | kept |
| `session-*` | the restore-tabs bench | kept |
| `index\` | a regenerable cache, possibly hundreds of MB | kept unless `-RemoveCache` |

`recover.rs:223-231` already documents `recovery\` as somewhere "a user can find
and delete them without touching anything else", which reads as an argument for
never touching it programmatically. There is deliberately no "also remove my
settings?" checkbox: a checkbox at uninstall time is answered by muscle memory,
and one of the two answers cannot be taken back.

`tools/ci.ps1` plants a sentinel `.recover` file, installs, uninstalls, and
fails if the sentinel is gone. That assertion is the only thing that makes this
a promise in fact rather than in prose.

### What a Windows user actually sees

Being asked to run a `.cmd` out of a zip you downloaded is, structurally, the
shape of a lure. `README-WINDOWS.txt` says so in those words rather than
pretending otherwise, and then says what is different: both files are short and
readable, `-DryRun` shows the whole plan, and nothing runs until the user types
`yes`.

The order in `README-WINDOWS.txt` is verify → read → unblock → install:

1. `Get-FileHash` the zip against the published sidecar.
2. Open `Install-Polylinker.ps1`, or run it with `-DryRun`.
3. `Unblock-File` the **archive**, before extracting. Clearing the mark on the
   zip means nothing extracted from it carries one, so there is no prompt at
   all — clearing it afterwards means doing it to every extracted file.
4. Run it.

On first launch of `polylinker.exe`, SmartScreen may show *"Windows protected
your PC"*. The words *"More info → Run anyway"* appear nowhere in anything
shipped, because instructing somebody to click past a security warning is the
habit this project does not want to teach. What the shipped text says instead is
what the warning means (Windows does not recognise the publisher; it has not
found anything wrong with the file), what the checksum does and does not prove
(that this copy matches the release page; nothing about who built it), what a
certificate costs, and that some managed machines will refuse it outright and
the right response there is to ask the administrator rather than work around it.

### Deploying to several machines

```powershell
# In an elevated session, on each machine:
.\Install-Polylinker.ps1 -AllUsers -Yes
```

`-Yes` skips the typed confirmation; `-DryRun` first is still the right way to
see what it will do. No associations are registered — each user runs
`-Associate` for themselves if they want it.

This is the case an MSI serves properly, and it is the case that is worth
revisiting the day a certificate exists.

## macOS notarisation

Must run on macOS, so it is not in `release.ps1`:

```bash
codesign --force --options runtime --timestamp \
  --sign "Developer ID Application: NAME (TEAMID)" dist/polylinker
ditto -c -k --keepParent dist/polylinker dist/polylinker.zip
xcrun notarytool submit dist/polylinker.zip \
  --apple-id APPLE_ID --team-id TEAMID --password APP_SPECIFIC_PASSWORD --wait
xcrun stapler staple dist/polylinker
```

`--options runtime` is not optional: notarisation rejects a binary without the
hardened runtime, and the rejection message does not say so clearly.

## There is no auto-updater, on purpose

This was a decision, not an omission.

Polylinker's claim is that it runs offline and sends nothing anywhere. An
auto-updater contradicts that twice over: it phones a server on a schedule,
which is a beacon saying this machine exists and is running this version, and it
downloads and executes code the user did not ask for. On a lab machine that also
holds unpublished sequence, both are worth avoiding.

The update path is therefore: **the user checks when the user wants to.**
`pl --version` prints the version and the commit. The release page lists the
current one. That is the whole mechanism.

If an updater is ever added, the bar it has to clear is written down here so the
question is not reopened casually:

1. It downloads nothing without being asked, each time.
2. It verifies a signature over the download before the bytes touch disk in an
   executable location — a checksum fetched from the same server as the file
   proves nothing about an attacker who controls the server.
3. The public key is compiled into the binary being replaced, so the trust
   anchor is not fetched from the network.
4. It never replaces a running binary silently.

Any updater that cannot meet all four is worse than telling the user to
download the new version themselves.

The installer inherits this rule and the gate enforces it: `tools/ci.ps1` fails
if any network or scheduling facility appears anywhere in `tools/installer/`.

Note that `docs/PLAN.md` §5.1 and §10 risk 9 still describe a Tauri stack with a
"free, signature-mandatory auto-updater" and a `bundle > windows > signCommand`,
and the roadmap row at PLAN.md:214 was written against it. **The app is not
Tauri** — it is eframe/egui with no webview (`bins/pl-gui/Cargo.toml`). This
document supersedes the updater half of that plan. The paragraphs are left in
place because PLAN.md is a record of how the architecture was decided, not a
description of what is built, but nobody should read them as a live intention.

## Reproducibility

`SHA256SUMS.txt` records the commit and the exact `rustc` version, and the
script warns when the working tree is dirty, because a hash that cannot be tied
to a commit is a number and not a guarantee. Byte-for-byte reproducible builds
across machines are **not** claimed: the build embeds absolute paths, and
verifying the claim needs a second machine to build on. Saying so is better than
implying a property nobody has checked.
