Polylinker on macOS
===================

Polylinker is an offline plasmid editor. It never sends a sequence anywhere, it
has no updater, and it needs no administrator rights.

There is nothing to install. The three programs in this folder run as they are.


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

There is no .app bundle, no .dmg and no Homebrew formula. `polylinker` is a bare
executable, so double-clicking it in Finder opens a Terminal window alongside the
editor, and the menu bar shows the executable's name rather than a proper
application name. That is cosmetic and it is honest about what this is: an
unsigned binary that would be misrepresented by wrapping it in the packaging of
a signed application.

The Python extension is loaded from wherever you put it:

    import importlib.util as u
    s = u.spec_from_file_location("polylinker", "./polylinker.so")
    m = u.module_from_spec(s); s.loader.exec_module(m)


UNSIGNED
--------

This build is not code-signed and not notarised, and that is a funding question
rather than an oversight. It needs Apple Developer Program membership at USD 99
a year, issued to a person or an organisation. See docs/RELEASING.md in the
source tree.

What this means for you, concretely:

  * Gatekeeper will refuse these files until you clear the quarantine flag, and
    the wording it uses -- "the developer cannot be verified" -- is accurate.
    Apple has not verified anybody, because nobody paid to be verified.
  * The SHA-256 you checked in step 1 proves this copy is byte-for-byte the one
    published on the release page. It proves nothing about who published it.
    Those are different guarantees and only one of them is available here.
  * Some managed Macs refuse unsigned software by MDM policy, and clearing the
    quarantine attribute will not change that. If yours does, the correct next
    step is to ask whoever administers the machine -- not to work around it.

If you are not comfortable with any of that, do not run it. That is a reasonable
position and this file is not going to talk you out of it.
