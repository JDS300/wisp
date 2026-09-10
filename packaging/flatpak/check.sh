#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The Milestone 5 checks (docs/specs/2026-09-09-spec-4-packaging.md), against
# the installed io.github.jds300.Wisp Flatpak. Run packaging/flatpak/build.sh
# first. One PASS/FAIL line per check on stdout; diagnostics go to stderr.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
app_id="io.github.jds300.Wisp"

fail=0
pass() { echo "PASS $1"; }
failed() { echo "FAIL $1 $2"; fail=1; }

log_dir="$(mktemp -d)"
daemon_pid=""
cleanup() {
    # Killing the backgrounded `flatpak run` wrapper (instance A) ends its
    # sandbox — `flatpak kill "$app_id"` is not used here because it would
    # terminate every running instance of the app for this user, including
    # one a developer may have running over their own game; only the
    # instance this script started is addressed here.
    if [[ -n "$daemon_pid" ]]; then
        kill "$daemon_pid" >/dev/null 2>&1 || true
        wait "$daemon_pid" 2>/dev/null || true
    fi
    rm -rf "$log_dir"
}
trap cleanup EXIT

# 1: flatpak run io.github.jds300.Wisp --version
version_out="$(flatpak run "$app_id" --version 2>"$log_dir/version.log")"
version_code=$?
if [[ $version_code -eq 0 ]] && [[ "$version_out" == wisp\ * ]]; then
    pass version
else
    failed version "exit=$version_code output=[$version_out]"
    cat "$log_dir/version.log" >&2
fi

# 2: instance A — flatpak run --command=wispd io.github.jds300.Wisp --stub &
flatpak run --command=wispd "$app_id" --stub >"$log_dir/wispd.log" 2>&1 &
daemon_pid=$!
sleep 0.5
if kill -0 "$daemon_pid" 2>/dev/null; then
    pass instance-a-starts
else
    failed instance-a-starts "the backgrounded flatpak run exited immediately"
    cat "$log_dir/wispd.log" >&2
fi

# 3: instance B — flatpak run --command=wisp io.github.jds300.Wisp status --json
# The socket-sharing rule: this only works if both instances resolve
# $XDG_RUNTIME_DIR/app/io.github.jds300.Wisp/wispd.sock. If this fails, it is
# the first thing to report — nothing else in this task matters until it does.
status_out=""
status_ready=0
for _ in $(seq 1 50); do
    if status_out="$(flatpak run --command=wisp "$app_id" status --json 2>"$log_dir/status.log")"; then
        status_ready=1
        break
    fi
    sleep 0.1
done
if [[ $status_ready -eq 1 ]] && [[ "$status_out" == *'"v":3'* ]]; then
    pass status-sees-daemon
else
    failed status-sees-daemon "instance B did not see instance A's daemon: [$status_out]"
    cat "$log_dir/status.log" >&2
fi

# 4: flatpak run --command=sh io.github.jds300.Wisp -c 'echo $DISPLAY; ls /tmp/.X11-unix'
display_out="$(flatpak run --command=sh "$app_id" -c 'echo "$DISPLAY"; ls /tmp/.X11-unix' 2>"$log_dir/display.log")"
display_code=$?
echo "display check output:" >&2
echo "$display_out" >&2
sandbox_display="$(printf '%s\n' "$display_out" | sed -n '1p')"
sandbox_sockets="$(printf '%s\n' "$display_out" | tail -n +2)"
socket_count="$(printf '%s\n' "$sandbox_sockets" | sed '/^$/d' | wc -l)"
if [[ $display_code -ne 0 ]]; then
    failed display "exit=$display_code"
    cat "$log_dir/display.log" >&2
elif [[ -n "$sandbox_display" ]]; then
    # DISPLAY was set at launch: the fresh sandbox directory must hold only
    # that display's socket — never more than one. Strip the leading ":"
    # and, for a screen-qualified display like ":0.0", everything from the
    # first "." on: the socket is still named for the display alone.
    expected_socket="${sandbox_display#:}"
    expected_socket="${expected_socket%%.*}"
    expected_socket="X${expected_socket}"
    if [[ "$socket_count" -eq 1 ]] && [[ "$sandbox_sockets" == "$expected_socket" ]]; then
        pass display
    else
        failed display "DISPLAY=$sandbox_display but /tmp/.X11-unix held [$sandbox_sockets]"
    fi
else
    # No DISPLAY at launch: /tmp/.X11-unix is expected to be empty.
    if [[ "$socket_count" -eq 0 ]]; then
        pass display
    else
        failed display "DISPLAY was empty but /tmp/.X11-unix held [$sandbox_sockets]"
    fi
fi

# 5: appstreamcli validate --no-net packaging/io.github.jds300.Wisp.metainfo.xml
if appstream_out="$(appstreamcli validate --no-net "$root/packaging/io.github.jds300.Wisp.metainfo.xml" 2>&1)"; then
    pass appstream
else
    failed appstream "appstreamcli validate failed"
    echo "$appstream_out" >&2
fi

# 6: desktop-file-validate packaging/io.github.jds300.Wisp.desktop
if desktop_out="$(desktop-file-validate "$root/packaging/io.github.jds300.Wisp.desktop" 2>&1)"; then
    pass desktop-file
else
    failed desktop-file "desktop-file-validate failed"
    echo "$desktop_out" >&2
fi

exit "$fail"
