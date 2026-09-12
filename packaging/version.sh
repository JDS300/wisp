#!/usr/bin/env bash
# SPDX-License-Identifier: MIT

# wisp_version() prints the version pinned in the root Cargo.toml's
# [workspace.package] section. No jq, no cargo: the release must not depend
# on a working toolchain to know its own name.
wisp_version() {
    local here root
    here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    root="$(cd "$here/.." && pwd)"
    sed -n '/^\[workspace\.package\]/,/^\[/{
        s/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p
    }' "$root/Cargo.toml" | head -n1
}

# wisp_channel <version> prints "live", "beta" or "bad" (JDS300, 2026-09-12).
# A version with no prerelease suffix is live. A prerelease suffix spelled
# exactly `beta.N`, N a positive integer with no leading zero, is a beta.
# Anything else with a prerelease suffix (`rc.1`, `alpha`, `beta`, `beta.0`)
# is "bad": a beta is spelled X.Y.Z-beta.N and nothing else is a channel.
# Shared by cut-release.sh and release.sh so the rule lives in one place.
wisp_channel() {
    local v="$1" pre
    v="${v%%+*}"          # drop Cargo build metadata before looking for '-'
    if [[ "$v" != *-* ]]; then
        printf 'live'
        return 0
    fi
    pre="${v#*-}"
    if [[ "$pre" =~ ^beta\.[1-9][0-9]*$ ]]; then
        printf 'beta'
    else
        printf 'bad'
    fi
}
