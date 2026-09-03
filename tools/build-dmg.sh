#!/bin/bash
# Build polylinker-<version>-<platform>.dmg from a dist/ that tools/release.ps1
# has already produced: stage a Polylinker.app bundle from the manifest, put
# it on a disk image beside an Applications link, and write a .sha256 sidecar.
#
# THE POINT OF THIS SCRIPT IS THAT THE BUNDLE HAS NO FILE LIST OF ITS OWN --
# the same rule tools/build-msi.ps1 states for the MSI, for the same reason:
# a second list of files is the thing that drifts, and this project's
# packaging has dropped licence texts twice already through exactly that. So
# nothing here names a payload file. dist/SHA256SUMS.txt -- the manifest
# tools/release.ps1 writes and tools/check-archive.ps1 verifies -- is read,
# and every member of it except the manifest itself goes into the bundle:
# Mach-O executables to Contents/MacOS, everything else to Contents/Resources
# under the same relative path. Which member is an executable is read from
# its bytes (`file`), not from a name. tools/check-dmg.sh reads the same
# manifest and asserts the bundle carries every member with its recorded
# hash and nothing else.
#
# WHY A SHELL SCRIPT AND NOT tools/build-dmg.ps1, when every other packaging
# tool here is PowerShell. hdiutil, plutil, lipo and iconutil exist on macOS
# and nowhere else, so this script can only ever run there, where /bin/bash
# is guaranteed and pwsh is a download. tools/hooks/pre-commit and
# tools/make-demo.py are the precedents for a non-PowerShell tool. What is
# lost is one gate step: tools/ci.ps1's 'every file the release reads is
# committed' parses release.ps1's string literals and does not parse this
# file, so the one committed input this script reads on its own -- the icon
# -- is checked below by asking git directly. (bash 3.2 throughout: that is
# the bash macOS ships, and `mapfile`, `${x,,}` and associative arrays are
# not in it.)
#
# It writes nothing into dist/. tools/release.ps1 hashes EVERYTHING under
# dist/ into the manifest and the tarball (release.yml records the same trap
# for the MSI), so the stage and the image go to --out, which defaults to a
# sibling directory and is refused if it is dist/.
#
# WHAT THIS DOES NOT DO. It does not sign. The bundle carries no
# _CodeSignature and no notarisation ticket, so Gatekeeper refuses it exactly
# as it refuses the bare executables in the tarball, and README-MACOS.txt and
# the release notes give the recursive form of the same xattr remedy. The
# arm64 slices carry the ad-hoc signature rustc's linker writes, which
# identifies nobody and is what lets an arm64 Mach-O load at all; that is
# not the signing docs/RELEASING.md declines. It does not claim
# reproducibility either: hdiutil writes timestamps into the filesystem it
# makes, so unlike the zip and the tarball two runs over one dist/ give two
# different images, and no gate step says otherwise.
#
# Usage:
#   tools/build-dmg.sh [--dist DIR] [--out DIR] [--version X.Y.Z]
#                      [--stage-only] [--keep-stage]
#
#   --stage-only  stage Polylinker.app and stop before hdiutil, so the bundle
#                 can be inspected without making an image. It is NOT a
#                 non-Mac mode, and saying so was wrong until 2026-09-03: the
#                 copy loop uses `cp -X` and the plist is checked with
#                 `plutil`, both of which are macOS-only and both of which run
#                 before this exit.
#   --keep-stage  leave <out>/stage/ beside the image for inspection.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd -P)"
repo="$(cd "$here/.." && pwd -P)"
cd "$repo"

dist='dist'
out='dmg'
version=''
stage_only=0
keep_stage=0
# Defined BEFORE the argument loop, because the loop calls `die`: with the
# definitions below it, a missing flag value produced "die: command not found"
# and then carried on.
say() { printf '  %s\n' "$*"; }
die() { printf 'build-dmg.sh: %s\n' "$*" >&2; exit 1; }
usage() { printf 'build-dmg.sh: %s\n' "$*" >&2; exit 2; }

while [ $# -gt 0 ]; do
    case "$1" in
        # `[ $# -ge 2 ]` before every value read: `--dist` as the last argument
        # would otherwise die with bash's own "$2: unbound variable" under
        # `set -u`, which reads as a crashed script rather than a usage error.
        --dist) [ $# -ge 2 ] || usage '--dist needs a value'; dist="$2"; shift 2 ;;
        --out) [ $# -ge 2 ] || usage '--out needs a value'; out="$2"; shift 2 ;;
        --version) [ $# -ge 2 ] || usage '--version needs a value'; version="$2"; shift 2 ;;
        --stage-only) stage_only=1; shift ;;
        --keep-stage) keep_stage=1; shift ;;
        # To the `set -` line and no further, so the range cannot drift the way
        # a hard-coded one did: this used to truncate --stage-only mid-sentence
        # and omit --keep-stage entirely.
        -h|--help) sed -n '2,/^set /{/^set /!p;}' "$0"; exit 0 ;;
        *) echo "build-dmg.sh: unknown argument '$1'" >&2; exit 2 ;;
    esac
done

# The version is read from the workspace Cargo.toml, which is the single
# source; release.ps1, ci.ps1, build-msi.ps1 and bins/winres.rs all re-read
# that same string rather than restating it.
if [ -z "$version" ]; then
    version="$(grep -m1 -E '^[[:space:]]*version[[:space:]]*=[[:space:]]*"[0-9]+\.[0-9]+\.[0-9]+"' Cargo.toml \
               | sed -E 's/.*"([0-9]+\.[0-9]+\.[0-9]+)".*/\1/')"
    [ -n "$version" ] || die 'could not read the version from Cargo.toml'
fi
# Three NUMERIC fields, which is what the message has always said and what the
# glob `*.*.*` did not check: `--version a.b.c` produced a
# polylinker-a.b.c-macos-*.dmg and an Info.plist to match.
printf '%s' "$version" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$' \
    || die "the version must be three numeric fields, not '$version'"

# Normalised with pwd -P, and compared as strings, for build-msi.ps1's reason:
# the guard that keeps the image out of dist/ has to compare one spelling of
# each directory, not whatever spelling the caller typed.
[ -d "$dist" ] || die "there is no $dist/. Run tools/release.ps1 first."
dist_full="$(cd "$dist" && pwd -P)"
manifest="$dist_full/SHA256SUMS.txt"
[ -f "$manifest" ] || die "there is no SHA256SUMS.txt in $dist_full. Run tools/release.ps1 first."
mkdir -p "$out"
out_full="$(cd "$out" && pwd -P)"
# UNDER dist/, not merely EQUAL to it. The header says this script writes
# nothing into dist/, and an equality test let `--out dist/dmg` through, which
# leaves an image inside the very directory `tools/release.ps1` hashes.
case "$out_full/" in
    "$dist_full"/*)
        # `rmdir`, so the refusal leaves nothing behind: the normalisation
        # above needs the directory to exist, and creating one inside dist/
        # only to refuse it would put a stray entry in the very place this is
        # defending. `rmdir` removes it only if it is empty, so a caller who
        # pointed --out at a directory that already had something in it keeps
        # it.
        rmdir "$out_full" 2>/dev/null || true
        die 'the image must not be written into or under dist/; see the comment at the top of this script' ;;
esac

# ---------------------------------------------------------------- the manifest
# Format: a header, a '--' line, then '<sha256>  <relative path>' per file.
# The header's `platform:` line is read too, for the image's name and for the
# refusal below: a .dmg of a Windows payload is not a thing.
platform=''
members=()
past=0
while IFS= read -r line || [ -n "$line" ]; do
    if [ "$past" -eq 0 ]; then
        case "$line" in
            --) past=1 ;;
            platform:*) platform="$(printf '%s' "${line#platform:}" | tr -d '[:space:]')" ;;
        esac
        continue
    fi
    if printf '%s' "$line" | grep -qE '^[0-9a-f]{64}  .+$'; then
        members+=("${line:66}")
    fi
done < "$manifest"
[ "${#members[@]}" -gt 0 ] || die "no file lines parsed out of $manifest"
[ -n "$platform" ] || die "$manifest has no 'platform:' line; a dist/ without it did not come from tools/release.ps1"
case "$platform" in
    macos-*) ;;
    *) die "this dist/ says 'platform: $platform', and a .dmg is a macOS disk image. It is built only from a macos-* dist/." ;;
esac
say "platform: $platform (from the manifest header)"

# ------------------------------------------------------------------ the icon
# The one input this script reads that is not in dist/. Taken from where it
# is DRAWN, as release.ps1 takes the .ico, and required to be committed for
# the reason tools/ci.ps1's 'every file the release reads is committed' step
# gives -- that step parses release.ps1 and cannot see this file, so the
# question is asked here instead.
icon="bins/pl-gui/icon/polylinker.icns"
[ -f "$icon" ] || die "$icon is missing; python3 bins/pl-gui/icon/build-icns.py draws it"
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git ls-files --error-unmatch "$icon" >/dev/null 2>&1 \
        || die "$icon is not committed; a release input that is not in the tree is a release nobody can rebuild"
fi

# ------------------------------------------------------------------ the stage
stage="$out_full/stage"
app="$stage/Polylinker.app"
rm -rf "$stage"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

# Every member except the manifest, placed by what it IS: `file` reads the
# Mach-O header, so an executable that was renamed would still land in MacOS/
# and a text file called `polylinker` would not. Subdirectories (licences/,
# features/) are kept under Resources/.
programs=()
resources=0
excluded=0
for m in "${members[@]}"; do
    # Counted, not assumed: `tools/release.ps1` never lists SHA256SUMS.txt in
    # its own manifest, so this arm normally never fires -- and the first
    # version of this script reported "1 excluded" every time regardless.
    if [ "$m" = 'SHA256SUMS.txt' ]; then
        excluded=$((excluded + 1))
        continue
    fi
    src="$dist_full/$m"
    [ -f "$src" ] || die "the manifest lists $m but it is not on disk in $dist_full"
    kind="$(file -b "$src")"
    case "$m" in
        */*) is_top=0 ;;
        *) is_top=1 ;;
    esac
    if [ "$is_top" -eq 1 ] && printf '%s' "$kind" | grep -qE 'Mach-O.*executable'; then
        dest="$app/Contents/MacOS/$m"
        cp -X "$src" "$dest"
        chmod 0755 "$dest"
        programs+=("$m")
    else
        dest="$app/Contents/Resources/$m"
        mkdir -p "$(dirname "$dest")"
        cp -X "$src" "$dest"
        chmod 0644 "$dest"
        resources=$((resources + 1))
    fi
done
[ "${#programs[@]}" -gt 0 ] || die 'no Mach-O executable in the manifest; there is nothing to make a bundle of'
have_editor=0
for p in "${programs[@]}"; do [ "$p" = 'polylinker' ] && have_editor=1; done
[ "$have_editor" -eq 1 ] || die "the manifest's executables are: ${programs[*]} -- none is 'polylinker', which is what CFBundleExecutable names"
say "manifest: ${#members[@]} file(s); ${#programs[@]} program(s) into Contents/MacOS (${programs[*]}), $resources into Contents/Resources$( [ "$excluded" -gt 0 ] && printf ', %s excluded (SHA256SUMS.txt)' "$excluded")"

cp -X "$icon" "$app/Contents/Resources/Polylinker.icns"
chmod 0644 "$app/Contents/Resources/Polylinker.icns"
printf 'APPL????' > "$app/Contents/PkgInfo"

# Info.plist.
#
# CFBundleIdentifier IS PERMANENT, like the MSI's UpgradeCode: LaunchServices
# keys its registrations and the app's defaults domain on it, and changing it
# makes a later release a different application to macOS. Minted 2026-09-03
# as the reverse of the repository's home, since the project has no domain;
# tools/check-dmg.sh asserts this exact string so it cannot be edited by
# accident.
#
# NO CFBundleDocumentTypes, and that is measured rather than tidy. The editor
# opens files from argv (`open_argv` in bins/pl-gui/src/main.rs) and from
# drag-and-drop; a Finder double-click on a document reaches an application
# as an Apple Event, which nothing in winit/eframe delivers to this program.
# Registering the eight sequence extensions here would put Polylinker in
# every "Open With" menu and have it open to an empty window -- the .plproj
# mistake tools/installer/Polylinker.wxs refuses, on every format at once.
# README-MACOS.txt says so. `./Contents/MacOS/polylinker file.gb` and
# dropping a file on the window both work.
#
# LSMinimumSystemVersion 11.0 is what the release notes promise ("macOS 11+")
# and is the floor of the aarch64-apple-darwin target.
copyright='Copyright 2026 The Polylinker contributors. MIT OR Apache-2.0.'
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>Polylinker</string>
	<key>CFBundleExecutable</key>
	<string>polylinker</string>
	<key>CFBundleIconFile</key>
	<string>Polylinker</string>
	<key>CFBundleIconName</key>
	<string>Polylinker</string>
	<key>CFBundleIdentifier</key>
	<string>io.github.liorlobel.polylinker</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Polylinker</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$version</string>
	<key>CFBundleVersion</key>
	<string>$version</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.medical</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSHumanReadableCopyright</key>
	<string>$copyright</string>
	<key>NSPrincipalClass</key>
	<string>NSApplication</string>
	<key>NSSupportsAutomaticGraphicsSwitching</key>
	<true/>
</dict>
</plist>
EOF
plutil -lint -s "$app/Contents/Info.plist" >/dev/null || die 'the Info.plist this script wrote does not parse'

# The conventional drag target. A symlink, so the image carries no copy of
# anything.
ln -s /Applications "$stage/Applications"

# Nothing staged from this checkout may carry the download tag: a bundle
# whose files were quarantined before it was even packed would be refused for
# a reason the release notes do not describe.
if xattr -lr "$app" 2>/dev/null | grep -q 'com.apple.quarantine'; then
    die 'a staged file carries com.apple.quarantine; the payload came from a download, not from a build'
fi

say "staged: $app"
say "  Contents/MacOS:     $(ls "$app/Contents/MacOS" | tr '\n' ' ')"
say "  Contents/Resources: $(find "$app/Contents/Resources" -type f | wc -l | tr -d ' ') file(s), including Polylinker.icns"

if [ "$stage_only" -eq 1 ]; then
    say '--stage-only: stopping before hdiutil'
    exit 0
fi

# ------------------------------------------------------------------ the image
name="polylinker-$version-$platform.dmg"
image="$out_full/$name"
rm -f "$image"
# UDZO (zlib-compressed, read-only) on HFS+: the format every macOS since 10.x
# mounts by double-click, and the one that carries a symlink and POSIX modes
# without fuss. `-ov` because a previous run may have left one.
hdiutil create -quiet -volname 'Polylinker' -srcfolder "$stage" -fs 'HFS+' \
    -format UDZO -imagekey zlib-level=9 -ov "$image"
[ -f "$image" ] || die "hdiutil reported success but $image is not there"
hdiutil verify -quiet "$image" || die "$name does not verify"

# The sidecar, in the shape release.ps1 and build-msi.ps1 write: the digest,
# two spaces, the bare file name, no newline.
sha="$(shasum -a 256 "$image" | awk '{print $1}')"
printf '%s  %s' "$sha" "$name" > "$image.sha256"

[ "$keep_stage" -eq 1 ] || rm -rf "$stage"

bytes="$(stat -f '%z' "$image")"
say "$name  $bytes bytes"
say "$sha"
