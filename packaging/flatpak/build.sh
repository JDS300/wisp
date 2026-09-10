#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
manifest="$here/io.github.jds300.Wisp.yml"
committed_sources="$here/cargo-sources.json"

# An offline build against stale sources fails deep inside cargo with a
# message that does not say what is wrong, so this is the diagnostic: check
# the committed cargo-sources.json still describes Cargo.lock before ever
# starting flatpak-builder.
tmp_sources="$(mktemp)"
trap 'rm -f "$tmp_sources"' EXIT
"$here/regen-cargo-sources.sh" "$tmp_sources" >/dev/null
if ! cmp -s "$tmp_sources" "$committed_sources"; then
    echo "build.sh: packaging/flatpak/cargo-sources.json is stale (Cargo.lock has moved on)." >&2
    echo "build.sh: run packaging/flatpak/regen-cargo-sources.sh and commit the result." >&2
    exit 1
fi

# Under the repo tree, not /tmp: flatpak-builder's state dir must be on the
# same filesystem as the build dir, and /tmp here is tmpfs while the repo is
# a real disk filesystem. --state-dir below points it at a sibling of the
# build dir, not a child of it — --force-clean refuses to run when the state
# dir sits inside the build dir it is about to clean ("Refusing to delete
# current working directory, state directory or their parents"). Siblings
# under the same root keep the one-filesystem guarantee regardless of what
# directory build.sh is invoked from. Both removed by the trap; never
# committed.
build_dir="$(mktemp -d "$root/.flatpak-build-XXXXXX")"
state_dir="$root/.flatpak-builder"
rm -rf "$state_dir"
trap 'rm -f "$tmp_sources"; rm -rf "$build_dir" "$state_dir"' EXIT

# The flathub remote may only be configured at system scope (flatpak
# remotes --user prints nothing on a box where only `flatpak remote-add
# --system flathub ...` was ever run) — everything below is --user, so a
# --user remote of the same name is needed too. --if-not-exists makes this
# safe to run every time.
if ! flatpak remote-list --user | grep -q '^flathub'; then
    flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
fi

# org.flatpak.Builder from Flathub, not a native flatpak-builder: the
# development box has none. `flatpak run` does not install the app it is
# asked to run, only the runtime dependencies --install-deps-from names, so
# the Builder itself is installed here first if it is missing (expect
# minutes on the first run of either step, for the Builder and then for the
# Platform, the Sdk and rust-stable, all absent today).
if ! flatpak info --user org.flatpak.Builder >/dev/null 2>&1; then
    flatpak install --user --noninteractive flathub org.flatpak.Builder
fi

flatpak run org.flatpak.Builder \
    --user --install --force-clean --install-deps-from=flathub \
    --state-dir="$state_dir" \
    "$build_dir" "$manifest"

if [[ "${1:-}" == "--bundle" ]]; then
    # shellcheck source=../version.sh
    source "$root/packaging/version.sh"
    version="$(wisp_version)"
    if [[ -z "$version" ]]; then
        echo "build.sh: could not read the workspace version" >&2
        exit 1
    fi
    dist="$root/dist"
    mkdir -p "$dist"
    bundle="$dist/Wisp-${version}.flatpak"
    # --user's own repo, the same one --install just wrote into.
    flatpak build-bundle "$HOME/.local/share/flatpak/repo" "$bundle" io.github.jds300.Wisp
    echo "$bundle"
fi
