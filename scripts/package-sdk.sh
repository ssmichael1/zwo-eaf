#!/usr/bin/env bash
# Package the ZWO EAF SDK into one normalized tarball per Rust target.
#
# Usage: scripts/package-sdk.sh <path-to-EAF_SDK_Vx.y.z> [version]
#
# The input directory must contain the extracted Linux/macOS SDK at `eaf/`
# and the Windows SDK at `EAF_Windows_SDK_Vx.y.z/`. Output goes to `dist/`:
#   dist/eaf-sdk-<version>-<target>.tar.gz   (one per target)
#   dist/SHA256SUMS
#
# Every tarball has the same layout so build.rs needs no per-platform logic:
#   include/EAF_focuser.h
#   lib/<platform library files, flat>
#   LICENSE
#   eaf.rules            (linux only; udev rule for non-root HID access)
set -euo pipefail

SDK_DIR="${1:?usage: $0 <EAF_SDK_dir> [version]}"
VERSION="${2:-}"
if [[ -z "$VERSION" ]]; then
    # Infer from the directory name, e.g. EAF_SDK_V1.8.1 -> 1.8.1
    VERSION="$(basename "$SDK_DIR" | sed -nE 's/.*V([0-9]+\.[0-9]+\.[0-9]+).*/\1/p')"
    [[ -n "$VERSION" ]] || { echo "cannot infer version from $SDK_DIR; pass it explicitly" >&2; exit 1; }
fi

UNIX_SDK="$SDK_DIR/eaf"
WIN_SDK="$(ls -d "$SDK_DIR"/EAF_Windows_SDK_V* 2>/dev/null | head -1 || true)"
[[ -f "$UNIX_SDK/include/EAF_focuser.h" ]] || { echo "missing $UNIX_SDK/include/EAF_focuser.h" >&2; exit 1; }
[[ -n "$WIN_SDK" && -f "$WIN_SDK/include/EAF_focuser.h" ]] || { echo "missing Windows SDK under $SDK_DIR" >&2; exit 1; }

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$REPO_ROOT/dist"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$DIST"

# Avoid macOS AppleDouble (._*) entries and xattrs in the archives.
export COPYFILE_DISABLE=1

# target  source-lib-dir  os
TARGETS=(
    "macos-aarch64   $UNIX_SDK/lib/mac_arm64                 macos"
    "macos-x86_64    $UNIX_SDK/lib/mac_x64                   macos"
    "linux-x86_64    $UNIX_SDK/lib/x64                       linux"
    "linux-x86       $UNIX_SDK/lib/x86                       linux"
    "linux-armv6     $UNIX_SDK/lib/armv6                     linux"
    "linux-armv7     $UNIX_SDK/lib/armv7                     linux"
    "linux-aarch64   $UNIX_SDK/lib/armv8                     linux"
    "windows-x86_64  $WIN_SDK/lib/Windows/x64/Release        windows"
    "windows-x86     $WIN_SDK/lib/Windows/Win32/Release      windows"
)

for entry in "${TARGETS[@]}"; do
    read -r target libdir os <<<"$entry"
    name="eaf-sdk-$VERSION-$target"
    root="$STAGE/$name"
    rm -rf "$root"
    mkdir -p "$root/include" "$root/lib"

    cp "$UNIX_SDK/include/EAF_focuser.h" "$root/include/"
    cp "$UNIX_SDK/license.txt" "$root/LICENSE"

    case "$os" in
        macos)
            cp "$libdir/libEAFFocuser.a" "$libdir/libEAFFocuser.dylib" "$root/lib/"
            ;;
        linux)
            cp "$libdir"/libEAFFocuser.a "$libdir"/libEAFFocuser.so* "$root/lib/"
            # BLE helper libs are only shipped for some arches.
            for f in libWrapperSdbus.so libsdbus-c++.so.2; do
                [[ -f "$libdir/$f" ]] && cp "$libdir/$f" "$root/lib/"
            done
            cp "$UNIX_SDK/lib/eaf.rules" "$root/eaf.rules"
            ;;
        windows)
            cp "$libdir/EAF_focuser.lib" "$libdir/EAF_focuser.dll" "$root/lib/"
            ;;
    esac

    # Strip quarantine/resource-fork xattrs so the archive is clean.
    xattr -rc "$root" 2>/dev/null || true

    out="$DIST/$name.tar.gz"
    # Archive contents relative to the tarball root (no top-level dir), sorted
    # for stable output. gzip -n omits the timestamp for reproducibility.
    (cd "$root" && find . -type f -o -type l | sort | tar -cf - --no-recursion -T - ) | gzip -n -9 > "$out"
    echo "wrote $out"
done

(cd "$DIST" && shasum -a 256 eaf-sdk-"$VERSION"-*.tar.gz > SHA256SUMS && cat SHA256SUMS)
