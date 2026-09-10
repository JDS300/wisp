#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail

usage() {
    cat <<'EOF'
usage: install.sh [--prefix <dir>] [--uninstall]
       install.sh --help

Installs the three Wisp binaries and its desktop entry, icon and metainfo
under <prefix> (default: $HOME/.local). Re-running install overwrites in
place. --uninstall removes exactly what was installed and nothing else.
EOF
}

prefix="$HOME/.local"
uninstall=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --prefix)
            [[ $# -ge 2 ]] || { echo "install.sh: --prefix needs a value" >&2; exit 2; }
            prefix="$2"
            shift 2
            ;;
        --uninstall)
            uninstall=1
            shift
            ;;
        --help)
            usage
            exit 0
            ;;
        *)
            echo "install.sh: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

# Finds its own files relative to $0, resolved through symlinks, so the
# tarball works from anywhere it was unpacked to and from a symlink to it.
here="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
prefix="${prefix%/}"

bin_dir="$prefix/bin"
apps_dir="$prefix/share/applications"
icons_dir="$prefix/share/icons/hicolor/scalable/apps"
metainfo_dir="$prefix/share/metainfo"

wisp_bin="$bin_dir/wisp"
wispd_bin="$bin_dir/wispd"
hud_bin="$bin_dir/wisp-hud"
desktop_file="$apps_dir/io.github.jds300.Wisp.desktop"
icon_file="$icons_dir/io.github.jds300.Wisp.svg"
metainfo_file="$metainfo_dir/io.github.jds300.Wisp.metainfo.xml"

# Removes `dir` and walks up removing each parent in turn, stopping before
# `stop` (never removed itself) and stopping at the first directory that is
# not there or not empty -- someone else's file anywhere in the chain, or
# $prefix being a shared location such as $HOME/.local, ends the walk right
# there. `rmdir`, never `rm -r`.
remove_empty_up_to() {
    local dir="$1" stop="$2"
    while [[ "$dir" != "$stop" && "$dir" != "/" ]]; do
        rmdir "$dir" 2>/dev/null || break
        dir="$(dirname "$dir")"
    done
}

if [[ "$uninstall" -eq 1 ]]; then
    rm -f "$wisp_bin" "$wispd_bin" "$hud_bin" "$desktop_file" "$icon_file" "$metainfo_file"
    remove_empty_up_to "$bin_dir" "$prefix"
    remove_empty_up_to "$apps_dir" "$prefix"
    remove_empty_up_to "$icons_dir" "$prefix"
    remove_empty_up_to "$metainfo_dir" "$prefix"
    exit 0
fi

install -D -m 0755 "$here/wisp" "$wisp_bin"
install -D -m 0755 "$here/wispd" "$wispd_bin"
install -D -m 0755 "$here/wisp-hud" "$hud_bin"
install -D -m 0644 "$here/io.github.jds300.Wisp.desktop" "$desktop_file"
install -D -m 0644 "$here/io.github.jds300.Wisp.svg" "$icon_file"
install -D -m 0644 "$here/io.github.jds300.Wisp.metainfo.xml" "$metainfo_file"
