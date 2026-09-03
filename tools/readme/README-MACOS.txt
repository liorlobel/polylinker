Polylinker on macOS
===================

Polylinker is an offline plasmid editor. It never sends a sequence anywhere, it
has no auto-updater, and it needs no administrator rights. Nothing here runs on
its own or checks for a new version by itself: the editor's update check is off
until you switch it on under Help, and `pl update` is a command you type.

There is nothing to install. The three programs in this folder run as they are.
This is the bare form; the release page also carries a disk image,
polylinker-<version>-macos-universal.dmg, with the same programs inside a
Polylinker.app you drag to Applications. See section 3.


WHAT IS IN HERE
---------------

    polylinker        the editor, with a window
    pl                the command-line tool
    pl-mcp            the MCP server, for driving pl from an AI assistant
    polylinker.so     the Python extension module (CPython 3.9+, any 3.x)
    SHA256SUMS.txt    a hash for every file above and every file below
    licences/         the font licences, which have to travel with the binaries
    NOTICE.txt        who wrote what, and under which terms
    features/         the feature database's own attribution

All three programs are universal binaries and run natively on Apple Silicon and
on Intel. `file polylinker` will say so.


1. VERIFY
---------

The release page publishes a SHA-256 for the archive you downloaded. Check it
before you extract anything:

    shasum -a 256 polylinker-<version>-macos-universal.tar.gz

Compare the result with the published one, character for character. Afterwards,
SHA256SUMS.txt covers every individual file:

    shasum -a 256 -c SHA256SUMS.txt


2. CLEAR THE QUARANTINE FLAG
----------------------------

This build is not signed and not notarised. See UNSIGNED below for why.

macOS tags anything downloaded by a browser with an extended attribute named
com.apple.quarantine. When you run a quarantined program that carries no
Developer ID signature, Gatekeeper refuses it and shows

    "polylinker" cannot be opened because the developer cannot be verified.

The dialog offers Move to Bin and Cancel. Neither is the answer. Remove the
attribute from the files you extracted:

    xattr -d com.apple.quarantine polylinker pl pl-mcp polylinker.so

That is a per-file operation. It clears the download tag on exactly these four
files and changes nothing else -- Gatekeeper stays on, System Integrity
Protection is untouched, and every other program on the machine is still checked
the way it was a minute ago. If the command reports "No such xattr", the files
were not quarantined in the first place (`tar` extracting from a terminal does
not always propagate it) and there is nothing to do.

If you took the .dmg instead, the tag is on the image and on everything copied
out of it. Clear it from the bundle you dragged to Applications, recursively,
because a bundle is a folder:

    xattr -dr com.apple.quarantine /Applications/Polylinker.app

You may have been told elsewhere to right-click the file and choose Open. That
works, and it is a worse habit: it is the same click-through for a program you
checked as for one you did not. The command above says what is being allowed and
to which files.


3. RUN IT
---------

From a terminal, in this folder:

    ./polylinker              open the editor
    ./polylinker my.gb        open the editor on a file
    ./pl --help               the command-line tool

To put `pl` somewhere your shell will find it, move it there yourself:

    mkdir -p ~/.local/bin && mv pl ~/.local/bin/

and make sure ~/.local/bin is on your PATH.

This archive is the bare form. `polylinker` here is a bare executable, so
double-clicking it in Finder opens a Terminal window alongside the editor, and
the menu bar shows the executable's name rather than a proper application name.

The release page also carries polylinker-<version>-macos-universal.dmg, which
holds the same three programs and the same Python module inside a
Polylinker.app bundle -- with an icon and a name Finder shows, and the same
licence texts under Contents/Resources. Open the image, drag Polylinker.app to
Applications, clear the quarantine tag as section 2 says, and double-click it.
Two things the bundle does not do, stated so they are not discovered: it does
not register itself for .dna, .gb or any other file type, because the editor
takes files from the command line and from drag-and-drop and nothing in it
receives the event a Finder double-click on a document sends -- so open a file
by dropping it on the window, or by running the program inside the bundle,

    /Applications/Polylinker.app/Contents/MacOS/polylinker my.gb

which is the command line, and is the form that was measured. `open -a
Polylinker my.gb` does NOT open the file: `open` hands it over as the same
event a double-click sends, which is the one this program does not receive.
This paragraph recommended that command until 2026-09-03. (`open`'s `--args`
flag is documented to pass what follows it to argv instead; whether that
reaches this program was not measured, so it is not recommended here either.)
And the bundle is not
signed, so Gatekeeper refuses it exactly as it refuses this tarball and the
remedy is the recursive form of the same command. Until 2026-09-03 this
paragraph said there was no .app bundle and no .dmg, and that wrapping an
unsigned binary in the packaging of a signed application would misrepresent it;
the bundle is as unsigned as this tarball and says so. There is still no
Homebrew formula.

The Python extension is loaded from wherever you put it:

    import importlib.util as u
    s = u.spec_from_file_location("polylinker", "./polylinker.so")
    m = u.module_from_spec(s); s.loader.exec_module(m)


UNSIGNED
--------

This build is not code-signed and not notarised, and it is not going to be.
That is a decision rather than an oversight or a gap: Polylinker ships unsigned,
on every platform, and nothing here is waiting on an Apple Developer ID. What
follows is permanent, not a description of how things are until something
arrives. docs/RELEASING.md in the source tree has the reasoning.

What this means for you, concretely:

  * Gatekeeper will refuse these files until you clear the quarantine flag, and
    the wording it uses -- "the developer cannot be verified" -- is accurate.
    Apple has verified nobody, because this project has no Developer ID and is
    not getting one. That will not change in a later release.
  * The SHA-256 you checked in step 1 proves this copy is byte-for-byte the one
    published on the release page. It proves nothing about who published it.
    Those are different guarantees, and the second one is now available too:
    the release page publishes SHA256SUMS.txt.sig, an Ed25519 signature over
    that checksum table made by the release key, and prints the command to
    check it. The key's public half is compiled into pl and polylinker, which
    is what lets `pl update` check a download without trusting the page it came
    from. Gatekeeper has never heard of that key and will refuse these files
    exactly as described above; that is notarisation, and it is separate.
  * Some managed Macs refuse unsigned software by MDM policy, and clearing the
    quarantine attribute will not change that. If yours does, the correct next
    step is to ask whoever administers the machine -- not to work around it.

If you are not comfortable with any of that, do not run it. That is a reasonable
position and this file is not going to talk you out of it.
