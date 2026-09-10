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
