Polylinker on Windows
=====================

Polylinker is an offline plasmid editor. It never sends a sequence anywhere, it
has no auto-updater, and it does not need administrator rights. Nothing it
installs runs on its own or checks for a new version by itself: the editor's
update check is off until you switch it on under Help, and `pl update` is a
command you type.

You do not have to install it. polylinker.exe in this folder runs as it is --
double-click it. The installer below exists so it appears in the Start Menu and
in Settings -> Apps like anything else, not because anything needs installing.


THERE IS ALSO AN .MSI, AND FOR MOST PEOPLE IT IS THE EASIER ONE
---------------------------------------------------------------

The release page publishes polylinker-<version>-windows-x64.msi next to this
zip. It is the ordinary Windows installer: double-click, next, done. It installs
for you alone unless you choose "for everyone", so it needs no administrator and
raises no elevation prompt on the default path.

This zip is still here for two kinds of reader: anyone who wants to run
Polylinker without installing anything at all, and anyone who would rather run
an installer they can read first. That second one is what section 2 below is
about, and the difference is real -- nothing here is code-signed, so being able
to read the installer is the only assurance on offer that is not a checksum.

One difference worth knowing if you use both. The .msi does NOT make Polylinker
the default program for .plproj files, and Install-Polylinker.ps1 does. The .msi
is the one that is right: Polylinker works out a file's format by looking inside
it, and it does not recognise its own .plproj bench files that way, so
double-clicking one does not open it. Open a bench from inside the app instead.


1. VERIFY
---------

The release page publishes a SHA-256 for the zip you downloaded. Check it before
you extract anything. In PowerShell:

    Get-FileHash .\polylinker-<version>-windows-x64.zip -Algorithm SHA256

Compare the result with the published one, character for character.

Inside this folder, SHA256SUMS.txt lists a hash for every single file, and the
installer re-checks all of them before it copies anything. You can check them
yourself:

    Get-FileHash .\polylinker.exe -Algorithm SHA256


2. READ
-------

Install-Polylinker.ps1 is the installer. It is a text file. Open it.

That is the point of shipping a script instead of a compiled setup program: this
build is not code-signed (see UNSIGNED below), so the two things it can offer
you are a checksum and the ability to see what it does before it does it. A
compiled installer would keep the first and take away the second.

If you would rather not read 700 lines, run it with -DryRun. It prints the
complete list of files it will copy and registry values it will write, and then
stops without touching anything:

    powershell -NoProfile -ExecutionPolicy Bypass -File .\Install-Polylinker.ps1 -DryRun


3. UNBLOCK, THEN INSTALL
------------------------

Windows marks files that came from the internet. If you extracted this folder
with Explorer, every file in it is marked, and Windows will complain about the
script. Clear the mark on the ZIP before extracting -- that way nothing inside
it is ever marked:

    Unblock-File .\polylinker-<version>-windows-x64.zip

If you already extracted, clear the mark on the extracted files instead:

    Get-ChildItem -Recurse | Unblock-File

Then double-click Install.cmd, or run:

    powershell -NoProfile -ExecutionPolicy Bypass -File .\Install-Polylinker.ps1

Useful options:

    -DryRun       print the plan and stop
    -AddToPath    put pl.exe on your PATH so `pl` works in a terminal (off by default)
    -Associate    add Polylinker to the "Open with" menu for sequence files
    -Uninstall    remove it again
    -AllUsers     install for everyone on the machine (needs an admin session)

Being asked to run a .cmd from a zip you downloaded is, structurally, the shape
of a lure. Saying so is more useful than pretending otherwise. What is different
here: both files are short and readable, nothing runs until you type "yes", and
-DryRun shows you the whole plan first.


WHERE THINGS GO
---------------

    %LOCALAPPDATA%\Programs\Polylinker    the program, licences, this file
    %LOCALAPPDATA%\Polylinker             your settings, crash drafts, index cache
    Start Menu                            one shortcut, no folder
    HKCU ...\Uninstall\Polylinker         the Settings -> Apps entry

install-receipt.txt, in the install folder, lists every path and registry value
the installer wrote. The uninstaller removes exactly that list and nothing else.

An uninstall KEEPS everything in %LOCALAPPDATA%\Polylinker. Your window layout
is a preference, and recovery\ holds unsaved work rescued from a crash -- that
is yours, and an uninstaller is not the right place to be asked whether to throw
it away. The library index cache in index\ regenerates and can be deleted with
-RemoveCache if you want the disk space back.


FILE TYPES
----------

Polylinker does not take .dna from SnapGene, or .gb from anything else. With
-Associate it ADDS itself to the "Open with" menu and leaves every default
exactly as it was. Since Windows 8 the default handler is set by the user, in
the "Open with -> Choose another app" dialog, and no installer can legitimately
set it for them.

-Unassociate removes those entries again.


UNSIGNED
--------

This build is not code-signed, and that is a funding question rather than an
oversight. A Windows code-signing certificate is roughly GBP 200-400 a year, or
about USD 120 a year for Azure Trusted Signing -- and Azure Trusted Signing is
restricted to verified businesses and self-employed individuals, which an
unincorporated academic project may not qualify as at all. See docs/RELEASING.md
in the source tree.

What this means for you, concretely:

  * Windows may show "Windows protected your PC" the first time you run
    polylinker.exe. That message means Windows does not recognise the publisher.
    It does not mean Windows found anything wrong with the file.
  * The SHA-256 you checked in step 1 proves this copy is byte-for-byte the one
    published on the release page. It proves nothing about who published it.
    Those are different guarantees, and the second one is now available too:
    the release page publishes SHA256SUMS.txt.sig, an Ed25519 signature over
    that checksum table made by the release key, and prints the command to
    check it. The key's public half is compiled into pl.exe and polylinker.exe,
    which is what lets `pl update` check a download without trusting the page
    it came from. Windows has never heard of that key and will go on showing
    the warning above regardless -- that is code signing, and it is separate.
  * Some managed and locked-down machines refuse unsigned software outright, by
    policy. If yours does, this will not run, and the correct next step is to
    ask whoever administers the machine -- not to work around it.

If you are not comfortable with any of that, do not run it. That is a reasonable
position and this file is not going to talk you out of it.
