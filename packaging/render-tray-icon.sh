#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Renders packaging/io.github.jds300.Wisp.svg into the two raw ARGB32 pixmaps
# the tray embeds with include_bytes!. Run by hand when the SVG changes; the
# output is committed. No build script and no rasteriser in the dependency
# tree: CI has neither tool and does not need them.
#
# ARGB32, network (big-endian) byte order, not premultiplied — what the
# StatusNotifierItem specification's IconPixmap asks for, and what
# ksni::Icon::data is documented to hold. ImageMagick has no raw `argb:`
# format (it writes zero bytes and exits 0 for one), so the RGBA bytes are
# reordered here.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"

svg="$root/packaging/io.github.jds300.Wisp.svg"
out_dir="$root/crates/wisp-hud/icons"

for tool in rsvg-convert magick python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "render-tray-icon.sh: $tool not found; install librsvg, imagemagick and python3" >&2
        exit 1
    fi
done

if [[ ! -f "$svg" ]]; then
    echo "render-tray-icon.sh: $svg is missing" >&2
    exit 1
fi

mkdir -p "$out_dir"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

for size in 22 48; do
    png="$tmp/$size.png"
    rgba="$tmp/$size.rgba"
    argb="$tmp/$size.argb"
    target="$out_dir/tray-$size.argb"

    rsvg-convert -w "$size" -h "$size" -f png "$svg" -o "$png"
    magick "$png" -depth 8 rgba:- > "$rgba"

    expected=$(( size * size * 4 ))
    actual=$(stat -c%s "$rgba")
    if [[ "$actual" != "$expected" ]]; then
        echo "render-tray-icon.sh: ${size}px gave $actual bytes, expected $expected" >&2
        exit 1
    fi

    python3 - "$rgba" "$argb" <<'PY'
import sys

src, dst = sys.argv[1], sys.argv[2]
data = open(src, "rb").read()
out = bytearray(len(data))
# R G B A  ->  A R G B, one pixel at a time.
for i in range(0, len(data), 4):
    r, g, b, a = data[i], data[i + 1], data[i + 2], data[i + 3]
    out[i], out[i + 1], out[i + 2], out[i + 3] = a, r, g, b
open(dst, "wb").write(bytes(out))
PY

    if [[ "$(stat -c%s "$argb")" != "$expected" ]]; then
        echo "render-tray-icon.sh: the ARGB conversion changed the byte count" >&2
        exit 1
    fi

    # Idempotent: an unchanged SVG rewrites nothing, so a re-run leaves the
    # working tree clean and `git status` stays honest.
    if [[ -f "$target" ]] && cmp -s "$argb" "$target"; then
        echo "render-tray-icon.sh: tray-$size.argb unchanged" >&2
    else
        install -m 0644 "$argb" "$target"
        echo "render-tray-icon.sh: wrote $target ($expected bytes)" >&2
    fi
done
