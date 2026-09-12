# Wisp — build status and provenance

Moved out of `README.md` on 2026-09-12, unchanged: the roadmap, the spec
table and the provenance note that used to live there. The README is now
the page a player reads; this is the record of what has been built and
what has been verified.

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

## Status

| Spec | Subject | State |
|---|---|---|
| 0 | [Clean-room charter](specs/2026-09-08-clean-room-charter.md) | Approved |
| 1 | [The spine — ingest, IPC, overlay on screen](specs/2026-09-08-spec-1-the-spine.md) | Implemented — verified live over EverQuest under gamescope on the desktop; handheld pending · [implementation plan](plans/2026-09-08-spec-1-the-spine.md) |
| 2 | [Timers — countdown rows for spells landed on mobs](specs/2026-09-08-spec-2-timers.md) | Implemented — verified live over EverQuest on the desktop (mez timer: row on landing, warning at 10 s, critical at 5 s, cleared at expiry) · [implementation plan](plans/2026-09-08-spec-2-timers.md) |
| 3 | [Encounters — DPS, damage taken, healing, group rows](specs/2026-09-08-spec-3-encounters.md) | Implemented — verified live over EverQuest on the desktop (personal line tracking DPS, group rows without you); close, linger, zoning and HUD restart not specifically exercised · [implementation plan](plans/2026-09-08-spec-3-encounters.md) |
| 4 | [Packaging and distribution](specs/2026-09-09-spec-4-packaging.md) | Implemented — build, test, clippy, the musl static build, `packaging/release.sh` and the local Flatpak build verified locally; the first CI run on a pushed tag, the Flatpak's live checks against the game, and Milestone 6 (live over EverQuest Legends on the desktop) pending JDS300 · [implementation plan](plans/2026-09-09-spec-4-packaging.md) |
| 5 | [The HUD — Console look, keyboard layout mode, TOML config](specs/2026-09-10-spec-5-the-hud.md) | Implemented, automated gates green — Milestone 4 (HUD mode over the running game) and Milestone 7 (live verification) pending JDS300 · [implementation plan](plans/2026-09-10-spec-5-the-hud.md) |
| 6 | [Running it — `wisp stop`, the tray, release channels](specs/2026-09-11-spec-6-running-it.md) | Implemented, automated gates green — Milestone 6 (the tray live on Plasma over the game), Milestone 8 (HUD mode owning the keyboard over the game), Milestone 9 (the gamescope keyboard-grab spike) and Milestone 10 (the delivery path end to end) pending JDS300 · [implementation plan](plans/2026-09-11-spec-6-running-it.md) |
| 7 | [The face — the icon, the tray's four states, the README](specs/2026-09-12-spec-7-the-face.md) | Designed 2026-09-12; not yet implemented |

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
from any other EverQuest tool. [`PROVENANCE.md`](../PROVENANCE.md) records exactly
what moved into this repository, from where, and on what basis — written as the
work happens rather than reconstructed afterwards.
