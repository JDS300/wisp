# Spec 6 — running it: `wisp stop`, the tray, release channels

**Status:** designed 2026-09-11; implemented 2026-09-11; pending JDS300: Milestone 6 (tray live on Plasma), Milestone 8 (HUD mode over the game), Milestone 9 (the gamescope grab spike) and Milestone 10 (live, the delivery path)
**Depends on:** [Spec 5 — the HUD](2026-09-10-spec-5-the-hud.md), [Spec 4 — packaging](2026-09-09-spec-4-packaging.md), [Spec 1 — the spine](2026-09-08-spec-1-the-spine.md), [Spec 0 — clean-room charter](2026-09-08-clean-room-charter.md)
**Target client:** EverQuest Legends. Not Live, not Project Quarm.

---

## 1. What this spec is for

Spec 5 shipped as v0.2.0 on 2026-09-11, and shipping it showed three gaps that
are about running Wisp rather than what it draws.

First, there is no way to stop it. `wisp run` starts a daemon and a HUD and
follows them; the desktop entry launches it with no terminal; and once it is
up the only way down is `pkill -x wisp-hud`, which JDS300 should not have to
know. There is no tray icon either, so a running Wisp is invisible except for
the overlay itself.

Second, the delivery path was discovered rather than designed. Gear Lever
updates an AppImage from the update source embedded in it, which points at the
repository's latest GitHub release. A build that is not a release therefore
never reaches JDS300, and a hand-installed CI artifact is overwritten the next
time Gear Lever updates — which happened once, back to v0.1.0, on the day
Spec 5 was meant to be tested. The fix that day was to release; this spec
makes that the rule, with a beta channel so a release can be flagged as
something to try before it is something everyone gets.

Third, the version was invisible: Gear Lever reads `X-AppImage-Version` from
the desktop entry inside the AppImage, and Spec 4's did not write one. That
was fixed on the Spec 5 branch (2026-09-11) and is recorded here so the
release process owns it.

Fourth, found the same day in Milestone 4 of Spec 5: in HUD mode the arrow
keys move the selected block and also move the character. Spec 5 §4.7 says
why — the HUD polls key state and consumes nothing, so the game keeps every
key — and chose that because the gamescope spike showed an overlay that took
focus could not give it back. On the layer-shell backend the compositor
manages focus, and the protocol has a switch for exactly this. On gamescope
it stays an open question with one candidate to try.

So: `wisp stop`, a StatusNotifierItem tray icon with four entries, HUD mode
that owns the keyboard where the compositor lets it, and a release path with
two channels enforced by one script and described by one document.

---

## 2. Non-goals

- **A settings window.** The tray opens the config file in the user's editor;
  it does not edit it.
- **Notifications.** No desktop notifications for fights, timers or updates.
- **Update checks from inside Wisp.** Gear Lever, or whatever installed the
  AppImage, owns updates. Wisp never calls GitHub.
- **A tray on gamescope in game mode.** There is no StatusNotifierWatcher
  there. The tray registers when there is one and stays silent when there is
  not. Steam Deck desktop mode has one and gets the tray for free.
- **Flathub.** The Flatpak gains the one D-Bus permission the tray needs and
  nothing else about the Flatpak changes; submission is still deferred.
- **Signals.** The `wisp` launcher keeps its Spec 4 stance: no signal
  handling, no `libc`, no signal crate. Stopping is a request over the socket
  the daemon already has.
- **Analytics.** Spec 7.

---

## 3. Invariants

### 3.1 Unchanged

- Spec 1 §3: the parser is right or it is silent; the daemon never touches the
  log; one socket under `$XDG_RUNTIME_DIR/wisp/`.
- Spec 4: three static musl binaries in the tarball and the AppImage;
  `wisp run -- %command%` as a launch option; the launcher owns both children
  and stops the other when one exits.
- Spec 5 §3.3: nothing talks to the HUD over a socket. The tray lives inside
  the HUD, so the mode toggle is a function call, not a message.

### 3.2 New

- **The daemon accepts one request kind, `stop`, and nothing else.** A client
  that writes nothing is what every client before this spec was; a client that
  writes anything but the stop line is ignored, not disconnected, and not
  answered. The protocol stays newline-delimited JSON one way and one word the
  other.
- **`wisp stop` is idempotent.** Nothing listening is a clean exit, said once.
- **A release is a tag on `main` that equals the workspace version, cut by
  `packaging/cut-release.sh`.** No other path publishes. A version with a
  prerelease suffix is a beta; without one it is live. The AppImage's update
  source follows the channel of the build that carries it.
- **Spec 5 §3.1 amended: the HUD never changes focus outside HUD mode.**
  Inside HUD mode, on the layer-shell backend only, it asks the compositor
  for exclusive keyboard interactivity and releases it on exit; the compositor
  moves focus and moves it back. On every X11 backend the Spec 5 rule stands
  unchanged: no focus, empty input region, `GAMESCOPE_NO_FOCUS` set. §4.5.
- **The tray never blocks the HUD.** Registration, menu events and D-Bus
  traffic run on their own thread; the render loop reads a flag and a
  channel, and a missing or slow bus costs the HUD nothing.

---

## 4. Architecture

### 4.1 `wisp stop` — the request

`wispd`'s socket is one way today: the daemon writes a snapshot line to every
client on connect and on change, and clients never write. `Server::accept_pending`
sets each accepted stream non-blocking and keeps it for `broadcast`.

This spec adds a read. On every tick, alongside `accept_pending`, the server
polls each client for input with a non-blocking read into a small per-client
buffer (256 bytes; anything past that is discarded and the client is left
connected). When a buffer holds a complete line equal to `stop`, the server
returns `Request::Stop` to the main loop, which logs `wispd: stop requested`,
drops the listener (removing the socket file the way a clean exit already
does) and exits 0. The launcher notices the daemon's exit through its
existing poll, stops the HUD, and exits 0 — `wisp_status` already maps a
clean exit to a clean stop. With `wisp run -- <command>`, the game is not
touched: `follow_command` keeps waiting on it, as it does today when the
daemon exits for any other reason.

The line is the bare word `stop` followed by `\n`, not JSON: it is the one
request there is, it must be typeable into `socat`, and a JSON object would
suggest a request vocabulary this spec is not opening. The request does not
bump `PROTOCOL_VERSION` on its own — that number describes the snapshot, and
§4.3 bumps it for the snapshot's new field — but it is recorded in
`wisp-proto`'s docs and as `wisp_proto::STOP_LINE`, the one constant both
`wispd` and its two clients share.

`wisp stop`:

```
wisp stop
```

Connects to the socket path from `wisp_config::paths::socket_path()`, writes
`STOP_LINE`, and waits up to 5 s for the daemon to close the connection
(`read` returning 0), which is the acknowledgement. Then it exits 0 and
prints nothing. No daemon listening — the same `ENOENT`/`ECONNREFUSED` test
`wisp status` makes — prints `wisp: nothing to stop: no daemon is listening on
<path>` and exits 0. A daemon that accepted the line and did not close within
5 s prints `wisp: the daemon did not stop within 5 s` and exits 1; this spec
does not escalate to a kill, because the launcher's `pkill` path still exists
and a daemon that ignores its own stop line is a bug to see, not hide.

Ordering matters for a stale socket file: `Server::bind` already refuses a
path something is listening on and replaces one nothing is. `wisp stop`
connecting to a dead socket file gets `ECONNREFUSED` and reports nothing to
stop; it does not remove the file, which is the next `wispd`'s job.

### 4.2 The tray — a StatusNotifierItem in `wisp-hud`

The tray is a StatusNotifierItem (SNI) registered over the session D-Bus with
`org.kde.StatusNotifierWatcher`, the interface Plasma, GNOME with the
AppIndicator extension, and every wlroots bar speak. The crate is `ksni`
0.3.6 (pure Rust over `zbus` 5), built with `default-features = false`
and `features = ["blocking", "async-io"]` so no `tokio` enters the static
musl build. `zbus` needs no C library. ksni's MSRV is 1.80; CI is pinned to
1.94.1.

**Licence correction (Task 8).** This section was designed believing `ksni`
was MIT. `cargo metadata` says otherwise: `ksni` 0.3.6's own `[package]
license` field is `Unlicense`, a public-domain dedication. `THIRD_PARTY.md`
records the crate's actual licence and every other crate the tray tree
added; this paragraph is corrected rather than rewritten so the record shows
what was believed at design time and what the tooling reports.

**Why the HUD and not the launcher.** Every entry on the menu is something the
HUD already has: the latest snapshot for the status line, `HudMode.active` to
toggle, the config path to open, and the socket path to write `stop` to. The
`wisp` crate has no dependencies by design and stays that way. A fourth
binary would need a channel to the HUD for the mode toggle and one more
process to package, spawn and reap, for four menu entries.

**Lifecycle.** After the backend is up and before the first frame, the HUD
spawns the tray thread with `ksni::TrayService::spawn`. If registration fails
— no watcher on the bus, no session bus at all (gamescope game mode, `Xvfb`
in CI) — the HUD prints one line, `wisp-hud: no tray: <reason>`, and carries
on.

**API correction (Task 9).** This section was designed believing `ksni`
exposed a `TrayService` type with a `spawn` method. `ksni` 0.3.6 has no such
type: the real entry point is `ksni::blocking::TrayMethods::spawn`, the trait
`crates/wisp-hud/src/tray.rs` actually imports and calls; this paragraph is
corrected rather than rewritten so the record shows what was believed at
design time and what the crate provides. The thread owns the D-Bus connection; the render loop and the tray share
an `Arc<Mutex<TrayState>>` and a `std::sync::mpsc` channel of `TrayEvent`s
the render loop drains once per frame, right where it already handles keys.

```rust
pub struct TrayState {            // written by the HUD, read by the menu
    pub version: &'static str,     // env!("CARGO_PKG_VERSION")
    pub log_name: Option<String>,  // from the snapshot, §4.3
    pub fight: Option<u64>,        // duration_s of an active encounter
    pub hud_mode: bool,
}
pub enum TrayEvent { ToggleHudMode, OpenConfig, Stop }
```

**The menu**, top to bottom:

| Entry | Kind | Effect |
|---|---|---|
| `Wisp 0.3.0 · eqlog_Daggo_freeport.txt · idle` | disabled | The status line. `idle` becomes `fighting 42 s` while an encounter is active, and the file name becomes `waiting for a log` when the snapshot has none. |
| `HUD mode` | checkbox | Sends `ToggleHudMode`; the render loop enters or leaves HUD mode exactly as the chord does, including the §4.7 refusal when the layout failed to parse, in which case the checkbox stays unchecked and the refusal line prints as it does for the chord. The checkbox mirrors `TrayState.hud_mode`, so a mode entered by the chord shows checked. |
| `Open config` | item | Sends `OpenConfig`; the render loop spawns `xdg-open <config path>` detached and does not wait. A missing `xdg-open` prints one line. In the Flatpak, `xdg-open` is the portal shim and works without extra permissions. |
| `Stop Wisp` | item | Sends `Stop`; the render loop opens a fresh connection to the socket, writes `STOP_LINE`, and continues rendering. The daemon exits, the launcher stops the HUD. If the write fails (the daemon is already gone), the HUD exits 0 itself, which brings the launcher down the same way. |

The item's id is `io.github.jds300.Wisp`, its category `ApplicationStatus`,
its title `Wisp`, its status `Active`. The icon is the theme name
`io.github.jds300.Wisp`, which the tarball's `install.sh`, the AppImage and
the Flatpak all install; because an AppImage run from `~/AppImages` may not
have installed its icon, the item also carries embedded 22 px and 48 px ARGB
pixmaps, so the tray shows the wisp everywhere. The pixmaps are committed as
raw bytes under `crates/wisp-hud/icons/` and pulled in with `include_bytes!`;
`packaging/render-tray-icon.sh` regenerates them from
`packaging/io.github.jds300.Wisp.svg` with `rsvg-convert` and `magick`, both
on the development box, and is run by hand when the SVG changes. No build
script, no rasteriser in the dependency tree, and CI needs neither tool. Left
click activates HUD mode toggle (SNI `Activate`); the menu is the
right-click.

**The Flatpak** gains `--talk-name=org.kde.StatusNotifierWatcher` in
`finish-args`; without it the tray thread's registration fails and the HUD
reports no tray, which is the correct degradation and the reason the line is
added rather than assumed. `--talk-name` only lets the sandbox address the
watcher — it grants no well-known bus name of the tray's own, so inside a
Flatpak the tray does not own a name on the session bus; `ksni`'s
`disable_dbus_name(true)` is set whenever `FLATPAK_ID` is present, and
registration still succeeds without one.

### 4.3 The log name in the snapshot — protocol v5

The status line wants the file being tailed, and only the daemon knows it.
`Snapshot` gains `log: Option<String>`, the file name (not the path: the
socket is world-readable to the session and a path says where the game is
installed) of the current source, `None` while waiting for a log. Serialised
with `#[serde(default)]` so a v4 reader still decodes. `PROTOCOL_VERSION`
becomes 5 and the version comment gains the line. `wisp status` prints it as
`log:       <name>` under `log time:`, and `--json` carries the field.

### 4.4 Release channels

A release is a tag `v<version>` on `main` where `<version>` is exactly the
workspace version in the root `Cargo.toml`. release.yml already refuses a tag
that does not match. Two channels, decided by the version string alone:

| Channel | Version | Tag | GitHub release | Update source embedded in the AppImage |
|---|---|---|---|---|
| live | `0.3.0` | `v0.3.0` | normal | `gh-releases-zsync\|JDS300\|wisp\|latest\|Wisp-*-x86_64.AppImage.zsync` |
| beta | `0.3.0-beta.1`, `0.3.0-rc.2` | `v0.3.0-beta.1` | marked prerelease | `gh-releases-zsync\|JDS300\|wisp\|latest-pre\|Wisp-*-x86_64.AppImage.zsync` |

The facts that make this work, verified in Gear Lever's source on
2026-09-11 (`src/models/GithubUpdater.py`, `UpdateManagerChecker.py`):

- Gear Lever reads the `.upd_info` ELF section of the installed AppImage and
  picks the GitHub updater when it finds `gh-releases-zsync|`.
- A release field of `latest` calls GitHub's `releases/latest`, which
  excludes prereleases and drafts. A live user never sees a beta.
- A release field of `latest-pre` (or `latest-all`; Gear Lever treats them
  the same) lists all releases and takes the newest non-draft. A beta tester
  therefore also moves to the next live release when it is newer, and moving
  back to the live channel is installing a live build.
- The displayed version is `X-AppImage-Version` from the desktop entry, so a
  beta shows as `0.3.0-beta.1`. The AppImage's own `--version` prints the same.
- An update is detected by the `.zsync` header's hash, so the `.zsync` asset
  must be published beside every AppImage, as release.sh already ensures.

`latest-pre` is Gear Lever's word, not AppImageUpdate's; the reference
`AppImageUpdate` tool would not resolve it. Accepted: JDS300 uses Gear Lever,
the live channel stays standard, and a beta build is by definition for
someone who has read RELEASING.md.

**release.sh** reads the version, and when it contains `-` passes
`latest-pre` instead of `latest` to `appimagetool -u`. The choice is echoed on
stderr as `release.sh: channel beta (latest-pre)` or `channel live (latest)`.

**release.yml** passes `prerelease: ${{ contains(github.ref_name, '-') }}` to
`action-gh-release`, and `make_latest: ${{ !contains(github.ref_name, '-') }}`
so a beta never becomes GitHub's "latest" even if it is newest. Nothing else
changes.

**`packaging/cut-release.sh <version>`** is the one way to make a tag:

1. Refuses unless: the current branch is `main`; the tree is clean; `main` is
   at `origin/main` after a `git fetch`; `<version>` is a valid Cargo semver
   greater than the current workspace version; the tag `v<version>` does not
   exist locally or on origin. Each refusal is one line and exit 2.
2. Rewrites `version = "..."` in `[workspace.package]`, runs `cargo update
   --workspace --offline` so the lockfile follows, and inserts
   `<release version="<version>" date="<today>"/>` at the top of the
   metainfo's `<releases>`. Beta versions are inserted too — AppStream allows
   `type="development"` and the script sets it for a prerelease suffix.
3. Runs `cargo build --workspace --locked` and `cargo test --workspace
   --locked` (under `xvfb-run` when it is on `PATH`, as CI does; the HUD
   readback test skips itself otherwise). A failure leaves the edits in the
   tree, uncommitted, and exits 1 saying so.
4. Commits `Release <version>` (the body names the channel), creates the
   annotated tag `v<version>` with the same message, and pushes the commit
   and the tag in one `git push origin main v<version>`.
5. Prints the release workflow URL to watch and, for the live channel, the
   reminder that Gear Lever will offer it; for beta, the reminder that only
   `latest-pre` builds will.

`--dry-run` does steps 1–3 and prints the commit and tag it would make.
Every refusal is testable against a scratch repository, and the script's
tests do that: not on main, dirty tree, behind origin, version not greater,
tag exists.

**`docs/RELEASING.md`** is the delivery path in prose for the person cutting
the release: the two channels and when to use which (beta for anything JDS300
wants to try over the game before everyone gets it; live when a beta has held
up, or for a change small enough not to need one); the one command; what to
verify afterwards (the release run green, four assets on the release, `gh api
repos/JDS300/wisp/releases/latest` naming the right tag for a live cut, Gear
Lever showing the version after its update); the Gear Lever facts above; and
what never to do (hand-copy anything into `~/AppImages`; tag by hand; tag a
branch). README's release section points at it.

### 4.5 HUD mode owns the keyboard — layer shell

`zwlr_layer_surface_v1` has `set_keyboard_interactivity`. The HUD sets `None`
today and, belt and braces, an empty input region. This spec keeps both
outside HUD mode and, on the layer-shell backend only, sets `Exclusive` when
the chord enters the mode and `None` again when `Esc` or the chord leaves it.
With `Exclusive` on the overlay layer the compositor routes the keyboard to
the HUD and to nothing else; the game gets no arrow keys. With `None` again
the compositor returns focus to the toplevel that had it. KWin implements the
protocol and the switch; Milestone 8 is where that is proven on JDS300's
session rather than believed.

Two consequences shape the implementation.

**Polling goes blind while the HUD has focus.** `XQueryKeymap` through
Xwayland reports keys only while an X window holds the keyboard, so once the
HUD (a Wayland surface) has it, every HUD-mode key including the exit must
come from `wl_keyboard` events. The chord that *enters* the mode stays on
polling, because the game has focus then. The `Backend` trait gains:

```rust
/// Ask for, or give back, the keyboard. A no-op that returns `false` on
/// backends that cannot (every X11 one), so the caller keeps polling.
fn take_keyboard(&mut self, exclusive: bool) -> bool;
/// Key events since the last call, in order. Empty on X11 backends.
fn drain_keys(&mut self) -> Vec<KeyEvent>;   // KeyEvent { key: Key, pressed: bool }
```

In HUD mode the main loop feeds `HudMode::handle` from `drain_keys` when
`take_keyboard(true)` returned `true`, and from the poller as today when it
returned `false`. The `keys::Key` set is unchanged; `Shift` is tracked from
its own press and release like any other key.

**No libxkbcommon.** smithay-client-toolkit's keyboard helpers sit behind its
`xkbcommon` feature, which binds the C library and would end the static musl
build. The backend implements `Dispatch<wl_keyboard::WlKeyboard>` itself and
maps the raw evdev codes `wl_keyboard.key` carries:

| Key | evdev | Key | evdev |
|---|---|---|---|
| `Escape` | 1 | `BracketLeft` / `BracketRight` | 26 / 27 |
| `Tab` | 15 | `F` / `H` | 33 / 35 |
| `Up` / `Down` | 103 / 108 | `Plus` | 13 (`=`), 78 (keypad) |
| `Left` / `Right` | 105 / 106 | `Minus` | 12, 74 (keypad) |
| `Shift` | 42, 54 | `Chord` | grave 41 with ctrl 29 or 97 and a shift held |

Arrows, Tab, Esc and Shift are layout-independent. The letters and brackets
are physical positions — the keys under those caps on a US layout. Accepted
for eight keys with a help strip that names them; the alternative is parsing
the compositor's xkb keymap file by hand, which this spec declines.

**When the compositor does not play.** If no `wl_keyboard.enter` arrives
within 500 ms of asking for `Exclusive`, the backend reverts to `None`,
prints `wisp-hud: the compositor did not give the HUD the keyboard; HUD-mode
keys will also reach the game`, and `take_keyboard` returns `false` for the
rest of the run. HUD mode still works as it does today.

**Gamescope stays as it is, with one candidate.** The Spec 5 spike showed
that becoming gamescope's focus is a one-way door. An X keyboard *grab* — the
core-protocol grab request on the HUD's own window, `owner_events = false`,
async modes, and the matching ungrab on exit — is a different mechanism: the
server routes key events to the grab window without changing the focus
window, and the ungrab routes them back without a focus change either, which
is exactly the step that failed. Untested, and gamescope's `wlserver` sits
between the X server and the real keyboard, so it may forward nothing to a
grab or may not resume after one. Milestone 9 is a spike with a hard pass
criterion; the grab is implemented only if the spike passes, in a follow-up,
and until then HUD mode on gamescope leaks the arrows and `wisp hud place`
from a terminal is the leak-free path. The spec records the answer either
way.

### 4.6 The CLI

`wisp stop` joins the usage text:

```
       wisp stop
```

and the README's command table. `wisp status` gains the `log:` line (§4.3).

---

## 5. Milestones

| # | Milestone | Verified by |
|---|---|---|
| 1 | Protocol v5: `log` in the snapshot | `wisp status` and `--json` show the tailed file's name against a stub and against the frozen fixture; a v4 line still decodes |
| 2 | `wispd` reads the stop line | Unit tests: `stop\n` ends the loop; a silent client, a partial line, garbage and an over-long line leave the server running and the client connected; the socket file is gone after the stop |
| 3 | `wisp stop` | CLI tests against a spawned `wispd --stub`: exit 0, daemon gone, socket gone; against nothing listening: the one line, exit 0; with `wisp run` in the picture: the launcher exits 0 and the HUD is gone |
| 4 | Tray spike | `wisp-hud` builds for `x86_64-unknown-linux-musl` with ksni; `ldd` says statically linked; the size delta over v0.2.0's `wisp-hud` is recorded in the plan (the acceptance line is under 3 MB added, stripped) |
| 5 | Tray menu model | `TrayState` → menu text and check state is a pure function under unit test; each `TrayEvent` drives the render loop's handler under test with a fake socket for `Stop` |
| 6 | Tray, live on Plasma | JDS300: the wisp icon appears in the Plasma tray when `wisp run` starts; the status line reads the right file and flips to `fighting` in a fight; the checkbox follows the chord and drives it; Open config opens the file; Stop Wisp takes everything down and the game stays up; no icon and one stderr line under `Xvfb` |
| 7 | Release channels | Shell tests for every `cut-release.sh` refusal on a scratch repo; `release.sh` on a `-beta.1` version embeds `latest-pre` (checked with `readelf -p .upd_info`) and on a plain version embeds `latest` |
| 8 | HUD mode owns the keyboard, layer shell | Unit tests: the evdev map, Shift tracking, the 500 ms fallback with a fake that never sends `enter`; JDS300 on Plasma over the game: the chord enters the mode, the arrows move the block and the character stands still, `Esc` leaves the mode and the very next arrow moves the character; ten cycles without a stuck focus |
| 9 | Gamescope keyboard grab spike | A throwaway probe inside gamescope with the game running: grab on the chord, ungrab on `Esc`; pass only if the game receives keyboard and mouse after the ungrab three cycles in a row. Result recorded in an appendix to this spec; no Wisp code changes from the spike |
| 10 | Live, the delivery path | JDS300 cuts `v0.3.0-beta.1` with the script; the release is marked prerelease; `releases/latest` still says `v0.2.0`; a Gear Lever install of the beta shows `0.3.0-beta.1`; then `v0.3.0` live; Gear Lever on v0.2.0 offers 0.3.0 and never offered the beta |

---

## 6. Acceptance criteria

- `wisp stop` with a running `wisp run`: within 1 s, `pgrep -x wispd`,
  `pgrep -x wisp-hud` and `pgrep -x wisp` are all empty, the socket file is
  gone, and `wisp run`'s exit status is 0.
- `wisp stop` with `wisp run -- sleep 60` running: the same, and `sleep` is
  still running.
- `wisp stop` twice: the second prints exactly `wisp: nothing to stop: no
  daemon is listening on <path>` on stderr and exits 0.
- A client that connects and writes `stopp\n`, `{"stop":true}\n`, 300 bytes
  without a newline, or nothing: the daemon keeps publishing to it and to
  others.
- On a Plasma session, `busctl --user list | grep StatusNotifierItem` shows
  the HUD's item within 2 s of `wisp run`, and shows nothing 2 s after
  `wisp stop`.
- Under `xvfb-run` with no session bus, `wisp-hud` starts, draws, and prints
  exactly one `wisp-hud: no tray:` line.
- `cargo build --release --target x86_64-unknown-linux-musl -p wisp-hud`
  produces a `static-pie linked` binary; `wisp` still has no dependencies in
  its `Cargo.toml`.
- `packaging/cut-release.sh 0.3.0-beta.1 --dry-run` on a scratch clone of
  `main` prints the commit and tag it would make and changes nothing that
  `git status` shows beyond the three bumped files; each refusal in §4.4
  exits 2 with its one line.
- On Plasma in HUD mode, with the game focused before the chord: holding `→`
  for one second moves the selected block and the character does not move;
  after `Esc`, the same key moves the character and the block stays put.
  `wisp hud` shows the new offset.
- Under `xvfb-run` (plain-window backend) HUD mode behaves exactly as v0.2.0:
  `take_keyboard` returns `false` and the poller drives the keys.
- After `cut-release.sh 0.3.0-beta.1` for real: the release workflow is green;
  the release is marked pre-release; `gh api repos/JDS300/wisp/releases/latest
  -q .tag_name` prints the previous live tag; the AppImage's `.upd_info` reads
  `latest-pre`; `--version` prints `wisp 0.3.0-beta.1`.

---

## 7. Risks

| Risk | Answer |
|---|---|
| **ksni and zbus pull an async runtime and a lot of code into a static binary.** | Measured: with the tray unreachable (dependency added, nothing calling it) the stripped musl `wisp-hud` did not grow at all; with the tray wired up and reachable it grew by 3,324,904 bytes, to 7,069,344 — over this row's 3 MB line. The `blocking` + `async-io` features still avoid tokio, and every other binding constraint holds (`static-pie linked`, `statically linked`, no new C dependency, `wisp`'s `Cargo.toml` still dependency-free). Accepted rather than falling back to a hand-written SNI over `zbus` alone: the bulk is `zbus`'s own async/serialisation machinery, which the fallback would keep regardless. **Correction (Task 9).** The whole-branch review re-measured the shipped binary against a byte-for-byte reproduction of the v0.2.0 baseline and found the true delta was worse than this row says: +3,390,440 bytes, to 7,130,784. Rather than amend Milestone 4 to accept an overage, `[profile.release] lto = "fat"` and `codegen-units = 1` were added to the root `Cargo.toml`. Under that profile the baseline itself shrinks to 3,437,104 and the shipped `wisp-hud` to 5,888,416 — a delta of +2,451,312, back under this row's 3 MB line — while every binding constraint above still holds and `Cargo.lock` is untouched. This paragraph is corrected rather than rewritten so the record shows the overage that was actually measured and what closed it. |
| **Reading from clients on a non-blocking socket can spin or block the tick.** | One `read` per client per tick into a fixed buffer, `WouldBlock` is the normal answer, and the tick already sleeps. A client that floods is capped at 256 bytes between newlines and otherwise ignored. |
| **A beta tester on `latest-pre` is offered a newer live release and loses the beta channel.** | By design: the live release is newer and better. The next beta is offered again because it is newer still. RELEASING.md says so. |
| **GitHub's `make_latest` and `prerelease` flags drift.** | Both set explicitly on every release; the live-cut acceptance line checks `releases/latest` after each beta. |
| **`cut-release.sh` edits three files with `sed` and gets one wrong.** | The script re-reads each file after editing and refuses if the version does not read back — the same round-trip discipline Spec 5's config writer uses — and the build-and-test step runs on the edited tree before anything is committed. |
| **The tray's `Activate` (left click) toggling HUD mode surprises someone who expected a menu.** | Plasma opens the menu on right click and shows the title on hover; left click is the SNI convention for the item's primary action, and the mode toggle is the only action that is harmless to hit twice. If it annoys in Milestone 6, `Activate` becomes a no-op and the menu is the only surface — a one-line change. |
| **KWin ignores `Exclusive` on the overlay layer, or gives focus but never gives it back.** | Milestone 8 runs ten cycles before anything ships; the 500 ms fallback covers the first case at runtime. The second would be a KWin bug to report, and the release fallback is the Spec 5 behaviour behind a config key `hud.take_keyboard = false`, added only if that day comes. |
| **Physical-position letters confuse a non-QWERTY user.** | The help strip names the keys; six of the eight are layout-independent; the two letters and two brackets are documented as positions. Parsing the xkb keymap is the fix if it is ever asked for. |
| **A game that reads the keyboard through evdev or a raw device, not the compositor.** | Wine under Xwayland reads through X. A game that bypassed the compositor would still see the arrows; nothing in user space can stop that, and the spec does not claim to. |
| **`xdg-open` opens the config in something unhelpful.** | It opens what the desktop associates with plain text; that is the user's choice to make. `ksni` 0.3.6's dbusmenu items carry no tooltip field, so the menu entry cannot name the path itself — `wisp config path` prints it instead. |
