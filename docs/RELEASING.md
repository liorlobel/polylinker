# Releasing

## What is done, and what is not

`tools/release.ps1` builds the binaries, records the commit and toolchain, and
writes `SHA256SUMS.txt`. It signs if given an identity and says loudly that it
did not if it was not. It runs on all three platforms and produces one archive
per platform; `.github/workflows/release.yml` runs it on three GitHub runners
and attaches the three results to a release. [Cutting a release](#cutting-a-release)
is the procedure.

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

Both look exactly like what malware looks like, which is the real cost. An
academic tool asking a labmate to click past a security warning is teaching a
bad habit, so the shipped text does not do that on either platform.

**Windows.** SmartScreen shows *"Windows protected your PC"* on first run.
Dismissing it is one click and the words for that click appear nowhere in
anything this project ships — not in `README-WINDOWS.txt`, not in the release
notes. What the shipped text says instead is what the warning means (Windows
does not recognise the publisher; it has not found anything wrong with the
file), what the checksum does and does not prove, and that a managed machine
refusing it outright is a question for the administrator rather than something
to work around.

**macOS.** Gatekeeper refuses a downloaded, unsigned, un-notarised binary and
says *"cannot be opened because the developer cannot be verified"*. The dialog
offers only *Move to Bin* and *Cancel*.

The remedy that is shipped is the honest one: macOS tags browser downloads with
an extended attribute named `com.apple.quarantine`, and that tag is what
Gatekeeper is reacting to, so remove it from the files that were extracted.

```sh
xattr -d com.apple.quarantine polylinker pl pl-mcp polylinker.so
```

Not right-click → Open, which is the usual advice. It works, but it is one
gesture that means "I have decided to trust this" applied identically to
software the user checked and software they did not — the same click-through
habit the Windows paragraph above refuses to teach, wearing a different shape.
The command names exactly which files are being exempted and leaves Gatekeeper,
SIP and every other program on the machine untouched. `README-MACOS.txt` and
the release notes both carry it, with that explanation.

**Linux.** No equivalent expectation, so nothing to explain away. The thing a
Linux user does need to be told is the glibc floor, which is a different kind of
honesty — see below.

## Cutting a release

One tag. Everything else follows from it.

```sh
# 1. Bump the version. There is exactly one copy of it.
#    Cargo.toml, [workspace.package] version = "0.2.0"

# 2. Green gate, locally, on the commit you are about to tag.
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
pwsh -NoProfile -File tools/ci.ps1

# 3. Commit, push, and let CI go green on all three runners.

# 4. Tag. This is the only step that publishes anything.
git tag -a v0.2.0 -m "Polylinker 0.2.0"
git push origin v0.2.0
```

`.github/workflows/release.yml` triggers on `v*`. It also has a
`workflow_dispatch` trigger, which runs the three builds and leaves the archives
on the run as workflow artifacts **without** creating a release — that is how
you answer "would this release build?" without spending a tag.

### What the workflow does

Three jobs in a matrix, `fail-fast: false` so one platform failing does not hide
the other two, and a fourth job that publishes only if all three produced
something.

| Runner | Label | Archive |
|---|---|---|
| `windows-latest` | `windows-x64` | `polylinker-<version>-windows-x64.zip` |
| `ubuntu-latest` | `linux-x64` | `polylinker-<version>-linux-x64.tar.gz` |
| `macos-latest` | `macos-universal` | `polylinker-<version>-macos-universal.tar.gz` |

Every job:

1. **Checks the tag against `Cargo.toml`** before building anything. A `v0.2.0`
   tag on a tree that still says `0.1.0` would publish three archives named
   `0.1.0` under a release called `v0.2.0`, each carrying the wrong number in
   its own manifest. This fails in a minute rather than in twenty.
2. Builds and packages by running **`tools/release.ps1`** — the same script the
   local gate runs.
3. Runs **`tools/check-archive.ps1`** over the archive.
4. Uploads the archive and its `.sha256`.

The publish job re-downloads all three, re-checks each sidecar and re-runs
`check-archive.ps1` on the bytes that will actually be attached (an upload and a
download sit in between), writes one cross-platform `SHA256SUMS.txt` over the
three archives, renders `tools/release-notes.md`, and calls `gh release create
--verify-tag`. `gh` is preinstalled on the runner, so no third-party action is
in the trust path.

To review before the world sees it, add `--draft` to that call. It is not the
default, because a draft nobody remembers to publish looks exactly like a
release that failed.

### Why one script and not two

`tools/release.ps1` is platform-aware rather than split into a Windows script
and a Unix script. Three things in it are platform-specific — the `.exe` suffix,
the name CPython will load an extension module under, and zip versus tar.gz —
and one thing in it is not: the `$notices` array, which is the list of licence
texts that four licences require to accompany every copy. That list has drifted
from what actually shipped twice, on 2026-08-03 and 2026-08-04, and the thirty
lines of comment above it are the record. A second script is a second copy of
that list, and `tools/ci.ps1` would exercise one of them.

The same argument governs the workflow: no job in `release.yml` assembles an
archive. `tools/ci.ps1` enforces it, by failing if `tar -c`, `Compress-Archive`
or `zip -r` appears anywhere in that file.

### macOS: one universal binary

`macos-latest` is Apple Silicon. The Intel slice is cross-compiled on the same
runner (`rustup target add x86_64-apple-darwin`, then `lipo -create`) rather
than built on a second, Intel runner.

- **Why not arm64 only.** An Intel Mac is still ordinary lab equipment, and
  "this download does not work on my machine" is where a first release loses
  people.
- **Why not two artifacts.** It doubles the choice the user has to get right, on
  a release page they are reading because they are not a software person.
- **Why not an Intel runner.** `macos-*-intel` and the `-large` images are
  billed even for public repositories. `lipo` on the free arm64 runner is not.
- **What it costs.** A second full LTO build — the slowest job in the matrix by
  some distance — and roughly double the size of the three executables. That is
  the price of one download.

`polylinker.so` is joined the same way, and is importable on both architectures.

### Linux: which machines this actually runs on

`ubuntu-latest` is Ubuntu 24.04, whose glibc is **2.39**. glibc is backward but
not forward compatible, so that is a floor and not a preference:

| | glibc | Runs |
|---|---|---|
| Ubuntu 24.04+, Debian 13+, Fedora 40+, RHEL 10+ | ≥ 2.39 | yes |
| Ubuntu 22.04 | 2.35 | **no** |
| Debian 12 | 2.36 | **no** |
| RHEL 9 / Rocky 9 | 2.34 | **no** |

The failure is a bare ``version `GLIBC_2.39' not found`` at exec time, which
tells a wet-lab user nothing. So it is stated in three places, and one of them
is inside the archive: `README-LINUX.txt` travels with the binaries, because a
release page gets read once and a tarball gets copied onto a cluster by somebody
who never saw it.

**And it is measured rather than asserted.** The Linux job reads the highest
`GLIBC_x.y` symbol version the four artifacts actually reference, compares it
with the number written in `README-LINUX.txt`, and fails the release if the
binaries need more than the file promises. A runner image upgrade raises that
floor silently; this turns it into a red build instead of a bug report from
somebody with an older cluster.

Widening the floor means building on an older image — that is the only lever,
and it is a deliberate change to `runs-on`, not something to discover by
accident.

**Run-time libraries.** `pl` and `pl-mcp` need nothing but libc. The GUI reaches
X11, xkbcommon, Wayland and EGL/GL through `dlopen` rather than linking them, so
they do not appear in `DT_NEEDED` and a missing one shows up as a failure to
start. `README-LINUX.txt` lists them, with the Debian/Ubuntu package names.
Notably **not** GTK: there is no gtk crate anywhere in `Cargo.lock`, and `rfd`'s
Linux path is the XDG portal over Wayland. (`libgtk-3-dev` is still in the
apt-install line in both workflows. It is not needed to build either; removing
it belongs in `ci.yml`, where a wrong guess costs a red pull request instead of
a red release.)

### Archive formats

`.zip` for Windows, `.tar.gz` for both Unixes — the convention, and on the Unix
side also the only one of the two that works. A zip has no portable place to
record a file mode, so `unzip` clears the executable bit and the first thing the
user does is `chmod +x`; a tar carries the mode in its header. Windows keeps the
zip because Explorer opens one with nothing installed and does not open a
`.tar.gz` at all.

Both are written by hand in `release.ps1` — entries sorted, timestamps pinned to
2000-01-01, uid/gid zero and no `uname`/`gname`, compression fixed — so the
archive is a function of its contents rather than of the day and the machine it
was built on. `Compress-Archive` cannot do the first two, and shelling out to
`tar` would additionally stamp the build account into every header.

### What is checked, and where

| Check | Where |
|---|---|
| `release.ps1` runs; its manifest is set-equal to `dist/` | `tools/ci.ps1`, locally |
| ≥ 19 files hashed, ≥ 7 licence texts, `NOTICE.txt` / `LICENSE.txt` / `features/NOTICE.txt` by name | `tools/ci.ps1` |
| The zip is a deterministic function of `dist/` | `tools/ci.ps1` |
| The **archive** verifies against its own `SHA256SUMS.txt`, licence set included | `tools/check-archive.ps1`, locally and on all three runners |
| The tar writer produces something GNU tar or bsdtar will read | `tools/ci.ps1` (forces `-ArchiveFormat tar.gz` on Windows) |
| `release.yml` parses, covers three OSes, publishes only on a tag | `tools/ci.ps1` (PyYAML) |
| `release.yml` packs no archive of its own, and every `tools/` path it names exists | `tools/ci.ps1` |
| The release notes still carry the quarantine remedy, SmartScreen, the glibc floor, and the checksum caveat — and still contain no "Run anyway" | `tools/ci.ps1` |
| The measured glibc floor matches `README-LINUX.txt` | `release.yml`, Linux job |

The tar writer is forced on Windows for a specific reason: it is Unix-only code,
this gate is the only thing that runs on a developer machine, and that machine
is Windows. An archive format whose only exercise is a green job on a runner is
an archive format nobody has looked at the output of.

## The Windows install path

`tools/release.ps1` produces `dist/polylinker-<version>-windows-x64.zip`. That
zip is the download. Inside it, beside the binaries and the licence texts, are
three files:

| File | What it is |
|---|---|
| `Install-Polylinker.ps1` | the installer, in plain text |
| `Install.cmd` | four lines, so double-clicking works |
| `README-WINDOWS.txt` | verify, read, unblock, install — in that order |

### Why an MSI, and what it did and did not solve

**This section argued the opposite until 2026-08-05, when the decision was
reversed.** It is rewritten rather than deleted, because three of its four
arguments were answered by the design and one was not, and a reader deciding
whether to trust the installer deserves to know which is which.

An MSI now ships: `polylinker-<version>-windows-x64.msi`, built by
`tools/build-msi.ps1` from `tools/installer/Polylinker.wxs`. The zip and the
readable PowerShell installer still ship alongside it.

**Answered structurally — the second file list.** The strongest objection was
that every compiled installer carries a second list of files, a WiX
`<Component>` set, hand-copied from `release.ps1`'s `$notices` array — and that
this copy had already drifted twice in one week, dropping a licence text each
time. `Polylinker.wxs` therefore contains no files at all. The component set is
**generated** from `dist/SHA256SUMS.txt`, the same manifest `check-archive.ps1`
verifies, and the gate step *the MSI is generated from the manifest and not from
a second file list* asserts the two are equal — including a negative control, so
the comparison is known to be able to notice a difference rather than assumed to
be. There is still exactly one file list; the MSI is a second reader of it, not
a second copy of it.

**Answered by where it runs — nothing installed on the build machine.** WiX is a
`dotnet tool` and there is no .NET SDK here. It is therefore a *release-time*
dependency only: the MSI is built on the `windows-latest` runner, and every MSI
step in `tools/ci.ps1` skips itself when `wix` is absent, so a contributor
without an SDK still runs a complete gate. `cargo build` and `cargo test`
acquired no dependency at all.

**Answered by writing it in the gate's own language.** `tools/check-msi.ps1` is
PowerShell, like the rest of the gate. It installs with `msiexec`, asserts
against the disk and the registry, uninstalls, and asserts nothing survived —
including that a *planted* default handler for `.dna` is still intact, which is
what turns "the associations are additive" from a hope into a measurement.

**Not answered — signing.** Everything below is still true. The MSI ships
anyway, because the standard Windows install experience was judged worth the
cost. Two things reduce the damage: the MSI installs **for the current user by
default**, which raises no elevation prompt at all and so never shows the
`Publisher: Unknown` consent dialog; and the zip remains, for anyone whose
policy refuses unsigned packages or who would simply rather read a script.

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

**When a certificate arrives, sign the MSI, the three binaries and the zip.**
That is now the only outstanding item on this list; the MSI itself arrived
first, on 2026-08-05, and the `-AllUsers` layout below did turn out to be the
layout it uses, so it was a port and not a redesign.

One thing the port deliberately did **not** carry across. The PowerShell
installer takes the *default* handler for `.plproj` (`Install-Polylinker.ps1`,
the association table). The MSI does not associate `.plproj` at all, because
double-clicking one cannot work: the GUI decides a file's format by content
through `pl_fileio`, and no crate under `crates/` knows the `.plproj` format —
it is a bench file read by `bins/pl-gui/src/session.rs` from a menu. A
double-click reaches `load()` → `load_as()` and fails. The gate step *the MSI
takes no file type away from a program the reader already uses* asserts the
association stays absent.

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

Not automated, and not because it would be hard to write. It needs an Apple ID
and an app-specific password, and a release script that can hold those is a
release script that can leak them — the same rule that keeps key handling out of
the Windows path. `release.ps1 -MacIdentity` prints a pointer to this section and
does nothing else.

The day a Developer ID certificate exists, this runs on the universal binary
after `lipo` and before packaging:

```bash
for f in target/universal/pl target/universal/polylinker target/universal/pl-mcp; do
  codesign --force --options runtime --timestamp \
    --sign "Developer ID Application: NAME (TEAMID)" "$f"
done
ditto -c -k --keepParent target/universal dist/polylinker.zip
xcrun notarytool submit dist/polylinker.zip \
  --apple-id APPLE_ID --team-id TEAMID --password APP_SPECIFIC_PASSWORD --wait
xcrun stapler staple target/universal/polylinker
```

`--options runtime` is not optional: notarisation rejects a binary without the
hardened runtime, and the rejection message does not say so clearly.

Two things change when that happens, and both should be done together: the
`xattr` paragraph comes out of `README-MACOS.txt` and the release notes, and the
Apple ID and password become repository secrets rather than anything a script
constructs. `codesign` signs the individual Mach-O files; a stapled ticket
attaches to a bundle or a disk image, so notarisation of bare executables
verifies online at first launch rather than offline. If offline first launch
matters — and for a tool whose pitch is that it needs no network, it does — that
is the point at which a `.app` bundle and a `.dmg` become worth building, and
not before.

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
