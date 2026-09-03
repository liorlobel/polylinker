#!/bin/bash
# Mount the .dmg, assert everything the bundle on it claims, and unmount it.
#
# A CHECK THAT CANNOT FAIL PROVES NOTHING, so this does not read the image's
# own catalogue and conclude it looks right. It attaches the image the way a
# double-click does, and then looks at the volume: what is at its root, what
# the bundle's Info.plist says, whether the program inside reports the version
# the plist declares, whether every manifest member is inside with the hash
# the manifest recorded, whether anything ELSE is inside, and which
# architectures each Mach-O actually carries against what the manifest's
# `platform:` label claims. With --launch it also starts the editor from the
# mounted bundle with PL_GUI_SMOKE=1 and requires it to come back 0, which is
# the only evidence short of a person that the bundle opens a window.
#
# The sibling of tools/check-msi.ps1, and a shell script for the reason
# tools/build-dmg.sh gives: hdiutil, plutil and lipo run on macOS and nowhere
# else. bash 3.2 throughout.
#
# Usage:
#   tools/check-dmg.sh --dmg FILE [--dist DIR] [--launch]
#
#   --dist    the dist/ the image was built from; its SHA256SUMS.txt is what
#             the bundle is held to. Default dist.
#   --launch  run Contents/MacOS/polylinker from the mounted image with
#             PL_GUI_SMOKE=1, PL_GUI_RENDERER cleared out of the environment
#             (so the program's own glow-then-wgpu plan is what runs, even
#             when the caller has pinned one by hand) and HOME pointed at a
#             throwaway directory, under a 180 s alarm.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd -P)"
repo="$(cd "$here/.." && pwd -P)"
cd "$repo"

dmg=''
dist='dist'
launch=0
while [ $# -gt 0 ]; do
    case "$1" in
        # `[ $# -ge 2 ]` before every value read: `--dmg` as the last argument
        # would otherwise die with bash's own "$2: unbound variable" under
        # `set -u`, which reads as a crashed script rather than as the usage
        # error it is.
        --dmg) [ $# -ge 2 ] || { echo 'check-dmg.sh: --dmg needs a value' >&2; exit 2; }; dmg="$2"; shift 2 ;;
        --dist) [ $# -ge 2 ] || { echo 'check-dmg.sh: --dist needs a value' >&2; exit 2; }; dist="$2"; shift 2 ;;
        --launch) launch=1; shift ;;
        # To the `set -` line and no further, so the range cannot drift the way
        # a hard-coded one did: this printed `set -uo pipefail` as its last line.
        -h|--help) sed -n '2,/^set /{/^set /!p;}' "$0"; exit 0 ;;
        *) echo "check-dmg.sh: unknown argument '$1'" >&2; exit 2 ;;
    esac
done
[ -n "$dmg" ] || { echo 'check-dmg.sh: --dmg is required' >&2; exit 2; }
[ -f "$dmg" ] || { echo "check-dmg.sh: $dmg does not exist" >&2; exit 2; }
[ -f "$dist/SHA256SUMS.txt" ] || { echo "check-dmg.sh: no SHA256SUMS.txt in $dist" >&2; exit 2; }

fail=()
bad() { fail+=("$*"); }
note() { printf '      %s\n' "$*"; }
leaf="$(basename "$dmg")"

# ---------------------------------------------------------------- the manifest
# Members and their digests, and the platform label. Read from the manifest
# rather than restated, for the same reason the bundle itself is built from
# it.
platform=''
members=()
digests=()
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
        digests+=("${line:0:64}")
        members+=("${line:66}")
    fi
done < "$dist/SHA256SUMS.txt"
[ "${#members[@]}" -gt 0 ] || { echo 'check-dmg.sh: parsed no members out of the manifest' >&2; exit 2; }
[ -n "$platform" ] || { echo 'check-dmg.sh: the manifest has no platform: line' >&2; exit 2; }

# What the platform label promises about every Mach-O inside. An unknown
# label is a failure and not a default, for build-msi.ps1's reason.
case "$platform" in
    macos-universal) want_archs='arm64 x86_64' ;;
    macos-arm64) want_archs='arm64' ;;
    macos-x64|macos-x86_64) want_archs='x86_64' ;;
    *) echo "check-dmg.sh: the manifest says 'platform: $platform', which is not a macOS label this check knows" >&2; exit 2 ;;
esac
note "platform: $platform -> every Mach-O must carry exactly: $want_archs"

# ------------------------------------------------------------------ attach
# `pwd -P` after mktemp, because macOS's TMPDIR is /var/folders/... and
# `mount` reports the same directory as /private/var/folders/...: the first
# version of this script compared the unresolved spelling, never saw its own
# mount, and left every image it checked attached.
# Two statements, not one: `mnt="$(cd "$(mktemp -d ...)" && pwd -P)"` swallows
# a failed mktemp, because `cd ""` returns 0 without moving in bash 3.2 and
# `pwd -P` then reports the repository root -- which would be handed to
# `hdiutil attach -mountpoint`, shadowing the checkout until detach.
mnt="$(mktemp -d "${TMPDIR:-/tmp}/pl-dmg-check.XXXXXX")" \
    || { echo 'check-dmg.sh: mktemp could not make a mountpoint' >&2; exit 2; }
mnt="$(cd "$mnt" && pwd -P)"
detach() {
    # -F: `$mnt` is data here, not a pattern. A TMPDIR holding a regex
    # metacharacter (`[`, `.`, `*`) made this test miss its own mount and
    # leave the image attached.
    if mount | grep -qF -- " on $mnt "; then
        hdiutil detach -quiet "$mnt" 2>/dev/null || hdiutil detach -force -quiet "$mnt" 2>/dev/null || true
    fi
    rmdir "$mnt" 2>/dev/null || true
}
trap detach EXIT
printf '  attaching %s\n' "$leaf"
# stderr captured and printed rather than suppressed: `-quiet` hides the one
# sentence that says WHY, and "Resource busy" (the image already open in
# Finder) reads nothing like a corrupt image but fails identically.
if ! attach_err="$(hdiutil attach -quiet -nobrowse -readonly -noautoopen -mountpoint "$mnt" "$dmg" 2>&1)"; then
    echo "FAIL  $leaf: hdiutil could not attach it${attach_err:+: $attach_err}" >&2
    exit 1
fi

# ---------------------------------------------------------------- the root
# Exactly the bundle and the Applications link. Finder's own dotfiles are
# ignored; anything else at the root is a file a user will see and wonder
# about.
root_extra=()
seen_app=0
seen_link=0
for entry in "$mnt"/* "$mnt"/.[!.]*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    n="$(basename "$entry")"
    case "$n" in
        .DS_Store|.fseventsd|.Trashes|.background|.VolumeIcon.icns|.metadata_never_index) continue ;;
        Polylinker.app) seen_app=1 ;;
        Applications)
            seen_link=1
            target="$(readlink "$entry" || true)"
            [ "$target" = '/Applications' ] || bad "the Applications entry points at '$target', not /Applications"
            ;;
        *) root_extra+=("$n") ;;
    esac
done
[ "$seen_app" -eq 1 ] || bad 'there is no Polylinker.app at the root of the image'
[ "$seen_link" -eq 1 ] || bad 'there is no Applications link at the root of the image'
[ "${#root_extra[@]}" -eq 0 ] || bad "unexpected entries at the root of the image: ${root_extra[*]}"
app="$mnt/Polylinker.app"
# `bad` FIRST, so the array is never empty when it is expanded: bash 3.2 under
# `set -u` treats `"${empty[@]}"` as an unbound variable and aborts, which
# turned "Polylinker.app is a file, not a bundle" into a shell error.
if [ ! -d "$app" ]; then
    bad 'Polylinker.app at the root of the image is not a directory'
    printf 'FAIL  %s\n' "$leaf"
    printf '      %s\n' "${fail[@]}"
    exit 1
fi

# -------------------------------------------------------------- Info.plist
plist="$app/Contents/Info.plist"
if [ ! -f "$plist" ]; then
    bad 'no Contents/Info.plist'
else
    plutil -lint -s "$plist" >/dev/null 2>&1 || bad 'Contents/Info.plist does not parse'
    key() { plutil -extract "$1" raw -o - "$plist" 2>/dev/null || true; }
    exe="$(key CFBundleExecutable)"
    ident="$(key CFBundleIdentifier)"
    short="$(key CFBundleShortVersionString)"
    build="$(key CFBundleVersion)"
    iconfile="$(key CFBundleIconFile)"
    minos="$(key LSMinimumSystemVersion)"
    pkgtype="$(key CFBundlePackageType)"
    note "Info.plist: $ident $short, executable '$exe', icon '$iconfile', macOS >= $minos"

    [ "$exe" = 'polylinker' ] || bad "CFBundleExecutable is '$exe', not polylinker"
    [ -x "$app/Contents/MacOS/$exe" ] || bad "Contents/MacOS/$exe is missing or not executable"
    # Permanent, like the MSI UpgradeCode: see tools/build-dmg.sh.
    [ "$ident" = 'io.github.liorlobel.polylinker' ] \
        || bad "CFBundleIdentifier is '$ident'; it was minted as io.github.liorlobel.polylinker on 2026-09-03 and is permanent"
    [ "$pkgtype" = 'APPL' ] || bad "CFBundlePackageType is '$pkgtype', not APPL"
    [ -n "$minos" ] || bad 'no LSMinimumSystemVersion; the release notes promise macOS 11+'
    [ "$short" = "$build" ] || bad "CFBundleShortVersionString '$short' and CFBundleVersion '$build' disagree"
    case "$iconfile" in
        *.icns) iconpath="$app/Contents/Resources/$iconfile" ;;
        *) iconpath="$app/Contents/Resources/$iconfile.icns" ;;
    esac
    [ -f "$iconpath" ] || bad "CFBundleIconFile names '$iconfile' but $(basename "$iconpath") is not in Contents/Resources"
    [ "$(cat "$app/Contents/PkgInfo" 2>/dev/null)" = 'APPL????' ] || bad 'Contents/PkgInfo is missing or is not APPL????'
fi

# ----------------------------------------------- the program inside agrees
# The version the plist declares against the version the binary reports. They
# can only disagree if the bundle and its payload came from different builds,
# which a stale dist/ against a bumped Cargo.toml is exactly how to produce --
# tools/check-msi.ps1 records that this comparison was missing for weeks.
pl="$app/Contents/MacOS/pl"
if [ -x "$pl" ]; then
    ver="$("$pl" --version 2>&1 | tr -d '\r' | head -1)"
    note "pl --version -> $ver"
    plver="$(printf '%s' "$ver" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
    [ -n "$plver" ] || bad "the pl inside the bundle did not report a version: $ver"
    if [ -n "${short:-}" ] && [ -n "$plver" ] && [ "$plver" != "$short" ]; then
        bad "Info.plist declares version '$short' but the pl inside reports '$plver'; the bundle and its payload are from different builds"
    fi
else
    bad 'Contents/MacOS/pl is missing or not executable'
fi

# -------------------------------------------------------------- the payload
# Every manifest member except the manifest itself, at the place its kind
# puts it, with its recorded digest. The placement rule is re-derived from
# the bytes IN THE BUNDLE, not copied from build-dmg.sh: a Mach-O executable
# belongs in MacOS/, anything else in Resources/, and a member found in the
# other place is a finding.
i=0
present=0
excluded=0
for m in "${members[@]}"; do
    d="${digests[$i]}"
    i=$((i + 1))
    # Counted rather than assumed. `tools/release.ps1` builds the manifest from
    # everything under dist/ EXCEPT SHA256SUMS.txt, so this arm normally never
    # fires -- and the first version of this script subtracted one anyway and
    # reported "18 of 17 ... present", a count that could not be right in
    # either direction.
    if [ "$m" = 'SHA256SUMS.txt' ]; then
        excluded=$((excluded + 1))
        continue
    fi
    in_macos="$app/Contents/MacOS/$m"
    in_res="$app/Contents/Resources/$m"
    if [ -f "$in_macos" ] && [ -f "$in_res" ]; then
        bad "$m is in both Contents/MacOS and Contents/Resources"
        continue
    fi
    if [ -f "$in_macos" ]; then
        path="$in_macos"
        file -b "$path" | grep -qE 'Mach-O.*executable' || bad "$m is in Contents/MacOS but is not a Mach-O executable: $(file -b "$path")"
    elif [ -f "$in_res" ]; then
        path="$in_res"
        if file -b "$path" | grep -qE 'Mach-O.*executable'; then
            case "$m" in */*) ;; *) bad "$m is a Mach-O executable and sits in Contents/Resources rather than Contents/MacOS" ;; esac
        fi
    else
        bad "$m is in the manifest but nowhere in the bundle"
        continue
    fi
    got="$(shasum -a 256 "$path" | awk '{print $1}')"
    if [ "$got" != "$d" ]; then
        bad "$m: sha256 $got in the bundle, $d in the manifest"
    else
        present=$((present + 1))
    fi
done
note "$present of $((${#members[@]} - excluded)) manifest member(s) present with their recorded digests$( [ "$excluded" -gt 0 ] && printf ', %s excluded' "$excluded")"

# --------------------------------------------------------- and nothing else
# The bundle's own three files, the icon, and the members. A file that is not
# one of those is a file no manifest covers, which is where a licence text or
# a stray build product goes to hide.
allowed="$(mktemp "${TMPDIR:-/tmp}/pl-dmg-allowed.XXXXXX")"
{
    printf '%s\n' 'Contents/Info.plist' 'Contents/PkgInfo' "Contents/Resources/$(basename "${iconpath:-Polylinker.icns}")"
    for m in "${members[@]}"; do
        [ "$m" = 'SHA256SUMS.txt' ] && continue
        printf 'Contents/MacOS/%s\n' "$m"
        printf 'Contents/Resources/%s\n' "$m"
    done
} | sort -u > "$allowed"
extra=()
# `-type f -o -type l`, not `-type f`: a symlink is a file a user gets, and
# `-type f` followed it or ignored it rather than reporting it -- a bundle
# carrying `Contents/Resources/x -> /etc/passwd` passed. Empty directories are
# asked for separately, since `find -type d` would otherwise report every real
# one. `${f#"$app/"}` with the pattern QUOTED: unquoted, a mountpoint holding
# a glob metacharacter made the prefix a pattern that stripped nothing.
while IFS= read -r f; do
    rel="${f#"$app/"}"
    grep -qxF -- "$rel" "$allowed" || extra+=("$rel")
done < <(find "$app" \( -type f -o -type l \) | sort)
rm -f "$allowed"
[ "${#extra[@]}" -eq 0 ] || bad "files in the bundle that no manifest covers: ${extra[*]}"
empty_dirs=()
while IFS= read -r f; do
    empty_dirs+=("${f#"$app/"}")
done < <(find "$app" -type d -empty | sort)
[ "${#empty_dirs[@]}" -eq 0 ] || bad "empty directories in the bundle: ${empty_dirs[*]}"

# ----------------------------------------------------------- the slices
# `lipo -archs` on every Mach-O, held to the platform label. This is the
# analogue of check-msi.ps1's Template check: the label says what the bytes
# are, and the bytes are asked.
while IFS= read -r f; do
    file -b "$f" | grep -q 'Mach-O' || continue
    archs="$(lipo -archs "$f" 2>/dev/null | tr ' ' '\n' | sort | tr '\n' ' ' | sed 's/ $//')"
    if [ "$archs" != "$want_archs" ]; then
        bad "$(basename "$f") carries [$archs] but 'platform: $platform' promises [$want_archs]"
    else
        note "$(basename "$f"): $archs"
    fi
done < <(find "$app/Contents/MacOS" "$app/Contents/Resources" -type f | sort)

# --------------------------------------------------------------- the icon
# The container is walked, not trusted: magic, total length, and one PNG per
# entry whose IHDR is square. python3 ships with the Xcode command-line tools
# and on every GitHub macOS runner.
if [ -f "${iconpath:-/nonexistent}" ]; then
    if ! icon_report="$(python3 - "$iconpath" <<'PY'
import struct, sys
data = open(sys.argv[1], 'rb').read()
if data[:4] != b'icns':
    sys.exit('not an ICNS: bad magic')
total = struct.unpack('>I', data[4:8])[0]
if total != len(data):
    sys.exit(f'ICNS header says {total} bytes, file is {len(data)}')
off, n = 8, 0
while off < len(data):
    kind = data[off:off + 4].decode('latin-1')
    length = struct.unpack('>I', data[off + 4:off + 8])[0]
    payload = data[off + 8:off + length]
    if payload[:8] != b'\x89PNG\r\n\x1a\n':
        sys.exit(f'entry {kind} is not a PNG')
    w, h = struct.unpack('>II', payload[16:24])
    if w != h or w < 16:
        sys.exit(f'entry {kind} is {w}x{h}')
    n += 1
    off += length
print(f'{n} PNG entries')
PY
    )"; then
        bad "the icon does not walk: $icon_report"
    else
        note "icon: $icon_report"
    fi
fi

# ------------------------------------------------------------- quarantine
if xattr -lr "$app" 2>/dev/null | grep -q 'com.apple.quarantine'; then
    bad 'a file in the bundle carries com.apple.quarantine; the payload was downloaded, not built'
fi

# ------------------------------------------------------------ the launch
# Run from the read-only mount, as a user who double-clicks the image and
# then the icon would. PL_GUI_SMOKE=1 closes the window from its first frame
# (bins/pl-gui/src/main.rs); the renderer is left to the program's own
# glow-then-wgpu plan. The alarm is perl's because coreutils `timeout` is
# not on macOS.
if [ "$launch" -eq 1 ]; then
    printf '  launching Contents/MacOS/polylinker with PL_GUI_SMOKE=1\n'
    # HOME, not XDG_STATE_HOME. `state_base` in bins/pl-gui/src/recover.rs
    # takes its macOS branch first and returns `$HOME/Library/Application
    # Support/Polylinker`, reading no XDG variable at all -- so the XDG
    # spelling this used until 2026-09-03 isolated nothing, and the smoke run
    # claimed, and could delete, the session files of whoever ran the check.
    # `env -u PL_GUI_RENDERER` because the header promises the program's own
    # glow-then-wgpu plan, and a maintainer's exported override would
    # otherwise pin one silently.
    launch_home="$(mktemp -d "${TMPDIR:-/tmp}/pl-dmg-home.XXXXXX")" \
        || { bad 'mktemp could not make a throwaway HOME for the launch'; launch_home=''; }
    if [ -n "$launch_home" ]; then
        # `or die` on the exec: perl's exec RETURNS on failure and the
        # one-liner then falls off the end with status 0, so a missing or
        # unrunnable binary -- an x86_64-only slice on an arm64 runner, say --
        # was reported as "opened a window and exited 0".
        env -u PL_GUI_RENDERER HOME="$launch_home" PL_GUI_SMOKE=1 \
            perl -e 'alarm 180; exec @ARGV or die "exec $ARGV[0]: $!\n"' \
            -- "$app/Contents/MacOS/polylinker"
        rc=$?
        rm -rf "$launch_home"
        if [ "$rc" -ne 0 ]; then
            bad "the editor launched from the mounted bundle exited $rc"
        else
            note 'the editor opened a window from the mounted bundle and exited 0'
        fi
    fi
fi

if [ "${#fail[@]}" -gt 0 ]; then
    printf 'FAIL  %s\n' "$leaf"
    printf '      %s\n' "${fail[@]}"
    exit 1
fi
printf 'OK    %s: attached, bundle asserted against the manifest, detached\n' "$leaf"
exit 0
