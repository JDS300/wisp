#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Every refusal in packaging/cut-release.sh, plus the dry run and one real
# cut, against a scratch repository with a bare `origin` of its own. The
# script under test is *copied* into the scratch repo, because it finds the
# repository it operates on from its own location: run in place, it would
# edit this checkout.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

pass=0
fail=0

ok() { echo "ok   $1"; pass=$((pass + 1)); }
bad() { echo "FAIL $1: $2" >&2; fail=$((fail + 1)); }

# Runs the copied script in $repo and reports its exit code and output.
# The capture files are siblings of $repo (like $repo.origin already is),
# never inside it: $repo is the git working tree under test, and a file
# written inside it would sit there untracked, forever failing the "the
# working tree is not clean" check for every later case sharing this repo,
# and inflating the "changes exactly the three bumped files" assertion below.
run_cut() {
    local repo="$1"; shift
    ( cd "$repo" && ./packaging/cut-release.sh "$@" ) >"$repo.out" 2>"$repo.err"
}

# Asserts exit 2 and a stderr line containing $2.
expect_refusal() {
    local name="$1" needle="$2" repo="$3"; shift 3
    set +e
    run_cut "$repo" "$@"
    local code=$?
    set -e
    if [[ "$code" -ne 2 ]]; then
        bad "$name" "exit $code, expected 2; stderr: $(cat "$repo.err")"
        return
    fi
    if ! grep -q "$needle" "$repo.err"; then
        bad "$name" "stderr did not mention '$needle': $(cat "$repo.err")"
        return
    fi
    if [[ "$(wc -l <"$repo.err")" -ne 1 ]]; then
        bad "$name" "a refusal is one line, got: $(cat "$repo.err")"
        return
    fi
    ok "$name"
}

# A fresh scratch repo at $1, at workspace version $2, with a bare origin.
# Removes $repo.origin too, not just $repo: `git init` on an already-existing
# bare repo leaves its refs alone, so a stale origin from an earlier call
# would carry unrelated history and reject this call's first push.
make_repo() {
    local repo="$1" version="$2"
    rm -rf "$repo" "$repo.origin"
    mkdir -p "$repo/crates/hello/src" "$repo/packaging"

    cat >"$repo/Cargo.toml" <<EOF
[workspace]
resolver = "2"
members = ["crates/hello"]

[workspace.package]
version = "$version"
edition = "2021"
EOF
    cat >"$repo/crates/hello/Cargo.toml" <<'EOF'
[package]
name = "hello"
version.workspace = true
edition.workspace = true
EOF
    cat >"$repo/crates/hello/src/lib.rs" <<'EOF'
pub fn hello() -> &'static str { "hello" }

#[test]
fn it_says_hello() { assert_eq!(hello(), "hello"); }
EOF
    cat >"$repo/packaging/io.github.jds300.Wisp.metainfo.xml" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>io.github.jds300.Wisp</id>
  <releases>
    <release version="$version" date="2026-01-01"/>
  </releases>
</component>
EOF
    # Without this, `cargo build`/`cargo test` inside cut-release.sh (step
    # 3) leaves target/ untracked, and every "the tree is clean" or "only
    # the three bumped files changed" assertion below would see it too.
    cat >"$repo/.gitignore" <<'EOF'
/target/
EOF
    install -m 0755 "$root/packaging/cut-release.sh" "$repo/packaging/cut-release.sh"
    install -m 0755 "$root/packaging/version.sh" "$repo/packaging/version.sh"

    ( cd "$repo"
      cargo generate-lockfile --offline >/dev/null 2>&1
      git init --quiet -b main .
      git config user.email "test@example.invalid"
      git config user.name "cut-release test"
      git add -A
      git commit --quiet -m "scratch"
      git init --quiet --bare "$repo.origin"
      git remote add origin "$repo.origin"
      git push --quiet -u origin main
    )
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
repo="$work/repo"

# --- refusals --------------------------------------------------------------

make_repo "$repo" 0.2.0
( cd "$repo" && git checkout --quiet -b not-main )
expect_refusal "refuses a branch that is not main" "not on main" "$repo" 0.3.0
( cd "$repo" && git checkout --quiet main )

echo "dirt" >"$repo/crates/hello/src/dirt.rs"
expect_refusal "refuses a dirty tree" "working tree is not clean" "$repo" 0.3.0
rm "$repo/crates/hello/src/dirt.rs"

( cd "$repo"
  echo "// later" >>crates/hello/src/lib.rs
  git commit --quiet -am "ahead of origin"
)
expect_refusal "refuses a main that is not at origin/main" "not at origin/main" "$repo" 0.3.0
( cd "$repo" && git reset --hard --quiet origin/main )

expect_refusal "refuses a version that is not semver" "not a Cargo semver version" "$repo" 0.3
expect_refusal "refuses a version that is not semver" "not a Cargo semver version" "$repo" "v0.3.0"
expect_refusal "refuses the same version" "not greater than the current 0.2.0" "$repo" 0.2.0
expect_refusal "refuses a lower version" "not greater than the current 0.2.0" "$repo" 0.1.9
expect_refusal "refuses a prerelease of the current version" "not greater than the current 0.2.0" "$repo" "0.2.0-beta.1"

( cd "$repo" && git tag -a v0.3.0 -m "by hand" )
expect_refusal "refuses a tag that exists locally" "already exists locally" "$repo" 0.3.0
( cd "$repo" && git push --quiet origin v0.3.0 && git tag -d v0.3.0 >/dev/null )
expect_refusal "refuses a tag that exists on origin" "already exists on origin" "$repo" 0.3.0

# --- the comparator, through the one interface that uses it ----------------
# (0.3.0-alpha.9 and a bare 0.3.0-beta are no longer reached by the
# comparator at all: the beta-spelling rule below refuses them first.)

make_repo "$repo" "0.3.0-beta.1"
expect_refusal "a beta does not follow itself" "not greater" "$repo" "0.3.0-beta.1"
make_repo "$repo" "0.3.0-beta.10"
expect_refusal "beta.2 does not follow beta.10" "not greater" "$repo" "0.3.0-beta.2"

# --- the beta spelling rule (JDS300, 2026-09-12) ----------------------------
# X.Y.Z-beta.N, N a positive integer, is the only spelling for a non-release
# version. Every other prerelease suffix is refused before the network fetch,
# the "greater than current" check, or anything else that touches the repo.

make_repo "$repo" 0.2.0
for spelling in "0.3.0-rc.1" "0.3.0-alpha" "0.3.0-beta" "0.3.0-beta.0"; do
    expect_refusal "refuses $spelling as a channel spelling" \
        "cut-release.sh: $spelling: a beta is spelled X.Y.Z-beta.N (0.3.0-beta.1); nothing else is a channel" \
        "$repo" "$spelling"
done
# Also covers the two inputs the comparator tests used to exercise directly:
# alpha.9 and a bare beta are prerelease suffixes that are not beta.N either.
expect_refusal "alpha.9 is refused as a channel spelling, not compared" \
    "cut-release.sh: 0.3.0-alpha.9: a beta is spelled X.Y.Z-beta.N (0.3.0-beta.1); nothing else is a channel" \
    "$repo" "0.3.0-alpha.9"
[[ -z "$( cd "$repo" && git tag -l )" ]] \
    && ok "a bad spelling makes no tag" || bad "a bad spelling makes no tag" "$( cd "$repo" && git tag -l )"
[[ "$( cd "$repo" && git rev-parse HEAD )" == "$( cd "$repo" && git rev-parse origin/main )" ]] \
    && ok "a bad spelling makes no commit" || bad "a bad spelling makes no commit" "HEAD moved"

# A beta following the previous beta still passes a dry run.
make_repo "$repo" "0.3.0-beta.1"
set +e
run_cut "$repo" "0.3.0-beta.2" --dry-run
code=$?
set -e
if [[ "$code" -ne 0 ]]; then
    bad "a beta follows the previous beta" "exit $code; stderr: $(cat "$repo.err")"
else
    grep -q "channel beta (latest-pre)" "$repo.err" \
        && ok "a beta follows the previous beta" \
        || bad "a beta follows the previous beta" "$(cat "$repo.err")"
fi

# The accepting direction across the same boundary: a live release follows
# the beta that led up to it.
make_repo "$repo" "0.3.0-beta.1"
set +e
run_cut "$repo" "0.3.0" --dry-run
code=$?
set -e
if [[ "$code" -ne 0 ]]; then
    bad "a release follows its own beta" "exit $code; stderr: $(cat "$repo.err")"
else
    grep -q "channel live (latest)" "$repo.err" \
        && ok "a release follows its own beta" \
        || bad "a release follows its own beta" "$(cat "$repo.err")"
fi

# --- release.sh refuses a bad channel spelling, before building anything ---
# A minimal scratch tree: release.sh only needs to read the workspace
# version and compute the channel before this refusal fires, so nothing
# else it would otherwise touch (zsyncmake, curl, cargo, dist/) needs to
# exist here.

release_scratch="$work/release-scratch"
rm -rf "$release_scratch"
mkdir -p "$release_scratch/packaging"
cat >"$release_scratch/Cargo.toml" <<'EOF'
[workspace]
resolver = "2"
members = []

[workspace.package]
version = "0.3.0-rc.1"
edition = "2021"
EOF
install -m 0755 "$root/packaging/release.sh" "$release_scratch/packaging/release.sh"
install -m 0755 "$root/packaging/version.sh" "$release_scratch/packaging/version.sh"

set +e
( cd "$release_scratch" && ./packaging/release.sh ) >"$release_scratch.out" 2>"$release_scratch.err"
code=$?
set -e
if [[ "$code" -ne 2 ]]; then
    bad "release.sh refuses a bad channel spelling" \
        "exit $code, expected 2; stderr: $(cat "$release_scratch.err")"
elif ! grep -q "release.sh: 0.3.0-rc.1: a beta is spelled X.Y.Z-beta.N (0.3.0-beta.1); nothing else is a channel" \
        "$release_scratch.err"; then
    bad "release.sh refuses a bad channel spelling" "stderr: $(cat "$release_scratch.err")"
else
    ok "release.sh refuses a bad channel spelling"
fi
[[ ! -e "$release_scratch/dist" ]] \
    && ok "release.sh's refusal builds nothing" \
    || bad "release.sh's refusal builds nothing" "dist/ exists"

# --- the dry run -----------------------------------------------------------

make_repo "$repo" 0.2.0
set +e
run_cut "$repo" "0.3.0-beta.1" --dry-run
code=$?
set -e
if [[ "$code" -ne 0 ]]; then
    bad "the dry run succeeds" "exit $code; stderr: $(cat "$repo.err")"
else
    grep -q 'would commit "Release 0.3.0-beta.1"' "$repo.out" \
        && grep -q "would push main and v0.3.0-beta.1" "$repo.out" \
        && ok "the dry run says what it would do" \
        || bad "the dry run says what it would do" "$(cat "$repo.out")"

    changed="$( cd "$repo" && git status --porcelain | awk '{print $2}' | sort | tr '\n' ' ')"
    [[ "$changed" == "Cargo.lock Cargo.toml packaging/io.github.jds300.Wisp.metainfo.xml " ]] \
        && ok "the dry run changes exactly the three bumped files" \
        || bad "the dry run changes exactly the three bumped files" "$changed"

    [[ -z "$( cd "$repo" && git tag -l )" ]] \
        && ok "the dry run makes no tag" \
        || bad "the dry run makes no tag" "$( cd "$repo" && git tag -l )"

    [[ "$( cd "$repo" && git rev-parse HEAD )" == "$( cd "$repo" && git rev-parse origin/main )" ]] \
        && ok "the dry run makes no commit" \
        || bad "the dry run makes no commit" "HEAD moved"

    grep -q '<release version="0.3.0-beta.1" .*type="development"' "$repo/packaging/io.github.jds300.Wisp.metainfo.xml" \
        && ok "a beta is an AppStream development release" \
        || bad "a beta is an AppStream development release" \
           "$(grep '<release' "$repo/packaging/io.github.jds300.Wisp.metainfo.xml")"
fi

# --- one real cut ----------------------------------------------------------

make_repo "$repo" 0.2.0
set +e
run_cut "$repo" "0.3.0"
code=$?
set -e
if [[ "$code" -ne 0 ]]; then
    bad "a real cut succeeds" "exit $code; stderr: $(cat "$repo.err")"
else
    [[ "$( cd "$repo" && git tag -l )" == "v0.3.0" ]] \
        && ok "a real cut tags v0.3.0" || bad "a real cut tags v0.3.0" "$( cd "$repo" && git tag -l )"
    [[ -n "$( cd "$repo" && git ls-remote --tags origin refs/tags/v0.3.0 )" ]] \
        && ok "a real cut pushes the tag" || bad "a real cut pushes the tag" "origin has no v0.3.0"
    [[ "$( cd "$repo" && git log -1 --format=%s )" == "Release 0.3.0" ]] \
        && ok "a real cut commits Release <version>" \
        || bad "a real cut commits Release <version>" "$( cd "$repo" && git log -1 --format=%s )"
    ( cd "$repo" && git log -1 --format=%B ) | grep -q "Channel: live" \
        && ok "the commit body names the channel" || bad "the commit body names the channel" "no channel line"
    ( cd "$repo" && git log -1 --format=%B ) | grep -qi "co-authored-by" \
        && bad "a release commit carries no agent trailer" "it does" \
        || ok "a release commit carries no agent trailer"
    [[ -z "$( cd "$repo" && git status --porcelain )" ]] \
        && ok "a real cut leaves a clean tree" || bad "a real cut leaves a clean tree" "dirty"
    grep -q 'version = "0.3.0"' "$repo/Cargo.toml" \
        && ok "the workspace version is bumped" || bad "the workspace version is bumped" "not bumped"
fi

echo
echo "cut-release tests: $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
