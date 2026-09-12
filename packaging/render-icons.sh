#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Cuts every raster Wisp ships out of the one design sheet, at fixed pixel
# boxes measured on 2026-09-12 (Spec 7 §4.1). Run by hand when the sheet
# changes; the outputs are committed. No build script, no rasteriser in the
# dependency tree: CI has neither ImageMagick nor this script and needs
# neither.
#
# Twenty-nine files: one README banner, eight launcher PNGs, twenty tray
# pixmaps. The tray pixmaps are ARGB32 in network (big-endian) byte order,
# not premultiplied -- what the StatusNotifierItem specification's IconPixmap
# asks for and what ksni::Icon::data is documented to hold. ImageMagick 7 has
# no raw `argb:` format (it writes zero bytes and exits 0 for one), so the
# RGBA bytes are reordered here and the byte count is checked on both sides.
#
# Every `magick` that writes a PNG passes -strip: without it ImageMagick
# stamps a tIME chunk and three date:* tEXt chunks into the file and two runs
# a second apart differ, which would make "re-running changes nothing" false.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"

sheet="$root/docs/art/wisp-sheet.jpeg"
banner="$root/docs/art/banner.png"
png_root="$root/packaging/icons/hicolor"
argb_dir="$root/crates/wisp-hud/icons"

# The sheet's own size. A re-laid-out sheet must fail here, loudly, rather
# than produce twenty-nine plausible-looking crops of the wrong thing.
sheet_dims="2816x1536"

# Nothing is written until every refusal below has passed.
for tool in magick python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "render-icons.sh: $tool not found; install imagemagick and python3" >&2
        exit 2
    fi
done

if [[ ! -f "$sheet" ]]; then
    echo "render-icons.sh: $sheet is missing" >&2
    exit 2
fi

dims="$(magick identify -format '%wx%h' "$sheet")"
if [[ "$dims" != "$sheet_dims" ]]; then
    echo "render-icons.sh: the sheet is $dims, expected $sheet_dims" >&2
    exit 2
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

wrote=0

# Idempotent: an unchanged sheet rewrites nothing, so a re-run leaves the
# working tree clean and `git status --short` stays honest.
place() {
    local src="$1" target="$2"
    if [[ -f "$target" ]] && cmp -s "$src" "$target"; then
        echo "render-icons.sh: $(basename "$target") unchanged" >&2
    else
        install -D -m 0644 "$src" "$target"
        echo "render-icons.sh: wrote $target ($(stat -c%s "$target") bytes)" >&2
        wrote=$(( wrote + 1 ))
    fi
}

assert_size() {
    local file="$1" want="$2" got
    got="$(magick identify -format '%wx%h' "$file")"
    if [[ "$got" != "$want" ]]; then
        echo "render-icons.sh: $file is $got, expected $want" >&2
        exit 2
    fi
}

# --- the README banner ------------------------------------------------------
magick "$sheet" -crop 1570x843+47+99 +repage -filter Lanczos -resize 1280 -strip "$tmp/banner.png"
assert_size "$tmp/banner.png" "1280x687"
place "$tmp/banner.png" "$banner"

# --- the launcher, eight PNGs ----------------------------------------------
# The crop is already tight on the rounded square, so -trim removes nothing
# and 520x520 is the expected post-trim size, not a failure. The tile is
# opaque by design (Spec 7 §2): its own background is kept, not made
# transparent.
magick "$sheet" -crop 520x520+1756+200 +repage -fuzz 8% -trim +repage "$tmp/launcher.png"
assert_size "$tmp/launcher.png" "520x520"
for n in 512 256 128 64 48 32 24 16; do
    magick "$tmp/launcher.png" -filter Lanczos -resize "${n}x${n}!" -strip "$tmp/launcher-$n.png"
    assert_size "$tmp/launcher-$n.png" "${n}x${n}"
    place "$tmp/launcher-$n.png" "$png_root/${n}x${n}/apps/io.github.jds300.Wisp.png"
done

# --- the tray, four states x five sizes ------------------------------------
# state : crop box : the post-trim size measured on 2026-09-12. The tiles are
# not square and not equal to each other; each is asserted on its own so a
# re-laid-out sheet is caught here rather than shipped.
# 2026-09-12 review: the two green crops were swapped here (not the words).
# The sheet's glowing tile ("Active") is brighter than its flatter sibling
# ("Active (Variant)"), so it now feeds Fighting -- the state the spec and
# README call "bright green" -- and the flatter tile feeds Tailing, "green".
tiles=(
    "tailing:340x350+770+1105:331x335"
    "fighting:340x350+80+1105:336x337"
    "error:340x350+1470+1105:333x333"
    "waiting:340x350+2170+1105:334x333"
)

for tile in "${tiles[@]}"; do
    IFS=: read -r state box trimmed <<<"$tile"
    magick "$sheet" -crop "$box" +repage -fuzz 8% -trim +repage "$tmp/$state.png"
    assert_size "$tmp/$state.png" "$trimmed"

    for n in 48 32 24 22 16; do
        expected=$(( n * n * 4 ))
        rgba="$tmp/$state-$n.rgba"
        argb="$tmp/$state-$n.argb"

        # -resize NxN! forces the square the tray asks for. The tiles are
        # within 1.2% of square already, so the distortion is invisible and
        # the byte count is guaranteed.
        magick "$tmp/$state.png" -filter Lanczos -resize "${n}x${n}!" -alpha on -depth 8 "rgba:$rgba"

        actual="$(stat -c%s "$rgba")"
        if [[ "$actual" != "$expected" ]]; then
            echo "render-icons.sh: $state ${n}px gave $actual bytes, expected $expected" >&2
            exit 2
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
            echo "render-icons.sh: the ARGB conversion changed the byte count" >&2
            exit 2
        fi

        place "$argb" "$argb_dir/tray-$state-$n.argb"
    done
done

echo "render-icons.sh: $wrote file(s) written" >&2
