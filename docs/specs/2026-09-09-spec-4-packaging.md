# Spec 4 — packaging and distribution

**Status:** implemented 2026-09-10; build, test, clippy, the musl static build, `packaging/release.sh`, `packaging/check-container.sh` and the Flatpak build (`packaging/flatpak/build.sh`, `packaging/flatpak/check.sh`) verified locally; pending JDS300: the first CI run on a pushed tag, the Flatpak's live checks against the game, and Milestone 6
**Depends on:** [Spec 3 — encounters](2026-09-08-spec-3-encounters.md), [Spec 2 — timers](2026-09-08-spec-2-timers.md), [Spec 1 — the spine](2026-09-08-spec-1-the-spine.md), [Spec 0 — clean-room charter](2026-09-08-clean-room-charter.md)
**Target client:** EverQuest Legends. Not Live, not Project Quarm.

---

## 1. What this spec is for

Spec 0 §2 named three components and two of them are built. This spec builds the
third — `wisp`, the CLI and control client — and then makes the result
installable: a config file, log discovery, static binaries, a tarball, an
AppImage, a Flatpak, and the two GitHub workflows that produce them. Nothing here
changes what the parser counts or what the HUD draws.

**Done means:** on a Linux machine that has never seen the source tree, one
downloaded artifact installs Wisp; `wisp config set logs_dir <dir>` then
`wisp run` finds the log itself and puts the HUD over the game; and on JDS300's
desktop the packaged build — the AppImage — runs the same live path the source
build ran in Specs 1–3. The handheld is not part of done: `PROVENANCE.md`'s
standing rule ("handheld pending") allows no Legion Go S testing until the
desktop application is complete.

---

## 2. Non-goals

Explicitly deferred, so their absence is a decision rather than an oversight:

- The tall panel's layout, theming, scale presets, and any HUD configuration
  beyond `scale` and `backend` — **Spec 5**. Spec 3 §7 said "Layout is Spec 4's
  problem"; it is Spec 5's.
- A cleaner mob-marking rule, and the rest of Spec 3 §7's parser backlog:
  single-word named NPC special attacks, other players' summoned pets, and
  `X has taken N damage by <Spell>.` lines — **Spec 5**, with a new reference
  script when it happens.
- Flathub submission, AUR, deb, rpm, Nix — later. Flathub is JDS300's action.
- Handheld verification — the standing rule above.
- Auto-update inside the application, a systemd user unit, a tray icon.
- Log discovery by searching the disk. **Wisp never searches the disk**: it reads
  one configured directory, or one configured file. On the development box the
  prefix is `/mnt/Data4TB/Games/everquest/prefix`, not under `$HOME`, so a
  "well-known roots" search would fail on the machine it was designed on.
- Windows, macOS. Linux only, as Spec 0 says.
- Porting `tools/appimage_update_info.py` — Tier A, permitted, **declined**:
  `appimagetool -u` writes the update-information string itself. `PROVENANCE.md`
  records it in the Tier A row ("Ported: no — declined") and a dated log entry.

---

## 3. Invariants

Everything the first three specs made binding stands and is untouched here: the
HUD never takes input on any backend (this spec changes Cargo features and moves
functions, never a backend's drawing code); game data is read at runtime and
never shipped, so no artifact contains `spells_us.txt` or anyone's log; naive
timestamps are subtracted, never relabelled; only lines that carry an amount
count. Three rules are new, and binding from Spec 4 on:

1. **Every release artifact is produced by a script that also runs locally.** CI
   calls `packaging/release.sh`; a human running the same script gets
   byte-for-byte the same file list. Nothing in a release is made by hand.
2. **The tarball and AppImage binaries need nothing from the host but a kernel
   and a display.** They are static (musl), the font is already embedded
   (`include_bytes!` in `wisp-hud/src/text.rs`), and the only files looked up at
   runtime are the user's own config, the user's own duration store, and the
   game's own files.
3. **Every new file carries the SPDX header** — the three new crates' sources,
   `release.sh`, `build.sh`, `regen-cargo-sources.sh`, `install.sh`, both
   workflows, the desktop file, the SVG and the metainfo — as
   `SPDX-License-Identifier: MIT`, in whatever comment syntax the file has, per
   charter §6.

---

## 4. Architecture

### Workspace

Spec 1 §4's tree grows by three crates:

```
crates/
  wisp-proto/     snapshot types + NDJSON codec + client  (lib)
  wisp-config/    config, socket path, discovery          (lib)   new
  wisp-probe/     backend detection                       (lib)   new
  wispd/          the daemon                              (bin)
  wisp-hud/       the overlay renderer                    (bin)
  wisp/           the CLI                                 (bin)   new
```

The rest of what this spec adds is files rather than crates — `packaging/` and
`.github/workflows/`, each described below.

| Moves | From today | To, and the rule it carries |
|---|---|---|
| `socket_path()` | `wispd/src/server.rs` and `wisp-hud/src/client.rs`, which implement one rule in slightly different code | `wisp-config`. `$XDG_RUNTIME_DIR/wisp/wispd.sock`, else `/run/user/<uid>/wisp/wispd.sock`; **when `FLATPAK_ID` is set**, `$XDG_RUNTIME_DIR/app/$FLATPAK_ID/wispd.sock` — the directory Flatpak shares between instances of the same app, so `wisp status` in a second `flatpak run` sees the daemon a first one started. Verified 2026-09-09 in another app's sandbox (appendix); Milestone 5 confirms it with Wisp's own |
| `spells_dir_from_log()` | `wispd/src/spells.rs` | `wisp-config`. A log under a directory named `Logs`, case-insensitively, means the client install is that directory's parent |
| `connect()`, `SnapshotStream` | `wisp-hud/src/client.rs` | `wisp-proto`, as a `client` module beside the codec they already call. `wisp status` needs them and a binary crate cannot export them; `wisp-hud` calls them from there and its `client.rs` goes away. No new dependency: `UnixStream` is stdlib |
| `root_atom_names()`, `wayland_globals()`, `choose()`, `BackendKind` | `wisp-hud/src/backend/` — the probes in `gamescope_x11.rs` and `layer_shell.rs`, the rule and its vocabulary in `mod.rs` | A new crate, `wisp-probe`. Detection, not drawing, and `wisp doctor` must report the choice without reimplementing it |
| the config file, the discovery rule | new | `wisp-config`. Below; the discovery rule is a pure function over `(directory listing, mtimes)` so it is unit-testable, and the polling stays in `wispd` |

`wisp-config` carries no display dependency at all, and detection is a crate of
its own rather than a feature of one, because a feature would not have worked:
Cargo unifies the features of a shared dependency across workspace members in a
single invocation, so `cargo build --workspace` — exactly what `ci.yml` runs —
would build `wisp-config` with the probes enabled and `wispd` would link `x11rb`
and `wayland-client` anyway. A crate boundary is the only split Cargo respects.
Only `wisp` and `wisp-hud` depend on `wisp-probe`.

`wisp-probe` holds detection and nothing else. Both probes are pure reads, each
returning an empty list when there is no display, which is a valid answer for
selection, and `choose` is a pure function over the two lists. `BackendKind`
moves with `choose` because it is the choice's vocabulary; `OverlayBackend`,
`Frame` and the three backend implementations stay in `wisp-hud`, which now calls
`wisp-probe` for both. Spec 0's "`wispd`: no UI, no display connection" holds
literally: the daemon depends on `wisp-config` and `wisp-proto` and on nothing
that can open a display.

### The config file

Path: `$XDG_CONFIG_HOME/wisp/config`, falling back to `$HOME/.config/wisp/config`
— the rule `DurationStore::default_path()` already applies to `XDG_DATA_HOME`.
`XDG_CONFIG_HOME` counts only when it is absolute, as in that function. With
neither it nor `HOME` set, `wisp config` exits 1 with a message and the daemons
treat the config as empty; there is **no current-directory fallback** anywhere in
this spec. Inside a Flatpak the path resolves to
`~/.var/app/io.github.jds300.Wisp/config/wisp/config` by itself, because the
sandbox sets `XDG_CONFIG_HOME`.

Format: one `key = value` per line. `#` starts a comment, blank lines are
ignored, and the value is everything after the first `=`, trimmed — so a path
with spaces needs no quoting. Unknown keys are reported once on stderr and
ignored. **It is not TOML and does not claim to be**: five keys do not justify a
dependency, and the project's hand-rolled argument parsing sets the precedent.

| Key | Read by | Meaning |
|---|---|---|
| `log` | wispd, wisp | One log file, as `--log` |
| `logs_dir` | wispd, wisp | The game's `Logs/` directory, as `--logs-dir` |
| `spells_dir` | wispd, wisp | The client install directory, as `--spells` |
| `scale` | wisp-hud, wisp | As `--scale` |
| `backend` | wisp-hud, wisp | `gamescope`, `layer-shell` or `plain`, as `--backend` |

Precedence: a command-line flag beats the file, the file beats the built-in
default. `log` beats `logs_dir` when both are set — a file is more specific than
a directory. Each binary reads only its own keys, and `wisp run` passes nothing
along that the user did not give it. `wisp config set` checks the key, not the
value: `backend = nonsense` is written, and `wisp-hud` refuses it at start as it
refuses `--backend nonsense` today.

### Log discovery — `wispd --logs-dir`

A new flag beside `--log`. The daemon polls the directory — one `readdir` on the
tick it already runs every 250 ms — and tails the `eqlog_*.txt` with the **newest
modification time**, ties broken by name (the later one in byte order). On the
development box the install's `Logs/` holds `eqlog_Daggo_freeport.txt` (modified
2026-09-09) and `eqlog_Daggo_rivervale.txt` (2026-08-12), so the rule picks the
first.

| Situation | Rule |
|---|---|
| No matching file, or the directory does not exist yet | wispd runs anyway, publishes snapshots with zero counters and an empty `ts`, and says once on stderr that it is waiting for a log in `<dir>`. The game creates the file on login; the daemon must not need restarting |
| The newest file appears or changes — first login, or another character or server | One rule, whether or not a file was open. Close the tailer if there is one; reset the counters, the encounter tracker and the timer tracker's live state; read the player's name from the new filename; open the new file **at its end**, which is `Tailer::open` with `from_start` false. `--from-start` applies only to the very first file opened |
| What a switch keeps | The spell table and the duration store. `timers::Tracker` gains a `reset()` — or an equivalent — that clears live timers and pending casts and keeps both, so a switch on a poll tick never reparses the 73,975-row `spells_us.txt`. Learned samples survive a switch, on disk and in memory |
| Spells | `--spells` beats config `spells_dir` beats the derived location: `spells_dir_from_log` of the logs directory, i.e. its parent when it is named `Logs`. That function takes a *file* and calls `parent()` on it, so `wisp-config` needs a second entry point for the directory case |
| Nothing to tail | With neither `--log` nor `--logs-dir` on the command line, nor `log`/`logs_dir` in the config, and no `--stub`: exit 2 with a message naming the config file's path and the command `wisp config set logs_dir <dir>`. `--stub` reads neither key |

wispd's usage line becomes:

```
wispd [--log <path> | --logs-dir <dir>] [--from-start] [--spells <dir>]
wispd --stub
```

### The `wisp` CLI

Spec 0's third component, finally built: `crates/wisp`, a binary depending on
`wisp-proto` (to decode snapshots), `wisp-config` and `wisp-probe`. Argument
handling is hand-rolled like the other two binaries — `args_os`, no clap.

| Command | Does |
|---|---|
| `wisp run [wispd flags] [hud flags] [-- <command>…]` | Starts `wispd`, waits for it to accept a connection (≤ 5 s), starts `wisp-hud` |
| `wisp status [--json]` | Connects through `wisp-proto`'s `client` module and prints the first snapshot — kills, lines, timers, the fight — as text, or as the raw NDJSON line with `--json`. Exit 1 with a message when no daemon is listening |
| `wisp doctor` | Starts nothing. Prints the version; the config path and whether the file exists; the log it would tail and how it was chosen (flag, config `log`, config `logs_dir` and the newest file there, or nothing); the spells directory and whether `spells_us.txt` is present; the socket path and whether something is listening; and the backend the HUD would choose and why, by calling `wisp-probe` — `DISPLAY` and whether its root window carries `GAMESCOPE_*` properties, and whether the compositor advertises `zwlr_layer_shell_v1`. Exit 1 when no log can be resolved, else 0. As implemented, `wisp doctor` also accepts the five value flags `run` forwards (`--log`, `--logs-dir`, `--spells`, `--scale`, `--backend`), prints a seventh `scale:` line, stats the log it names (missing → exit 1), and reports the backend by the HUD's full precedence — flag, then config, then `wisp-probe` detection — naming what detection alone would have chosen; see the plan's known gaps 8 and 10. |
| `wisp config path` / `show` / `set <key> <value>` | The path; every key with its value (or "unset"); write one key, creating the file and its directory, preserving other lines and comments, refusing an unknown key with exit 2 |
| `wisp --version` / `wisp version` | `wisp <version>`, from `CARGO_PKG_VERSION` |
| `wisp` alone, or an unknown command | Usage on stderr, exit 2. Bare `wisp` on `PATH` does **not** mean `run`; the desktop entry and the AppImage's `AppRun` say `run` explicitly |

`wisp run` is the launcher. It forwards `--log`, `--logs-dir`, `--spells`,
`--from-start` and `--stub` to wispd, `--scale` and `--backend` to wisp-hud, and
recognises nothing else. Both children are found beside `wisp`'s own executable
first (`current_exe().parent()`) and on `PATH` second, which is what lets an
AppImage work with a two-line `AppRun`. With a command after `--` it runs that
too and, when the command exits, stops the HUD and the daemon and exits with the
command's status — the `mangohud %command%` convention, so a Steam or Lutris
launch option of `wisp run -- %command%` works. Without a command it runs until
SIGINT or SIGTERM, or until a child exits, then stops the other; exit 0 on a
clean stop, 1 if a child failed. When a command was given and it is the HUD or
the daemon that dies, `wisp run` says so on stderr, tears down the other Wisp
child and leaves the command running: the game is what the user launched, and
losing an overlay is no reason to close it.

Two guards sit around the start. **Before** starting anything, `wisp run` tries
the socket: if something is already listening it prints the path and exits 1
rather than take it from a running daemon. **After** starting wispd, readiness is
a successful connect, not the appearance of the socket file — a dead daemon
leaves its file behind and `Server::bind` deletes whatever is at the path before
binding, so a file test can pass too early. On the 5 s timeout `wisp run` kills
the daemon child, prints the socket path and the timeout on stderr, and exits 1.

### Static binaries

Target `x86_64-unknown-linux-musl`. One Cargo change makes the workspace link for
musl:

```toml
smithay-client-toolkit = { version = "0.21.1", default-features = false, features = ["calloop"] }
```

The toolkit's `default` feature set is exactly `["calloop", "xkbcommon"]`, and
`xkbcommon` is what links `libxkbcommon` — which the HUD never uses, because
`layer_shell.rs` sets `KeyboardInteractivity::None` and nothing reads a keyboard.
Without the change the musl link fails on `-lxkbcommon`; with it, no
`PKG_CONFIG_ALLOW_CROSS` is needed either, because the toolkit's `build.rs` then
has nothing to probe. Verified on the development box, 2026-09-09; the sizes and
the test counts are in the appendix.

### Release artifacts — `packaging/release.sh`

Run locally and by CI, identical, and it sets `APPIMAGE_EXTRACT_AND_RUN=1` before
invoking `appimagetool` — a CI runner has no FUSE, and locally it is harmless. The
version is read from the workspace `Cargo.toml`; `wisp --version` prints the same
string; a release tag must be `v<version>` and the workflow refuses otherwise.
`dist/` is already ignored (`.gitignore`), so release output is never committed.

| Artifact | Contents |
|---|---|
| `dist/wisp-<version>-x86_64-linux.tar.gz` | `wisp-<version>/` holding ten files: `wisp`, `wispd`, `wisp-hud` (stripped), `install.sh`, `LICENSE`, `THIRD_PARTY.md`, `README.md`, `io.github.jds300.Wisp.desktop`, `io.github.jds300.Wisp.svg`, `io.github.jds300.Wisp.metainfo.xml` |
| `dist/Wisp-<version>-x86_64.AppImage` | The AppDir below |
| `dist/Wisp-<version>-x86_64.AppImage.zsync` | Written by `appimagetool -u` |
| `dist/SHA256SUMS` | Over the three above |

`install.sh` is prefix-aware. The default prefix is `~/.local`: the three
binaries go to `bin/`, and the desktop file, icon and metainfo to their XDG
directories under `share/`. `--prefix <dir>` overrides it, and `--uninstall`
removes exactly what it installed and nothing else.

The AppDir: the three binaries in `usr/bin`; `AppRun` a two-line shell script
that `exec`s `"$APPDIR/usr/bin/wisp" "$@"`, defaulting to `run` when it is given
no arguments at all (`$# -eq 0`) so that a double-clicked AppImage runs while
bare `wisp` on `PATH` still prints usage; the desktop file at the root and under
`usr/share/applications`; the icon at the root and under
`usr/share/icons/hicolor/scalable/apps`; the metainfo under
`usr/share/metainfo`. `appimagetool` is fetched from its GitHub release at a
**pinned version and SHA-256** into a cache directory outside the repository,
never committed. Update information:
`gh-releases-zsync|JDS300|wisp|latest|Wisp-*-x86_64.AppImage.zsync`.
`./Wisp-<version>-x86_64.AppImage --version` prints the version; `… run` runs.
The workspace stays at `0.1.0` and the first published release is `v0.1.0`; spec
numbers are internal and do not drive semver.

### Flatpak — `packaging/flatpak/`

`io.github.jds300.Wisp.yml`: runtime `org.freedesktop.Platform` **25.08**, sdk
`org.freedesktop.Sdk` 25.08 with `org.freedesktop.Sdk.Extension.rust-stable`, and
`command: wisp`. Module `wisp` builds with cargo **offline** from
`cargo-sources.json`, generated by `flatpak-cargo-generator.py` from
`flatpak/flatpak-builder-tools` — fetched at a pinned commit by
`packaging/flatpak/regen-cargo-sources.sh`. The generated JSON **is committed**;
the generator is not. This is the ordinary glibc build against the runtime, not
musl. It installs `wisp`, `wispd`, `wisp-hud`, the desktop file, the icon and the
metainfo.

| `finish-args` | Why |
|---|---|
| `--socket=x11` | Only the socket named by `DISPLAY` at launch is bound in: `/tmp/.X11-unix` inside the sandbox is a fresh directory holding that one socket, and `XAUTHORITY` is a fixed-path copy. So the gamescope path is `DISPLAY=:N flatpak run io.github.jds300.Wisp run` with the game already running, and a Flatpak can never follow an XWayland that starts later. In a handheld Game Mode session `DISPLAY` is gamescope's from the start, so this is natural there |
| `--socket=wayland` | layer-shell on the desktop; `wayland-0` is present in the sandbox's runtime directory |
| `--filesystem=host:ro` | The log and the client's spell files live in a Wine prefix that can be anywhere — on the development box, under `/mnt`. The daemon reads three files and writes none of them |

No `--share=ipc`: x11rb's `PutImage` path uses no shared memory, so the sandbox
does not need it. The durations store and the config land in the app's own
`~/.var/app/io.github.jds300.Wisp/` through the sandbox's `XDG_DATA_HOME` and
`XDG_CONFIG_HOME`.

Three more files live once under `packaging/` and are copied by the scripts into
the tarball, the AppDir and the Flatpak. `io.github.jds300.Wisp.desktop`:
`Type=Application`, `Name=Wisp`, `Comment=` Spec 0's subtitle,
`Icon=io.github.jds300.Wisp`, `Exec=wisp run`, `Terminal=false`,
`Categories=Game;Utility;`, `NoDisplay=false`.
`io.github.jds300.Wisp.metainfo.xml`: MIT, `<name>Wisp</name>`, `<summary>` Spec
0's store subtitle — *EverQuest log parser and in-game overlay for Linux and
Steam Deck* — the tagline *A light over the fight.* as the first description
paragraph, one `<release>` for 0.1.0 dated 2026-09-09, and no screenshots yet, a
Flathub requirement deferred with the submission. `io.github.jds300.Wisp.svg`: a
new simple icon drawn by hand for this repository — a soft glowing point with a
short trailing wisp on a dark rounded square — MIT with everything else here.

`packaging/flatpak/build.sh` runs `flatpak-builder --user --install
--force-clean`, using `org.flatpak.Builder` from Flathub because the development
box has no native `flatpak-builder`, and optionally `flatpak build-bundle` to
`dist/Wisp-<version>.flatpak`. It passes `--install-deps-from=flathub`, since the
Platform, the Sdk and `rust-stable` are not installed there today (appendix).

### CI — `.github/workflows/`

Both workflows run `rustup target add x86_64-unknown-linux-musl` before building.

`ci.yml`, on push and pull request: `cargo build --workspace --locked`,
`cargo test --workspace --locked` under `xvfb-run -a` — two integration tests
start `wisp-hud` and need a display, which ubuntu-latest otherwise lacks —
`cargo clippy --workspace --all-targets --locked -- -D warnings`, then
`packaging/release.sh` as a smoke run whose output is uploaded as a workflow
artifact and not released. `--locked` on every cargo invocation keeps a
`Cargo.lock` drift from re-resolving silently. **No `cargo fmt --check`**: most
of the tree is not rustfmt-clean today (appendix), and reformatting is not this
spec's business.

`release.yml`, on a tag `v*`, declares `permissions: contents: write`, checks
that the tag equals `v<Cargo version>`, runs `packaging/release.sh`, and creates
the GitHub Release carrying everything the script left in `dist/` — all four
files, because the update-information string names the `.zsync` and AppImageUpdate
fetches it from the release. Actions are pinned by commit SHA.

Both workflows were verified locally with `actionlint`
(`docker run rhysd/actionlint`); the first real run happens when JDS300 pushes a
tag, and is recorded as pending like every other live milestone. The Spec 3
replay guard is **local-only**: the frozen fixture lives outside the repository,
so CI never has it and never runs it.

---

## 5. Milestones

| # | Milestone | Verified by |
|---|---|---|
| 1 | `wisp-config`, `wisp-probe`, `wisp-proto::client`: config file, socket path, discovery rule, detection, snapshot reader | Unit tests; `wispd --logs-dir <the real Logs dir>` tails `eqlog_Daggo_freeport.txt` |
| 2 | `wispd --logs-dir` with switch-and-reset; config precedence | Tests with a temp dir where a second file becomes newer; the Spec 3 replay still reproduces its §6 numbers exactly (regression guard) |
| 3 | `wisp run` / `status` / `doctor` / `config` | Tests against `wispd --stub`; `wisp run --stub -- sleep 1` starts and stops all three with no log at all; `wisp status --json` is one valid v3 line |
| 4 | Static build and `release.sh` | All three binaries `statically linked`; tarball, AppImage and `SHA256SUMS` in `dist/`; in a clean `ubuntu:24.04` container with only the tarball, `wispd --stub` + `wisp status` works and `wisp-hud --backend plain` runs under `Xvfb` for 5 s; `Wisp-*.AppImage --version` prints on the dev box; the pinned `appimagetool` version's FUSE requirement is recorded in the plan |
| 5 | Flatpak | `packaging/flatpak/build.sh` installs and its offline cargo build resolves all six workspace crates; `flatpak run io.github.jds300.Wisp --version`; a daemon in one `flatpak run` instance is seen by `wisp status` in another; `flatpak run --command=sh io.github.jds300.Wisp -c 'echo $DISPLAY; ls /tmp/.X11-unix'`; `appstreamcli validate --no-net` and `desktop-file-validate` pass |
| 6 | Live, desktop only | JDS300: `wisp config set logs_dir …`, then `./Wisp-<v>-x86_64.AppImage run` over EverQuest Legends shows the kill count, a timer and the fight panel as the source build did. Pending JDS300 |

---

## 6. Acceptance criteria

Exact, not impressionistic.

**Static build.** `cargo build --release --workspace --target
x86_64-unknown-linux-musl` succeeds; `file` reports each of `wisp`, `wispd` and
`wisp-hud` as `static-pie linked`; `ldd` says `statically linked`.

**Release script.** `packaging/release.sh` leaves exactly
`wisp-0.1.0-x86_64-linux.tar.gz`, `Wisp-0.1.0-x86_64.AppImage`,
`Wisp-0.1.0-x86_64.AppImage.zsync` and `SHA256SUMS` in `dist/`; `sha256sum -c
SHA256SUMS` passes; the tarball lists exactly the ten files of §4, Release
artifacts.

**Clean container.** `docker run --rm -v dist:/dist ubuntu:24.04` with the
tarball extracted and nothing else installed: `wisp --version` prints
`wisp 0.1.0`; `wispd --stub &` then `wisp status --json` prints one line that
decodes as protocol v3; `wisp doctor` exits 1 (no log) and prints the config
path; then, with `xvfb` installed and that same `wispd --stub` still running,
`timeout 5 xvfb-run wisp-hud --backend plain` exits **124** — the HUD never
exits on its own, so the timeout is the pass condition.

**Discovery.** Unit tests cover newest-wins, the name tie-break, an empty
directory, a missing directory, and a switch when a second file's mtime passes
the first; a switch resets `session_kills` to 0 and re-derives the player's name.
After a switch the duration store is unchanged on disk and still loaded, and the
spell table is not reparsed.

**Config.** `wisp config set logs_dir "/a path/with spaces/Logs"` then
`wisp config show` prints it back exactly; an unknown key exits 2; a comment line
survives a `set`.

**Flatpak.** The checks in Milestone 5.

**Replay.** With `WISP_FIXTURE` set, the Spec 3 replay test still reports every
§6 number unchanged — 2,524 encounters and the rest. Local-only; CI never has the
fixture.

**Docs and headers.** README's status row 4 updated; every new file carries the
§3 SPDX header; `PROVENANCE.md` gains the Tier A "declined" note and a dated Spec
4 entry (sources: MangoHud's `mangohud %command%` launch convention — product,
and MIT source is permitted anyway; the AppImage and Flatpak documentation;
`flatpak-builder-tools` for the cargo generator, whose licence is checked and then
recorded in `THIRD_PARTY.md`); `THIRD_PARTY.md` records `appimagetool` and the
generator as tooling used, not vendored.

---

## 7. Risks

| Risk | Standing |
|---|---|
| **A static musl binary cannot `dlopen`.** | Accepted: nothing here does, and nothing links a display library either. `cargo tree -p wisp-hud -e features` enables only the `default` feature of `wayland-sys` 0.31.11 — not `client`, the feature that declares the link to `libwayland-client` — and only `default` of `wayland-backend` 0.3.17, not `client_system`. So wayland-backend's own Rust implementation is what compiles, and `wayland-sys`'s `pkg-config` build-dependency runs only for libraries whose features are enabled. `x11rb` 0.14.0 pulls only `gethostname`, `rustix` and `x11rb-protocol`. |
| **musl's allocator is slower than glibc's.** | Accepted. Irrelevant at 250 ms ticks and a few snapshots a second. |
| **`getuid` is reached through a hand-declared `extern "C"`,** in `server.rs` and `client.rs`. | Accepted; it links on musl, verified by the spike. One declaration survives the move to `wisp-config`. |
| **The AppImage needs FUSE on the host.** | Accepted, with the fallback built in: `--appimage-extract-and-run` needs no FUSE at all, and `release.sh` already sets `APPIMAGE_EXTRACT_AND_RUN=1` for CI. What the pinned `appimagetool` runtime requires on a given host is recorded in the plan when the version is pinned, at Milestone 4. |
| **`--filesystem=host:ro` is broad.** | Accepted for this spec. Narrowing it is a Flathub-submission question, not this one. |
| **The Flatpak socket-directory rule** (`$XDG_RUNTIME_DIR/app/$FLATPAK_ID`). | Verified 2026-09-09 in another app's sandbox, twice — once for the brief, once by peer review B (appendix). Milestone 5 confirms it with Wisp's own id. If it failed, two instances could not see one daemon and `wisp status` would need the socket path as a flag. |
| **A Flatpak cannot follow a display that starts after it.** Only the socket named by `DISPLAY` at launch is bound in, so the Flatpak reaches gamescope's XWayland only when the game is already up. | Accepted. The desktop path for `wisp run -- %command%` is the AppImage, not the Flatpak (next row); on a handheld Game Mode session `DISPLAY` is gamescope's from the start, so the Flatpak is natural there. |
| **`wisp run -- %command%` under a desktop Lutris launch starts the HUD before gamescope's XWayland exists,** so the HUD attaches to the desktop's layer-shell. | Accepted. That is the mode Spec 1 verified live on this rig. **The AppImage, not the Flatpak, is the artifact for wrapping a gamescope launch on the desktop.** |
| **Discovery keys on mtime,** which the game touches on every line it writes. | Accepted; that is what makes the newest file the live one. |
| **A character switch resets the session,** by design. | Accepted. The counters are session counters, and the session ended. The spell table, the duration store and learned samples are not session state and survive (§4, Log discovery). |
| **The metainfo `<summary>` names the Steam Deck** while the handheld is unverified by standing rule. | Accepted. Spec 0 §2 fixed that subtitle as the product's target, and the README's status table is where verification is reported. |
| **The config format is not TOML.** | Accepted. If the file ever needs structure, switch to TOML then; the parser is one function. |
| **CI is unverified until the first tag.** | Accepted; `actionlint` locally is the stand-in, and the first real run is recorded as pending like every live milestone. |
| **Flathub is a separate submission,** with requirements this spec does not meet (screenshots, a narrower filesystem grant). | Deferred with §2. |
| **The handheld stays unverified.** | Standing rule. No Legion Go S testing until the desktop application is complete. |
| **Detection code in a shared crate would put `x11rb` and `wayland-client` into `wispd`'s link.** | Found by the writer; solved by the crate split in §4, Workspace. A Cargo feature would not have held: features of a shared dependency are unified across workspace members in one invocation, so `cargo build --workspace` would enable them for `wispd` too. `wisp-probe` is depended on by `wisp` and `wisp-hud` only. |
| **`Server::bind` deletes whatever is at the socket path before binding,** so a second `wispd` silently takes the socket from a running one, and a stale file makes a file-existence readiness test pass too early. | Found by the writer; both answered in §4, The `wisp` CLI: `wisp run` refuses to start when something is already listening, and treats a successful connect — never the file — as ready. |
| **`spells_dir_from_log` takes a file path,** so `--logs-dir` cannot derive the spells directory through it unchanged. | Found by the writer. §4, Log discovery, requires a directory entry point in `wisp-config`; Milestone 1 tests both. |
| **`Tailer::poll` re-stats the log by path every tick,** so a `Logs/` directory that loses its search permission freezes an open tailer while the daemon keeps publishing unchanged numbers. | Found by Task 3's `an_unreadable_directory_does_not_kill_the_daemon`. Accepted: the numbers stop moving rather than going wrong, the daemon stays alive and keeps publishing, and recovery is automatic when the mode returns. Recorded here for Spec 5, beside the row about reopening a replaced file. |
| **`Tailer::poll` reopens a replaced or truncated file from byte 0 with no state reset,** while the new reset rule covers only a change of newest file. | Found by the writer. Pre-existing Spec 1 behaviour, made visible here: a rewrite of the file being tailed re-ingests lines into a live session. Left alone by this spec, and recorded so Spec 5 inherits it knowingly. |
| **No manifest declares `rust-version`,** and `wispd/src/main.rs` already uses `u64::is_multiple_of`, which needs a recent stable toolchain. | Found by the writer. The dev box has 1.94.1; the Flatpak SDK's `rust-stable` is `rustc 1.98.1 (48a229cea 2026-09-01)`, measured 2026-09-10 (Task 7) — newer, not older, so the offline build succeeded without a `rust-version` declaration and none was added. |
| **The install has two directories named `Logs`** — the game's, and `LaunchPad.libs/Logs`, which holds no `eqlog_*.txt`. | Found by the writer. The discovery rule is safe, because it requires `eqlog_*.txt`, but a user can point `logs_dir` at the wrong one and wait forever. `wisp doctor` printing the resolved directory and what it found there is the answer. |
| **The cargo generator does not vendor the workspace's `path` crates.** | Accepted. `flatpak-cargo-generator.py` vendors registry sources from `Cargo.lock` only; the six workspace crates are in the tree and need no vendoring. Milestone 5's offline build confirms all six resolve. |
| **`wispd` given a `--log` (or config `log`) path that does not exist dies with a raw `Debug` dump of the `io::Error`** — `Error: Os { code: 2, kind: NotFound, … }` — instead of a message naming the file. | Found by Task 5's review. Deferred to Spec 5: `wisp run` reports `wispd exited before listening: <status>` beside it, so the failure is comprehensible from the launcher, and the fix is a daemon behaviour change that does not belong in a docs task. The replacement is one line — `wispd: cannot open <path>: <error>`, exit 1. |

---

## Appendix — verified facts

Two kinds of fact: those measured on the development box on 2026-09-09, when the
design was settled, which reached this spec through its brief; and those the
writer verified by reading this repository and running the commands quoted.
Nothing is inferred from another project.

**Development box.** CachyOS Linux; glibc 2.44 (`ldd --version`); rustc and cargo
1.94.1 (`rustc --version` → `rustc 1.94.1 (e408947bf 2026-03-25)`);
`x86_64-unknown-linux-musl` installed (`rustup target list --installed`); Flatpak
1.18.2; Docker 29.7.2 with the image `ubuntu:24.04` (117 MB); Flathub remote
configured; `gamescope`, `mangoapp`, `desktop-file-validate` and `appstreamcli` on
`PATH`. Absent: `appimagetool`, `linuxdeploy`, `flatpak-builder`, `podman`,
`xvfb-run`, `actionlint`.

**Flatpak runtimes — corrected by the writer.** The brief recorded
`org.freedesktop.Platform` 25.08 as installed; `flatpak info
org.freedesktop.Platform/x86_64/25.08` answers `error: … not installed`. Its
25.08 *extensions* are installed, and the Sdk, `rust-stable` and
`org.flatpak.Builder` are not. 25.08 is still the right branch — it is what those
extensions and this box's KDE Platform are built on — but Milestone 5's first run
downloads all four, hence `--install-deps-from=flathub`.

**Inside a Flatpak sandbox, 2026-09-09.** Observed with an installed app's
sandbox (`flatpak run --command=sh com.usebottles.bottles`, Flatpak 1.18.2):
`XDG_RUNTIME_DIR` is `/run/user/1000` and `FLATPAK_ID` is the app id. One
instance wrote `$XDG_RUNTIME_DIR/app/$FLATPAK_ID/wisp/probe`, a second
`flatpak run` of the same app read it back, and the host sees the same file at
`/run/user/1000/app/com.usebottles.bottles/wisp/probe` — so the socket rule in
§4, Workspace, holds between two instances of one app. Independently confirmed by
peer review B. The same sandbox showed `/tmp/.X11-unix` as a **fresh directory
holding only the socket named by `DISPLAY` at launch** (`X0`), `XAUTHORITY` as a
fixed-path copy, and `wayland-0` present in the runtime directory — which is why
`--socket=x11` cannot follow a display that starts later.

**Musl spike.** With the feature change in §4, Static binaries, both binaries
build for musl as `static-pie linked` — `wispd` 918 KB, `wisp-hud` 2,163 KB
unstripped, measured before `wisp` existed. The glibc build and the full test
suite are unaffected (wispd 82 passed / 3 ignored, wisp-hud 31, wisp-proto 11),
and glibc release binaries link only `libgcc_s`, `libc` and the loader.
`cargo tree -p wisp-hud -e features` lists exactly `wayland-sys feature
"default"`, `wayland-backend feature "default"` and `wayland-client feature
"default"` — no `client`, no `client_system`, no `dlopen` — which is the evidence
behind the first risk row.

**The game's log directory.** The prefix is
`/mnt/Data4TB/Games/everquest/prefix`, not under `$HOME`. A `find` for
`eqlog_*.txt` under it, printing size, mtime and path, gives `138551393
2026-09-09 …/Logs/eqlog_Daggo_freeport.txt` and `87486 2026-08-12
…/Logs/eqlog_Daggo_rivervale.txt`. The brief recorded the live log at 138,538,874
bytes that evening; it grows while the game runs, which is the point of the mtime
rule. Two directories in the install are named `Logs`: the game's, and
`…/EverQuest Legends/LaunchPad.libs/Logs`, which holds no `eqlog_*.txt`.

**Repository, read 2026-09-09.** `socket_path()` exists twice, to the same rule
but not the same code: the two copies differ in their doc comment, a local
binding and where the `extern "C"` declaration sits. `DurationStore::default_path()`
already applies the `XDG_DATA_HOME` → `~/.local/share` rule the config path
mirrors, absolute-path check included. `spells_dir_from_log()` takes a file and
returns its grandparent when the parent is named `logs`, case-insensitively. The
font is compiled in by `include_bytes!`, and the layer-shell surface asks for no
keyboard. `smithay-client-toolkit` 0.21.1 declares `default = ["calloop",
"xkbcommon"]`, and `xkbcommon` pulls `dep:xkbcommon`, `bytemuck`, the
`pkg-config` build-dependency and `xkeysym/bytemuck`, so the one-line change in
§4 drops exactly the `libxkbcommon` link; `Cargo.lock` carries `xkbcommon` 0.8.0
with `smithay-client-toolkit` its only dependent, so that entry's disappearance
is the cheapest check that the change took effect. `dist/` is already ignored.
`cargo fmt --all --check` reports diffs in **15 of the tree's 18 source files**,
across all three crates, and clippy is clean; the workspace declares no
`rust-version`.
