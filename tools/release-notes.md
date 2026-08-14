<!--
The body of every GitHub Release. `.github/workflows/release.yml` substitutes
{{VERSION}} and {{TAG}} and appends the checksum table.

It is a file rather than a heredoc in the workflow because the honest paragraphs
about unsigned builds are the part most likely to be quietly softened, and a file
shows up in a diff. `tools/ci.ps1` asserts that the macOS quarantine remedy and
the SmartScreen paragraph are still in here.
-->

Polylinker {{TAG}} — an offline plasmid editor.

Never sends a sequence anywhere. No telemetry, no account, no auto-updater. The
only network request Polylinker can make is an update check you asked for, and a
fresh installation asks for nothing until you tell it to.

## Downloads

| Platform | File | Notes |
|---|---|---|
| **Windows 10/11, x64** | **`polylinker-{{VERSION}}-windows-x64.msi`** | **the installer — start here.** Installs for you alone by default, so no admin rights and no elevation prompt |
| Windows 10/11, x64 | `polylinker-{{VERSION}}-windows-x64.zip` | portable: unzip and run, nothing installed. Also contains a readable PowerShell installer |
| **Windows 11, ARM64** | **`polylinker-{{VERSION}}-windows-arm64.msi`** | **native ARM64 — new, and less proven than the rest of this table.** Read *Which Windows file?* below before taking it |
| Windows 11, ARM64 | `polylinker-{{VERSION}}-windows-arm64.zip` | the portable form of the same build |
| macOS 11+, Apple Silicon **and** Intel | `polylinker-{{VERSION}}-macos-universal.tar.gz` | one universal binary for both |
| Linux, x64, glibc 2.39 or newer | `polylinker-{{VERSION}}-linux-x64.tar.gz` | see the note below before downloading |

Each archive contains `polylinker` (the editor), `pl` (the command line),
`pl-mcp` (the MCP server), the Python extension module, the licence texts, and a
`SHA256SUMS.txt` covering every one of them. Verify the archive against the
checksum table at the bottom of this page before extracting it. The `.msi`
installs that same set, minus the two installer files it replaces, and is in
that checksum table too.

### Which Windows file?

**First, x64 or ARM64.** If you do not know, you have x64 — press
Windows+Pause and read *System type*, or run `echo $env:PROCESSOR_ARCHITECTURE`
in PowerShell: `AMD64` means x64, `ARM64` means ARM64. Nearly every Windows PC
is x64; ARM64 is Snapdragon-based machines such as the Surface Pro 11 and the
Copilot+ laptops.

**An ARM64 machine will run the x64 files perfectly well**, under Windows'
built-in emulation, and until this release that is what ARM64 users were
doing. The native build should be faster and will not warm your battery
emulating, but it is the newest thing here: it is compiled and its test suite
is run on real Windows-on-ARM hardware for every commit, and the zip and MSI
are built, installed, checked and uninstalled there — but the cross-checks
against Biopython, pydna, SciPy and the rest are run only on x64, and at the
time this release was cut **no person had run a Polylinker build on an ARM64
machine at all**. If you want the better-trodden path today, take the x64
files; they work. If you take the ARM64 ones and something is wrong, that is
worth an issue, because you will be among the first to look.

**Then, installer or zip.** Take the **`.msi`** unless you have a reason not to. It puts Polylinker in the
Start Menu and in Settings → Apps, offers to put `pl` on your PATH, and adds
Polylinker to the "Open with" list for GenBank, FASTA, SnapGene `.dna` and
`.ab1` trace files. It **adds** itself to those lists: if SnapGene owns `.dna`
on your machine, it still will afterwards.

Take the **`.zip`** if you want to run Polylinker without installing anything,
or if you would rather read the installer before running it — nothing here is
signed, and a script you can read is the only assurance on offer that is not a
checksum. Its `Install-Polylinker.ps1` does the same job in text.

## Every one of these is unsigned

There is no code-signing certificate for any platform, and there is not going to
be one. That is a decision rather than an oversight, and not a gap waiting to be
filled: Polylinker ships unsigned, on every platform, and no later release will
be different. `docs/RELEASING.md` has the reasoning.

It has a concrete cost on two of the three platforms, and the honest thing is to
say what you will see and what to do about it.

**macOS.** Gatekeeper will refuse to open these files and will say

> "polylinker" cannot be opened because the developer cannot be verified.

That message is accurate: Apple has verified nobody, because this project has no
Developer ID. macOS tags anything a browser downloaded with an extended attribute
named `com.apple.quarantine`, and it is that tag Gatekeeper checks. Remove it
from the files you extracted:

```sh
xattr -d com.apple.quarantine polylinker pl pl-mcp polylinker.so
```

That is a per-file operation. Gatekeeper stays on, System Integrity Protection
is untouched, and everything else on the machine is still checked exactly as it
was. If the command says `No such xattr`, the files were never quarantined and
there is nothing to do.

You may see the right-click → Open trick recommended elsewhere. It works, and it
is a worse habit: it is the same click-through for software you checked as for
software you did not. The command above names what is being allowed and to which
files.

**Windows.** SmartScreen may show *"Windows protected your PC"* the first time
you run `polylinker.exe`. That means Windows does not recognise the publisher.
It does not mean Windows found anything wrong with the file. The deliberate
omission here is the instruction to click past it: the words that usually follow
are not in anything this project ships, because teaching a labmate to dismiss a
security warning is a worse outcome than an awkward paragraph. Read
`README-WINDOWS.txt` inside the zip, check the SHA-256, and decide.

The `.msi` carries no signature either, and SmartScreen treats it the same way.
Two things are worth knowing before you open it. Its default is to install for
you alone, which needs no administrator — so on that path Windows does not show
the yellow-banded elevation dialog reading *Publisher: Unknown*, because it does
not ask for elevation at all. If you choose "for everyone" instead, it does, and
that dialog will name no publisher. Nothing is wrong; there is no publisher to
name, and there is not going to be one.

Some managed and locked-down machines — Windows and macOS both — refuse unsigned
software outright by policy. If yours does, this will not run, and the right
response is to ask whoever administers the machine rather than to work around
it.

**Linux** has no equivalent expectation. Nothing will stop you and nothing will
vouch for these either.

**What the checksum does and does not prove.** It proves the copy you have is
byte-for-byte the one published on this page. It proves nothing about who
published it — anyone who could replace the files could replace the table.
Those are different guarantees, and the second one is what the **Signature**
section at the bottom of this page is for: `SHA256SUMS.txt.sig` is an Ed25519
signature over that table, made by the release key, and that section gives the
OpenSSL command to check it. It is still not code signing, and everything said
above about SmartScreen and Gatekeeper is unchanged by it — those check a
certificate, and there is none.

## Linux: check your glibc first

The Linux binaries are built on Ubuntu 24.04 and need **glibc 2.39 or newer**.
glibc is backward compatible but not forward compatible, so on anything older
they will not start at all, and the error is a bare
``version `GLIBC_2.39' not found``. Check with `ldd --version`.

- Fine: Ubuntu 24.04+, Debian 13+, Fedora 40+, RHEL 10+
- **Will not run**: Ubuntu 22.04 (2.35), Debian 12 (2.36), RHEL 9 (2.34)

There is no build for older distributions. On one of those, build from source —
it needs Rust 1.92 and nothing exotic, and `pl` and `pl-mcp` need no system
libraries at all. `README-LINUX.txt` inside the archive lists the shared
libraries the editor opens at run time.

## No auto-updater, on purpose

Polylinker never checks for a new version on its own. A program that phones a
server on a schedule is a program announcing that this machine exists and is
running this version, and on a lab machine holding unpublished sequence that is
worth not doing. Nothing here runs on a timer, nothing remembers when it last
looked, and nothing installs anything.

There are two ways to ask, and both are things you do rather than things that
happen to you.

**From the command line.** `pl update --check` makes one request and prints the
answer. `pl update` downloads the release and keeps it only if it carries an
Ed25519 signature made by the key compiled into the copy you are already
running — a checksum served from the same place as the file would prove nothing
about whoever is serving it. It then prints the path and stops: running it is
yours to do, and to watch. `pl --version` still prints the version and the
commit without asking anybody anything, and this page lists the current release.

**In the desktop app, off by default.** Under Help there is a switch, unticked
in every new installation, with a sentence beside it saying exactly what the
request contains. Turned on, it asks once per launch and shows a quiet notice.
It sends no sequence, no file name and no identifier; the request tells
github.com your IP address and nothing about your work. The app never downloads
a release — the notice points at this page and at `pl update`.

`docs/RELEASING.md` records the four conditions this had to meet before it could
exist, and names the test that holds each one.

## Licences

Polylinker is MIT OR Apache-2.0, at your option — both texts are in the archive,
`LICENSE.txt` and `LICENSE-MIT.txt`. It embeds ten font faces under four other
licences, four of which require their text to accompany every copy — so
`licences/`, `NOTICE.txt` and `features/NOTICE.txt` are inside every archive too,
and all of it is covered by its `SHA256SUMS.txt`. The feature database is CC BY
4.0 and carries its own attribution in `features/NOTICE.txt`.

No restriction-enzyme database is redistributed. The 58 enzymes in the digest
are transcribed from published references and cross-checked against Biopython;
REBASE is not licensed for this and is not here.
