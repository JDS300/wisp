# Wisp

**A light over the fight.**

EverQuest Legends log parser and in-game overlay for Linux and Steam Deck.

---

## What this will be

A headless parser plus a small in-game overlay, for Linux only.

| Component | Role |
|---|---|
| `wispd` | Headless parser. Tails the EverQuest Legends text log and emits state snapshots over IPC. No UI, no display connection. |
| `wisp` | CLI and control client — configuration, diagnostics, status. |
| `wisp-hud` | Overlay renderer. Attaches to whichever display the game is on and draws the HUD. |

Timers for spells landed on mobs read the client's own `spells_us.txt` and
`spells_us_str.txt` from the user's EverQuest Legends install at runtime and
are never shipped with Wisp (see Spec 2's binding rule on this). `--spells
<dir>` overrides where `wispd` looks for them; if they aren't found, timers
are disabled with a message rather than the daemon refusing to run. Learned
spell durations persist across sessions at
`$XDG_DATA_HOME/wisp/durations.json`. The automated evidence is the spell
loader's counts against the real client files and a deterministic replay of
a 1.44-million-line fixture log; **JDS300 has verified live that a mez timer
appears on landing and counts down against the coded colour thresholds
(warning at 10 s, critical at 5 s), clearing at expiry** — see the Status
table and `PROVENANCE.md` for exactly what was and was not exercised. Mez
breaking on the awaken line, kill and zone clearing, restarting the HUD
mid-fight, and the dimmed `estimated`-confidence shade are **not yet
exercised**.

The encounter panel shows your damage per second, the damage you are taking
per second, your healing per second, and ranked rows for every player's
damage and healing in the current fight — one ledger of the amounts the
log's combat lines print, nothing inferred or carried from a spell table.
The automated evidence is a deterministic replay of the same
1.44-million-line fixture log, reproducing the reference implementation's
counters exactly: 2,524 fights, with the full table in
`docs/specs/2026-09-08-spec-3-encounters.md`, §6. Group rows show only
players the log has proven are in your group, never you: your numbers are
the personal line. **On 2026-09-09, JDS300 verified the panel live over
EverQuest Legends on the desktop:** the personal line tracked his DPS
through a fight and he no longer appeared in the damage or healing rows.
A groupmate's row appearing after a membership line, the ten-second close
and thirty-second linger before the panel clears, zoning clearing it at
once, and restarting `wisp-hud` mid-fight were not specifically exercised
— see `PROVENANCE.md`. Known limits: other players' summoned pets have no
owner in the log and appear as their own rows, damage between two mobs is
not counted at all, and a groupmate who was already in the group when you
joined has no row until they do something the log prints.

The split is not stylistic. On gamescope — what a Steam Deck runs — an overlay
must be a client inside the game's own XWayland instance, on a different
display from everything else. A single-process GUI cannot do that.

## How it draws over the game

By asking the compositor, not by touching the game.

- **On gamescope (Steam Deck, ROG Ally, Legion Go, or any `gamescope` launch):**
  an ordinary X11 window inside gamescope's XWayland, marked with
  `GAMESCOPE_EXTERNAL_OVERLAY` and `GAMESCOPE_NO_FOCUS`. This is the same
  mechanism `mangoapp` uses, and gamescope's own documentation recommends it
  over drawing inside the game.
- **On desktop Wayland (KDE, Sway, Hyprland, river):** a `wlr-layer-shell`
  surface on the overlay layer.
- **On GNOME:** no layer-shell exists, so an ordinary always-on-top window.
  This cannot reliably composite above a fullscreen game -- an ordinary
  window has no way to force itself above exclusive fullscreen content, only
  above other ordinary windows. GNOME users should run the game windowed or
  borderless.

**No injection, no `LD_PRELOAD`, no Vulkan layer, no reading game memory.**
Wisp is a sibling window that the compositor is asked to put on top. It reads
the log file EverQuest Legends already writes, and nothing else.

**The HUD never takes input** — not focusable, not clickable, not draggable, on
any backend. EverQuest confines the pointer during right-click mouse-look, and
the usual fixes for that on Linux (winecfg fullscreen capture, or gamescope's
`--force-grab-cursor`) hold the pointer outright. Anything wanting clicks loses
to a pointer grab; something that never wants them cannot. Configuration lives
in the CLI and a config file instead.

## The HUD

The HUD is a screen-sized transparent frame with two kinds of block drawn on
it, placed where the player put them:

| Block | Draws |
|---|---|
| `meter` | Player rows with bars, `shows = damage` or `healing`, `segment = fight` or `session`. You are a row, sorted in place with the others and highlighted, never listed twice. The header carries the display type, the segment, and the fight clock. A config may hold more than one `meter` block. |
| `timers` | Every active timer, grouped under its target, each row a draining bar coloured by kind (`mez`, `slow`, `dot` with its damage type, `debuff`), with warning and critical colour states as remaining time runs down. One instance. |

Each block has an `anchor`, a pixel `offset` from it, a `width`, a row cap
(`rows`), and a `hidden` flag that draws it only as a ghost in HUD mode.

### Editing the layout in place — HUD mode

`wisp-hud` polls the X server's key state from its own connection (the same
mechanism MangoHud's toggle key uses), so it never asks for focus. Outside
HUD mode it recognises one chord, `ctrl+shift+grave` by default. The chord is
edge-triggered — holding it does not repeat.

Inside HUD mode, every block gets a dashed outline and a name tag, the
selected one a solid outline and a halo, and a help strip runs along the
bottom edge:

| Key | Action |
|---|---|
| `↑ ↓ ← →` | move the selected block by 4 px; with Shift, 24 px |
| `Tab` / `Shift+Tab` | select the next / previous block |
| `[` / `]` | a meter's `shows`: damage ↔ healing |
| `F` | a meter's `segment`: fight ↔ session |
| `+` / `-` | `rows` up or down by one |
| `H` | toggle `hidden` |
| `Esc`, or the chord again | save and exit |

`Esc` and the chord both save the layout atomically and exit HUD mode. The
game keeps receiving every key it received before — the HUD reads keyboard
state, it never consumes an event.

### Editing the layout from a terminal — `wisp hud`

For setup beside the game, or from a script. Every verb edits the config file
and exits; a running HUD picks the change up through live reload within half
a second — nothing talks to it over a socket.

| Command | Effect |
|---|---|
| `wisp hud` | lists the blocks: index, kind, shows, segment, anchor, offset, width, rows, hidden; then `scale`, `chord` and `output` |
| `wisp hud place <n> <anchor> <x> <y>` | sets a block's anchor and offset |
| `wisp hud nudge <n> <dx> <dy>` | moves a block by a pixel delta |
| `wisp hud set <n> <key> <value>` | any block key (`shows`, `segment`, `width`, `rows`, `hidden`) |
| `wisp hud add meter\|timers` / `wisp hud remove <n>` | adds or removes an instance |
| `wisp hud scale <x>` | sets `hud.scale` |
| `wisp hud output <name>\|auto` | sets `hud.output` to a monitor `wisp doctor` lists, or removes it |

### The config file

TOML, read with the `toml` crate. The HUD polls the file's mtime every
500 ms and re-lays out on change; a file that fails to parse is reported once
on stderr and the last good layout stays up.

```toml
log = "/mnt/.../eqlog_Daggo_freeport.txt"   # or logs_dir; as in Spec 4
backend = "gamescope"

[hud]
scale = 1.0          # multiplies every size
chord = "ctrl+shift+grave"
output = "DP-1"      # layer-shell only: which wl_output; absent = compositor's choice

[[block]]
kind = "meter"
shows = "damage"     # damage | healing
segment = "fight"    # fight | session
anchor = "top-left"
offset = [20, 120]
width = 290
rows = 8

[[block]]
kind = "meter"
shows = "healing"
segment = "fight"
anchor = "top-left"
offset = [20, 400]
width = 290
rows = 4
hidden = true

[[block]]
kind = "timers"
anchor = "top-right"
offset = [20, 120]
width = 330
rows = 12
```

## Installing

Two prebuilt artifacts — the tarball and the AppImage — plus a local Flatpak
build, which compiles the workspace with cargo inside the SDK and needs the
repository checked out:

- **Tarball + `install.sh`.** Download `wisp-<version>-x86_64-linux.tar.gz`
  from a release, extract it, and run `./install.sh`. It installs the three
  binaries and the desktop entry, icon and metainfo under `~/.local` by
  default; `--prefix <dir>` installs elsewhere, and `--uninstall` removes
  exactly what was installed and nothing else.
- **AppImage.** Download `Wisp-<version>-x86_64.AppImage`, `chmod +x` it, and
  run it. It needs the kernel's FUSE interface (`/dev/fuse`), not the
  `libfuse2` package; where FUSE isn't available, `--appimage-extract-and-run`
  (or `APPIMAGE_EXTRACT_AND_RUN=1`) runs it without mounting at all.
- **Flatpak, built locally from a checkout.** `packaging/flatpak/build.sh`
  builds and installs it with `flatpak-builder`. The first run downloads
  `org.flatpak.Builder` itself plus the `org.freedesktop.Platform` and `Sdk`
  25.08 and the `rust-stable` extension — four downloads, since none of them
  is installed on a fresh machine.

First run, whichever artifact you used:

```
wisp config set logs_dir <the game's Logs directory>
wisp run
```

With the AppImage, `wisp` isn't on `PATH` — substitute its own path for
`wisp` in both commands: `./Wisp-<version>-x86_64.AppImage config set logs_dir …`
then `./Wisp-<version>-x86_64.AppImage run`.

`wisp run -- %command%` works as a Steam or Lutris launch option, the same
convention `mangohud %command%` uses. If it doesn't work, run `wisp doctor`
— it reports the config path, which log it resolved and how, whether the
client's spell files were found, and which overlay backend it would choose
and why.

**The AppImage, not the Flatpak, is the artifact for wrapping a gamescope
launch on the desktop.** A Flatpak's `--socket=x11` binds in only the X11
socket named by `DISPLAY` at launch, so it can never follow an XWayland that
starts later — such as gamescope's, when the game launches after the
Flatpak does. On a handheld Game Mode session `DISPLAY` is already
gamescope's from the start, so the Flatpak is natural there instead.

## Status

| Spec | Subject | State |
|---|---|---|
| 0 | [Clean-room charter](docs/specs/2026-09-08-clean-room-charter.md) | Approved |
| 1 | [The spine — ingest, IPC, overlay on screen](docs/specs/2026-09-08-spec-1-the-spine.md) | Implemented — verified live over EverQuest under gamescope on the desktop; handheld pending · [implementation plan](docs/plans/2026-09-08-spec-1-the-spine.md) |
| 2 | [Timers — countdown rows for spells landed on mobs](docs/specs/2026-09-08-spec-2-timers.md) | Implemented — verified live over EverQuest on the desktop (mez timer: row on landing, warning at 10 s, critical at 5 s, cleared at expiry) · [implementation plan](docs/plans/2026-09-08-spec-2-timers.md) |
| 3 | [Encounters — DPS, damage taken, healing, group rows](docs/specs/2026-09-08-spec-3-encounters.md) | Implemented — verified live over EverQuest on the desktop (personal line tracking DPS, group rows without you); close, linger, zoning and HUD restart not specifically exercised · [implementation plan](docs/plans/2026-09-08-spec-3-encounters.md) |
| 4 | [Packaging and distribution](docs/specs/2026-09-09-spec-4-packaging.md) | Implemented — build, test, clippy, the musl static build, `packaging/release.sh` and the local Flatpak build verified locally; the first CI run on a pushed tag, the Flatpak's live checks against the game, and Milestone 6 (live over EverQuest Legends on the desktop) pending JDS300 · [implementation plan](docs/plans/2026-09-09-spec-4-packaging.md) |
| 5 | [The HUD — Console look, keyboard layout mode, TOML config](docs/specs/2026-09-10-spec-5-the-hud.md) | Implemented, automated gates green — Milestone 4 (HUD mode over the running game) and Milestone 7 (live verification) pending JDS300 · [implementation plan](docs/plans/2026-09-10-spec-5-the-hud.md) |

Spec 1's overlay backends and log parser are implemented and covered by
automated tests. **On 2026-09-08, JDS300 verified both the layer-shell and
gamescope X11 backends live over EverQuest Legends on the desktop:** each
ran the kill counter live above his gamescope session with
`--force-grab-cursor`, mouse-look unaffected. **Plain-window click-through
and handheld (Steam Deck / Legion Go S) support remain claimed but
unverified** — see `PROVENANCE.md` for exactly what was and was not
verified, and how. Launching `wisp-hud` from the desktop with no flags
selects layer-shell; to use the gamescope backend instead, find gamescope's
own XWayland with `pgrep -a Xwayland` and launch `wisp-hud` with `DISPLAY`
set to that number.

Spec 3's group rows are limited to names the log has proven are in your
group; you are never one of them, since your own numbers are the personal
line above them.

## Provenance

Wisp is an independent project. It is **not** a fork, and it carries no code
from any other EverQuest tool. [`PROVENANCE.md`](PROVENANCE.md) records exactly
what moved into this repository, from where, and on what basis — written as the
work happens rather than reconstructed afterwards.

## Licence

[MIT](LICENSE). Use it for anything.

---

*EverQuest is a trademark of Daybreak Game Company. Wisp is an independent
community project, unaffiliated with and unendorsed by Daybreak.*
