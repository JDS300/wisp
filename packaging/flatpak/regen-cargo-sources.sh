#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

# Pinned external artifact (see docs/plans/2026-09-09-spec-4-packaging.md,
# "Pinned external artifacts"). flatpak-builder-tools has no root LICENSE at
# this commit (checked 2026-09-09: the root holds only .github/, .gitignore,
# CODEOWNERS, README.md and the per-ecosystem directories) — the generator's
# own file declares `__license__ = "MIT"`, which is the license recorded here.
generator_commit="1fc32195e3e60fe5c97f0af646dec7a99df5962b"
generator_url="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/${generator_commit}/cargo/flatpak-cargo-generator.py"
generator_license="MIT (declared as __license__ = \"MIT\" in the file itself; no root LICENSE at this commit)"

cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/wisp-packaging"
mkdir -p "$cache_dir"
generator="$cache_dir/flatpak-cargo-generator.py"

# Fetched into the cache directory, never the repository: the generator is
# not committed, only what it produces is. The URL is content-addressed by
# commit, so a cached copy can never be stale — only ever refetched under a
# different commit, which is a different file name's worth of pinning, not
# a cache-invalidation problem.
if [[ -f "$generator" ]]; then
    echo "regen-cargo-sources.sh: using cached flatpak-cargo-generator.py at commit $generator_commit, license: $generator_license" >&2
else
    curl -fsSL -o "$generator" "$generator_url"
    echo "regen-cargo-sources.sh: fetched flatpak-cargo-generator.py at commit $generator_commit, license: $generator_license" >&2
fi

lockfile="$root/Cargo.lock"
# An optional output path lets build.sh regenerate to a temp file for its
# staleness check without disturbing the committed cargo-sources.json.
output="${1:-$root/packaging/flatpak/cargo-sources.json}"

# The generator's PEP 723 header declares its own dependencies (aiohttp,
# PyYAML, tomlkit), so `uv run` needs nothing else and is tried first; the
# fallback is a throwaway venv for a box without uv.
if command -v uv >/dev/null 2>&1; then
    echo "regen-cargo-sources.sh: using uv" >&2
    uv run "$generator" "$lockfile" -o "$output"
else
    echo "regen-cargo-sources.sh: uv not found, falling back to python3 with a throwaway venv" >&2
    venv="$(mktemp -d)"
    trap 'rm -rf "$venv"' EXIT
    python3 -m venv "$venv"
    "$venv/bin/pip" install --quiet tomlkit aiohttp
    "$venv/bin/python3" "$generator" "$lockfile" -o "$output"
fi

echo "$output"
