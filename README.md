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
a 1.44-million-line fixture log; **live verification that a mez timer
actually counts down to zero in step with the in-game wear-off is claimed
but unverified** — see the Status table and `PROVENANCE.md`.

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

## Status

| Spec | Subject | State |
|---|---|---|
| 0 | [Clean-room charter](docs/specs/2026-09-08-clean-room-charter.md) | Approved |
| 1 | [The spine — ingest, IPC, overlay on screen](docs/specs/2026-09-08-spec-1-the-spine.md) | Implemented — verified live over EverQuest under gamescope on the desktop; handheld pending · [implementation plan](docs/plans/2026-09-08-spec-1-the-spine.md) |
| 2 | [Timers — countdown rows for spells landed on mobs](docs/specs/2026-09-08-spec-2-timers.md) | Implemented — live verification (Milestone 4) pending · [implementation plan](docs/plans/2026-09-08-spec-2-timers.md) |
| 3 | Encounter model | Not started |
| 4 | Packaging and distribution | Not started |

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
