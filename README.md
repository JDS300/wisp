# Wisp

**A light over the fight.**

EverQuest log parser and in-game overlay for Linux and Steam Deck.

---

> [!NOTE]
> **Nothing is built yet.** This repository currently contains its founding
> documents only — the licence, the provenance record, and the charter that
> governs what may be written here. Code begins with Spec 1.

## What this will be

A headless parser plus a small in-game overlay, for Linux only.

| Component | Role |
|---|---|
| `wispd` | Headless parser. Tails the EverQuest text log and emits state snapshots over IPC. No UI, no display connection. |
| `wisp` | CLI and control client — configuration, diagnostics, status. |
| `wisp-hud` | Overlay renderer. Attaches to whichever display the game is on and draws the HUD. |

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
the log file EverQuest already writes, and nothing else.

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
| 1 | [The spine — ingest, IPC, overlay on screen](docs/specs/2026-09-08-spec-1-the-spine.md) | Approved · [implementation plan ready](docs/plans/2026-09-08-spec-1-the-spine.md) |
| 2 | Parser and encounter model | Not started |
| 3 | HUD surfaces and interaction | Not started |
| 4 | Packaging and distribution | Not started |

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
