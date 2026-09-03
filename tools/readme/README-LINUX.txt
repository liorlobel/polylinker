Polylinker on Linux
===================

Polylinker is an offline plasmid editor. It never sends a sequence anywhere, it
has no auto-updater, and it needs no root. Nothing here runs on its own or
checks for a new version by itself: the editor's update check is off until you
switch it on under Help, and `pl update` is a command you type.

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


1. VERIFY
---------

The release page publishes a SHA-256 for the archive you downloaded. Check it
before you extract anything:

    sha256sum polylinker-<version>-linux-x64.tar.gz

Compare the result with the published one, character for character. Afterwards,
SHA256SUMS.txt covers every individual file:

    sha256sum -c SHA256SUMS.txt


2. WILL IT RUN HERE?
--------------------

This is the question worth asking before anything else, because the answer is
not "yes" everywhere.

GLIBC. These binaries are built against glibc 2.28, and glibc is backward
compatible but not forward compatible: a binary built against 2.28 runs on 2.28
and on everything newer, and on nothing older. Check yours:

    ldd --version

  2.28 or newer      RHEL/Rocky/Alma 8+, Ubuntu 20.04+, Debian 10+,
                     Fedora 29+  -- fine
  older than 2.28    Ubuntu 18.04 (2.27), RHEL 7 (2.17) -- these binaries will
                     not start, and the error will be a bare
                     "version `GLIBC_2.28' not found"

That number was 2.39 until 2026-09-03, because this leg built on whatever
`ubuntu-latest` happened to be and that had become Ubuntu 24.04. It put the
download out of reach of Ubuntu 22.04, Debian 12 and RHEL 9 -- the machines
this program is mostly for. The binaries are now compiled inside an AlmaLinux 8
container, which is the oldest glibc the project is willing to support, and the
release refuses to publish if this number and the binaries ever disagree in
EITHER direction.

There is no build for anything older than 2.28 and this file is not going to
pretend otherwise. On such a machine, build from source -- the toolchain
requirement is Rust 1.92, the repository is at
github.com/liorlobel/polylinker, and the README there lists the build
dependencies. `pl` and `pl-mcp` are pure Rust and build with no system
libraries at all.

SHARED LIBRARIES. `pl` and `pl-mcp` need nothing but libc. `polylinker` is the
one with a window, and it opens the graphics and input libraries by name at run
time rather than linking them, so a missing one appears as a failure to start
rather than as a loader error naming the file. On a desktop install they are
already present. On a headless or minimal image, the ones it looks for are:

    libX11, libXcursor, libXi, libXrandr    X11
    libxkbcommon, libxkbcommon-x11          keyboard handling
    libwayland-client                       Wayland, if you are on Wayland
    libEGL / libGL                          OpenGL, for the drawing

On Debian and Ubuntu those come from libx11-6, libxcursor1, libxi6, libxrandr2,
libxkbcommon0, libxkbcommon-x11-0, libwayland-client0 and libgl1 or libegl1.
Polylinker does not use GTK, so a GTK package is not among them.

If you have no display at all, `pl` does everything the editor does except show
it to you, including PNG and SVG export.


3. RUN IT
---------

    ./polylinker              open the editor
    ./polylinker my.gb        open the editor on a file
    ./pl --help               the command-line tool

To put `pl` somewhere your shell will find it, move it there yourself:

    mkdir -p ~/.local/bin && mv pl ~/.local/bin/

and make sure ~/.local/bin is on your PATH.

There is no .deb, no .rpm, no Flatpak, no AppImage and no install script -- and
in particular there is nothing here that asks you to pipe a download into a
shell. The tarball is the whole delivery mechanism. The executable bit is set in
the archive, so a plain `tar xzf` is enough and no `chmod` is needed.

The Python extension is loaded from wherever you put it:

    import importlib.util as u
    s = u.spec_from_file_location("polylinker", "./polylinker.so")
    m = u.module_from_spec(s); s.loader.exec_module(m)


UNSIGNED
--------

Linux has no equivalent of SmartScreen or Gatekeeper, so nothing will stop you
running these and nothing will vouch for them either. The SHA-256 you checked in
step 1 proves this copy is byte-for-byte the one published on the release page.
It proves nothing about who published it. Those are different guarantees, and
the second one is now available too: the release page publishes
SHA256SUMS.txt.sig, an Ed25519 signature over that checksum table made by the
release key, and prints the command to check it. The key's public half is
compiled into pl and polylinker, which is what lets `pl update` check a
download without trusting the page it came from. It is not code signing and
does not pretend to be.

If you are not comfortable with that, do not run it. That is a reasonable
position and this file is not going to talk you out of it.
