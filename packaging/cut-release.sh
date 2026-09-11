#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The one way to make a Wisp release (Spec 6 §3.2, §4.4). A release is a tag
# `v<version>` on `main` where <version> is exactly the workspace version.
# No other path publishes: a hand-made tag, a tag on a branch, or a build
# copied into ~/AppImages by hand are all ways of shipping something Gear
# Lever will overwrite. See docs/RELEASING.md.
#
# Commits made here carry no Co-Authored-By trailer: they are the release
# manager's own commits, not an agent's.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
# shellcheck source=version.sh
source "$here/version.sh"

metainfo="$root/packaging/io.github.jds300.Wisp.metainfo.xml"
workflow_url="https://github.com/JDS300/wisp/actions/workflows/release.yml"

usage() {
    cat <<'EOF'
usage: cut-release.sh <version> [--dry-run]
       cut-release.sh --help

Bumps the workspace version, the lockfile and the metainfo, builds and tests
the edited tree, then commits `Release <version>`, tags `v<version>` and
pushes both to origin. A version with a prerelease suffix (0.3.0-beta.1) is
a beta and is published as a GitHub pre-release; without one it is live.

Exit 2 is a refusal (one line saying which rule), 1 is a check that failed.
EOF
}

refuse() {
    echo "cut-release.sh: $1" >&2
    exit 2
}

version=""
dry_run=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) dry_run=1; shift ;;
        --help) usage; exit 0 ;;
        -*) echo "cut-release.sh: unknown argument: $1" >&2; usage >&2; exit 2 ;;
        *)
            if [[ -n "$version" ]]; then
                echo "cut-release.sh: more than one version given" >&2
                usage >&2
                exit 2
            fi
            version="$1"
            shift
            ;;
    esac
done
if [[ -z "$version" ]]; then
    usage >&2
    exit 2
fi

# --- the semver comparator -------------------------------------------------
# `sort -V` is not one: it puts 0.3.0-beta.1 *after* 0.3.0, and semver says a
# prerelease is lower than the release it precedes. Measured on the
# development box, 2026-09-11.

# The prerelease part of a version, or "".
pre_of() {
    local v="$1"
    v="${v%%+*}"
    if [[ "$v" == *-* ]]; then
        printf '%s' "${v#*-}"
    fi
}

# The `major.minor.patch` part.
core_of() {
    local v="$1"
    v="${v%%+*}"
    printf '%s' "${v%%-*}"
}

# 0 (true) when prerelease $1 sorts above prerelease $2. Both non-empty.
pre_gt() {
    local -a a b
    IFS=. read -r -a a <<<"$1"
    IFS=. read -r -a b <<<"$2"
    local n=${#a[@]}
    if (( ${#b[@]} > n )); then n=${#b[@]}; fi
    local i x y
    for (( i = 0; i < n; i++ )); do
        x="${a[i]-}"
        y="${b[i]-}"
        # A field the other has and this one does not: fewer fields is lower.
        if [[ -z "$x" ]]; then return 1; fi
        if [[ -z "$y" ]]; then return 0; fi
        if [[ "$x" == "$y" ]]; then continue; fi
        if [[ "$x" =~ ^[0-9]+$ && "$y" =~ ^[0-9]+$ ]]; then
            if (( 10#$x > 10#$y )); then return 0; else return 1; fi
        fi
        # Numeric identifiers always have lower precedence than alphanumeric.
        if [[ "$x" =~ ^[0-9]+$ ]]; then return 1; fi
        if [[ "$y" =~ ^[0-9]+$ ]]; then return 0; fi
        if [[ "$x" > "$y" ]]; then return 0; else return 1; fi
    done
    return 1
}

# 0 (true) when version $1 sorts strictly above version $2.
semver_gt() {
    local -a a b
    IFS=. read -r -a a <<<"$(core_of "$1")"
    IFS=. read -r -a b <<<"$(core_of "$2")"
    local i
    for i in 0 1 2; do
        if (( 10#${a[i]:-0} > 10#${b[i]:-0} )); then return 0; fi
        if (( 10#${a[i]:-0} < 10#${b[i]:-0} )); then return 1; fi
    done
    local a_pre b_pre
    a_pre="$(pre_of "$1")"
    b_pre="$(pre_of "$2")"
    if [[ -z "$a_pre" && -z "$b_pre" ]]; then return 1; fi   # identical
    if [[ -z "$a_pre" ]]; then return 0; fi                  # release > its own prereleases
    if [[ -z "$b_pre" ]]; then return 1; fi
    pre_gt "$a_pre" "$b_pre"
}

# --- step 1: the refusals --------------------------------------------------

cd "$root"

branch="$(git rev-parse --abbrev-ref HEAD)"
[[ "$branch" == "main" ]] || refuse "not on main (on $branch)"

[[ -z "$(git status --porcelain)" ]] || refuse "the working tree is not clean"

# Checked before the network fetch below: a typo'd version should get its
# own refusal even offline, not "main is not at origin/main" from a fetch
# that never needed to run.
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]] \
    || refuse "$version is not a Cargo semver version"

# No --tags here: fetching every remote tag first would re-import a tag
# straight back into refs/tags/ whenever it already sits on origin, so the
# "already exists locally" check below would always fire first and the
# "already exists on origin" one could never be reached. The two checks that
# follow don't need it either -- rev-parse reads local refs, ls-remote asks
# origin directly. Verified 2026-09-11: with --tags, deleting a local tag
# that was already pushed does not stop the next run from re-fetching it.
git fetch --quiet origin main
local_head="$(git rev-parse HEAD)"
origin_head="$(git rev-parse origin/main)"
[[ "$local_head" == "$origin_head" ]] || refuse "main is not at origin/main; pull or push first"

current="$(wisp_version)"
[[ -n "$current" ]] || refuse "could not read the workspace version"
semver_gt "$version" "$current" || refuse "$version is not greater than the current $current"

tag="v$version"
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
    refuse "the tag $tag already exists locally"
fi
if [[ -n "$(git ls-remote --tags origin "refs/tags/$tag")" ]]; then
    refuse "the tag $tag already exists on origin"
fi

if [[ "$version" == *-* ]]; then
    channel="beta"
    update_channel="latest-pre"
    release_type=' type="development"'
else
    channel="live"
    update_channel="latest"
    release_type=''
fi

# --- step 2: the three edits, each read back -------------------------------
# Every edit is a sed and every sed is checked by re-reading the file. The
# same round-trip discipline Spec 5's config writer uses: an edit that did
# not land must stop the release, not produce a tag for a version nobody
# bumped.

sed -i '/^\[workspace\.package\]/,/^\[/{s/^version[[:space:]]*=[[:space:]]*"[^"]*"/version = "'"$version"'"/}' Cargo.toml
[[ "$(wisp_version)" == "$version" ]] \
    || { echo "cut-release.sh: Cargo.toml did not read back as $version" >&2; exit 1; }

cargo update --workspace --offline >/dev/null
grep -q "^version = \"$version\"\$" Cargo.lock \
    || { echo "cut-release.sh: Cargo.lock does not name $version" >&2; exit 1; }

today="$(date +%Y-%m-%d)"
sed -i "0,/<releases>/s@<releases>@<releases>\n    <release version=\"$version\" date=\"$today\"$release_type/>@" "$metainfo"
grep -q "<release version=\"$version\" date=\"$today\"" "$metainfo" \
    || { echo "cut-release.sh: the metainfo did not read back with $version" >&2; exit 1; }

echo "cut-release.sh: $current -> $version, channel $channel ($update_channel)" >&2

# --- step 3: build and test the edited tree --------------------------------
# Before anything is committed. A tag on a commit whose tests fail is a
# release nobody can withdraw.

runner=()
if command -v xvfb-run >/dev/null 2>&1; then
    runner=(xvfb-run -a)
fi
if ! cargo build --workspace --locked; then
    echo "cut-release.sh: the build failed; the version edits are in the tree, uncommitted" >&2
    exit 1
fi
if ! ${runner[@]+"${runner[@]}"} cargo test --workspace --locked; then
    echo "cut-release.sh: the tests failed; the version edits are in the tree, uncommitted" >&2
    exit 1
fi

if [[ "$dry_run" -eq 1 ]]; then
    echo "cut-release.sh: dry run: would commit \"Release $version\" and tag $tag"
    echo "cut-release.sh: dry run: would push main and $tag to origin"
    echo "cut-release.sh: the version edits are in the working tree; undo them with" >&2
    echo "  git checkout -- Cargo.toml Cargo.lock packaging/io.github.jds300.Wisp.metainfo.xml" >&2
    exit 0
fi

# --- step 4: commit, tag, push ---------------------------------------------

git add Cargo.toml Cargo.lock packaging/io.github.jds300.Wisp.metainfo.xml
message="Release $version

Channel: $channel (the AppImage's update source is $update_channel)."
git commit --quiet -m "$message"
git tag -a "$tag" -m "$message"
git push --quiet origin main "$tag"

# --- step 5: what to watch -------------------------------------------------

echo "cut-release.sh: pushed $tag; watch $workflow_url"
if [[ "$channel" == "live" ]]; then
    echo "cut-release.sh: once the run is green, Gear Lever offers $version to everyone."
else
    echo "cut-release.sh: once the run is green, only installs whose .upd_info reads"
    echo "  latest-pre are offered $version. Check with: readelf -p .upd_info <AppImage>"
fi
