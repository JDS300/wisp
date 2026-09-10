#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"

# Checked before any docker invocation: a missing dist/ would otherwise be
# auto-created by `docker run -v` as root on the bind mount, leaving a
# root-owned directory in the way of the next packaging/release.sh run.
if [[ ! -d "$root/dist" ]]; then
    echo "FAIL sums dist/ is missing; run packaging/release.sh first"
    exit 1
fi

# The whole check runs as one script inside one container and one daemon.
# The heredoc is captured literally (quoted delimiter) so every $variable
# below is expanded by the container's own bash, never by this one.
docker run --rm -v "$root/dist:/dist:ro" ubuntu:24.04 bash -c "$(cat <<'CONTAINER_SCRIPT'
set -uo pipefail
fail=0

pass() { echo "PASS $1"; }
failed() { echo "FAIL $1 $2"; fail=1; }
# Diagnostics belong on stderr: stdout stays PASS/FAIL lines only.
dump() {
    if [ -f "$1" ]; then
        echo "--- $1 ---" >&2
        cat "$1" >&2
    fi
}

if ( cd /dist && sha256sum -c SHA256SUMS --quiet ); then
    pass sums
else
    failed sums "sha256sum -c SHA256SUMS --quiet failed"
fi

tarfile=$(ls /dist/wisp-*-x86_64-linux.tar.gz 2>/dev/null | head -n1)
if [ -z "$tarfile" ]; then
    failed version "no wisp-*-x86_64-linux.tar.gz found in /dist"
    exit "$fail"
fi

workdir=$(mktemp -d)
# Without -e, extraction failing (or leaving nothing behind) has to be
# caught explicitly: an empty $appdir would otherwise put "." first on
# PATH, and every later check would run against whatever happens to be in
# the container's current directory instead of failing cleanly.
if ! tar -xzf "$tarfile" -C "$workdir" 2>/tmp/tar-extract.log; then
    failed version "could not extract the tarball"
    dump /tmp/tar-extract.log
    exit "$fail"
fi
appdir=$(find "$workdir" -mindepth 1 -maxdepth 1 -type d | head -n1)
if [ -z "$appdir" ]; then
    failed version "could not extract the tarball"
    exit "$fail"
fi
export PATH="$appdir:$PATH"
version=$(basename "$appdir")
version=${version#wisp-}

actual=$(wisp --version)
expected="wisp $version"
if [ "$actual" = "$expected" ]; then
    pass version
else
    failed version "expected [$expected] got [$actual]"
fi

wispd --stub >/tmp/wispd.log 2>&1 &
daemon_pid=$!

ready=0
status_out=""
for _ in $(seq 1 50); do
    if status_out=$(wisp status --json 2>/dev/null); then
        ready=1
        break
    fi
    sleep 0.1
done

if [ "$ready" -eq 1 ] && printf '%s' "$status_out" | grep -q "\"v\":3"; then
    pass status
else
    failed status "wisp status --json did not decode as v3: [$status_out]"
    dump /tmp/wispd.log
fi

doctor_out=$(wisp doctor 2>&1)
doctor_code=$?
if [ "$doctor_code" -ne 1 ]; then
    failed doctor "exit=$doctor_code output=[$doctor_out]"
    dump /tmp/wispd.log
elif ! printf '%s' "$doctor_out" | grep -q "/root/.config/wisp/config"; then
    failed doctor "exit=$doctor_code output=[$doctor_out]"
    dump /tmp/wispd.log
elif ! printf '%s' "$doctor_out" | grep -q "a daemon is listening"; then
    failed doctor "doctor did not report the daemon as listening"
    dump /tmp/wispd.log
else
    pass doctor
fi

apt-get update >/tmp/apt-update.log 2>&1
apt-get install -y --no-install-recommends xvfb >/tmp/apt-install.log 2>&1

if kill -0 "$daemon_pid" 2>/dev/null; then
    set +e
    timeout 5 xvfb-run wisp-hud --backend plain >/tmp/wisp-hud.log 2>&1
    hud_code=$?
    set -e
    if [ "$hud_code" -eq 124 ]; then
        pass hud-under-xvfb
    else
        failed hud-under-xvfb "expected exit 124, got $hud_code"
        dump /tmp/apt-install.log
        dump /tmp/wisp-hud.log
    fi
else
    echo "FAIL hud-under-xvfb: the stub daemon from check 2 is gone"
    fail=1
    dump /tmp/apt-install.log
    dump /tmp/wisp-hud.log
fi

kill "$daemon_pid" 2>/dev/null || true
wait "$daemon_pid" 2>/dev/null || true

exit "$fail"
CONTAINER_SCRIPT
)"
