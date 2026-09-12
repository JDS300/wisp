#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
# shellcheck source=version.sh
source "$here/version.sh"

version="$(wisp_version)"
if [[ -z "$version" ]]; then
    echo "release.sh: could not read the workspace version" >&2
    exit 1
fi

# Two channels, decided by the version string alone (spec §4.4). A version
# with a prerelease suffix is a beta and its AppImage points at `latest-pre`,
# which Gear Lever resolves to the newest non-draft release of any kind; a
# plain version points at `latest`, which GitHub's own API resolves to the
# newest non-prerelease. A live user is therefore never offered a beta.
if [[ "$version" == *-* ]]; then
    channel="beta"
    update_channel="latest-pre"
else
    channel="live"
    update_channel="latest"
fi
echo "release.sh: channel $channel ($update_channel)" >&2

if ! command -v zsyncmake >/dev/null 2>&1; then
    echo "release.sh: zsyncmake not found (package: zsync); appimagetool -u writes no .zsync without it" >&2
    exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
    echo "release.sh: curl not found (package: curl)" >&2
    exit 1
fi

# Pinned external artifacts (see docs/plans/2026-09-09-spec-4-packaging.md,
# "Pinned external artifacts"). Cached outside the repository and never
# committed; a cache hit skips the download entirely.
appimagetool_release="1.9.1"
appimagetool_file="appimagetool-${appimagetool_release}-x86_64.AppImage"
appimagetool_url="https://github.com/AppImage/appimagetool/releases/download/${appimagetool_release}/appimagetool-x86_64.AppImage"
appimagetool_sha="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"

runtime_release="20251108"
runtime_file="runtime-x86_64-${runtime_release}"
runtime_url="https://github.com/AppImage/type2-runtime/releases/download/${runtime_release}/runtime-x86_64"
runtime_sha="2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"

cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/wisp-packaging"
mkdir -p "$cache_dir"

# Downloads only when the cached file's hash does not already match.
ensure_cached() {
    local file="$1" url="$2" sha="$3"
    local path="$cache_dir/$file"
    if [[ ! -f "$path" ]] || ! sha256sum -c --quiet - <<<"$sha  $path" >/dev/null 2>&1; then
        curl -fsSL -o "$path" "$url"
    fi
    if ! sha256sum -c --quiet - <<<"$sha  $path" >/dev/null 2>&1; then
        echo "release.sh: $file does not match its pinned SHA-256 ($sha)" >&2
        exit 1
    fi
}

ensure_cached "$appimagetool_file" "$appimagetool_url" "$appimagetool_sha"
ensure_cached "$runtime_file" "$runtime_url" "$runtime_sha"
chmod +x "$cache_dir/$appimagetool_file"

cargo build --release --workspace --target x86_64-unknown-linux-musl --manifest-path "$root/Cargo.toml" 1>&2

musl_dir="$root/target/x86_64-unknown-linux-musl/release"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

tar_root="$stage/tar/wisp-${version}"
mkdir -p "$tar_root"
for bin in wisp wispd wisp-hud; do
    install -m 0755 "$musl_dir/$bin" "$tar_root/$bin"
    strip "$tar_root/$bin"
done
install -m 0755 "$here/install.sh" "$tar_root/install.sh"
install -m 0644 "$root/LICENSE" "$tar_root/LICENSE"
install -m 0644 "$root/THIRD_PARTY.md" "$tar_root/THIRD_PARTY.md"
install -m 0644 "$root/README.md" "$tar_root/README.md"
install -m 0644 "$here/io.github.jds300.Wisp.desktop" "$tar_root/io.github.jds300.Wisp.desktop"
install -m 0644 "$here/io.github.jds300.Wisp.svg" "$tar_root/io.github.jds300.Wisp.svg"
install -m 0644 "$here/io.github.jds300.Wisp.metainfo.xml" "$tar_root/io.github.jds300.Wisp.metainfo.xml"

dist="$root/dist"
mkdir -p "$dist"

tarball="$dist/wisp-${version}-x86_64-linux.tar.gz"
appimage="$dist/Wisp-${version}-x86_64.AppImage"
zsync="$dist/Wisp-${version}-x86_64.AppImage.zsync"
sums="$dist/SHA256SUMS"

# Only the four names this run is about to write: a human may have put
# something else in dist/, and this must not rm -rf it.
rm -f "$tarball" "$appimage" "$zsync" "$sums"

tar -C "$stage/tar" -czf "$tarball" "wisp-${version}"

appdir="$stage/AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" \
    "$appdir/usr/share/icons/hicolor/scalable/apps" "$appdir/usr/share/metainfo"
for bin in wisp wispd wisp-hud; do
    install -m 0755 "$tar_root/$bin" "$appdir/usr/bin/$bin"
done
# The AppDir copies carry X-AppImage-Version so AppImage managers (Gear
# Lever, AppImageLauncher) can show the installed version; the shared
# source file stays unversioned because the tarball and the Flatpak use it
# too. The key must be under [Desktop Entry], so it goes right after it.
sed "/^\[Desktop Entry\]$/a X-AppImage-Version=${version}" \
    "$here/io.github.jds300.Wisp.desktop" > "$stage/appimage.desktop"
grep -q "^X-AppImage-Version=${version}$" "$stage/appimage.desktop" || {
    echo "release.sh: could not add X-AppImage-Version to the desktop entry" >&2
    exit 1
}
install -m 0644 "$stage/appimage.desktop" "$appdir/io.github.jds300.Wisp.desktop"
install -m 0644 "$stage/appimage.desktop" \
    "$appdir/usr/share/applications/io.github.jds300.Wisp.desktop"
install -m 0644 "$here/io.github.jds300.Wisp.svg" "$appdir/io.github.jds300.Wisp.svg"
install -m 0644 "$here/io.github.jds300.Wisp.svg" \
    "$appdir/usr/share/icons/hicolor/scalable/apps/io.github.jds300.Wisp.svg"
install -m 0644 "$here/io.github.jds300.Wisp.metainfo.xml" \
    "$appdir/usr/share/metainfo/io.github.jds300.Wisp.metainfo.xml"

cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
# SPDX-License-Identifier: MIT
if [ "$#" -eq 0 ]; then
    exec "$APPDIR/usr/bin/wisp" run
else
    exec "$APPDIR/usr/bin/wisp" "$@"
fi
EOF
chmod 0755 "$appdir/AppRun"

# A CI runner has no FUSE, and locally it is harmless.
export APPIMAGE_EXTRACT_AND_RUN=1
export ARCH=x86_64
# appimagetool writes the .zsync beside its current directory, not beside
# the output path it was given, so this runs with dist/ as its cwd.
(
    cd "$dist"
    "$cache_dir/$appimagetool_file" \
        --runtime-file "$cache_dir/$runtime_file" \
        -u "gh-releases-zsync|JDS300|wisp|${update_channel}|Wisp-*-x86_64.AppImage.zsync" \
        "$appdir" "$(basename "$appimage")" 1>&2
)

(
    cd "$dist"
    sha256sum "$(basename "$tarball")" "$(basename "$appimage")" "$(basename "$zsync")" > "$(basename "$sums")"
)

echo "$tarball"
echo "$appimage"
echo "$zsync"
echo "$sums"
