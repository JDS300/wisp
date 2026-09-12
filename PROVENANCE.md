# Provenance

A contemporaneous record of what entered this repository, from where, and on
what basis. Maintained under the rules in
[Spec 0 — clean-room charter](docs/specs/2026-09-08-clean-room-charter.md).

**Written as the work happens.** Assembled afterwards, a document like this is
an argument. Kept alongside the work, it is a record. Update it in the same
commit as the change it describes, not later.

---

## Current state

**As of 2026-09-08: no code has been ported. Nothing has been written.** This
repository contains its licence, this file, and the charter. The tables below
declare what is *permitted* to move and on what terms; the Ported column is the
truth about what actually has.

---

## Why this project exists separately

`itsspin/spinips` carries no licence. Verified 2026-09-08:
`git ls-files | grep -i licen` returns nothing across its entire merged
history, and no README or documentation mentions one. Under default copyright
that grants no rights, so a fork lives on GitHub's Terms of Service alone —
which covers forking and viewing within GitHub and nothing further.

Upstream was asked to support Linux and did not reply. Wisp is the independent
alternative: same author's own work, none of anyone else's.

---

## Tier A — permitted to move verbatim

Sole git author JDS300, stdlib-only or pure data, no upstream imports.

| Source file | Size | Basis | Ported |
|---|---|---|---|
| `spinips:tools/appimage_update_info.py` | 302 lines | Sole git author JDS300, added in `d16a2f1`. Imports `argparse`, `struct`, `sys`, `pathlib`, `tempfile`, `fnmatch` — stdlib only. `appimagetool -u` writes the update-information string itself, so there is nothing to port. | no — declined 2026-09-09 |
| `spinips:tools/scrape_debuff_spells.py` | 111 lines | Sole git author JDS300. Imports `json`, `re`, `subprocess`, `sys`, `time`, `pathlib`, `html` — stdlib only. | no |
| `spinips:loremaster/tests/fixtures/debuff_spell_reference.json` | 87 entries, 19 KB | Data, not code. Scraped published facts plus durations measured from JDS300's own game logs. | no |

## Tier B — permitted to move after severance

Sole git author JDS300, carrying exactly one upstream coupling.

| Source file | Size | Coupling | Ported |
|---|---|---|---|
| `spinips:loremaster/debuff_timer.py` | 628 lines, 6 commits | `from mez_timer import (...)` — five symbols, see below | no |
| `spinips:tools/diagnose_debuff_timers.py` | 258 lines, 2 commits | Imports `debuff_timer` only; clean once the above is | no |

**Sever-then-import.** The replacement happens before the file lands here, so
no commit in this repository ever contains an upstream reference. There is no
window in which the history shows a dependency that was later removed.

## Tier C — never moves

- Everything else under `spinips:loremaster/`, including `loremaster.py`,
  `mez_timer.py`, `lull_timer.py`, `desktop_worker.py`, `engine_protocol.py`
- All of `spinui_reloaded/`, `spinui_glass/`, `layouts/`, `installer/`
- The entire `loremaster-desktop/` Electron application, its CSS and themes
- `linux_capture.py`, `hover_ocr.py` — excluded by irrelevance, not
  contamination: Wisp performs no screen capture and no OCR
- **The names:** Loremaster, Rune Seed, SpinUI, Lore Lens, Vellum & Ember,
  Midnight Frost Glass, Adventurer's Chronicle, Spin's Loremaster

---

## Authorship of the porting candidates

All five Tier A and Tier B files were written by JDS300 **with AI assistance
throughout**. Git names JDS300 as sole author, but all 14 commits touching them
carry a `Co-Authored-By: Claude` trailer.

Recorded here rather than left to be discovered. It does not affect tier
assignment — what matters is that these files contain none of upstream's
expression, and machine-written code is independently generated rather than
copied. It does mean the copyright claim over them is thinner than over
hand-written code, which under MIT is close to immaterial.

---

## Severance record

The five symbols `debuff_timer.py` imports from upstream's `mez_timer.py`.

**Corrected 2026-09-08** — see the log. An earlier revision deleted two of these
believing Wisp targeted Project Quarm. It targets EverQuest Legends.

| Symbol | Upstream | Replacement | Done |
|---|---|---|---|
| `DEFAULT_WARNING_SECONDS` | `10.0` | Wisp's own UI threshold. A presentation choice, not a derived value. | no |
| `DEFAULT_CRITICAL_SECONDS` | `5.0` | As above. | no |
| `SERVER_TICK_SECONDS` | `6` | EverQuest's server tick. A fact about the game; restated, not copied. | no |
| `scaled_duration_ticks(base, rank)` | `(base*(10+rank)+5)//10` | **Rewritten** from the published 10%-per-rank rule. | no |
| `split_spell_rank(name)` | parses `Rk. II` | **Rewritten, differently.** EQL ranks with a bare trailing roman numeral (`Dazzle V`), not `Rk. II` — a Live convention absent from 1.44M lines of EQL log. Derived from the log. | no |

---

## Sources

### Standing — always permitted, no per-use record needed

- **EverQuest's own log output.** The primary source. Preferred over any
  second-hand description of behaviour.
- **The EQL wiki** — published spell data.
- **JDS300's own captured EverQuest Legends logs** — the authoritative source
  for what EQL actually prints, as distinct from EverQuest Live, which most
  published spell data describes. Reference fixture:
  `eqlog_Daggo_freeport.txt`, 117 MB / 1,440,036 lines.
  **Project Quarm logs are not used.** Quarm is a different client on a
  different codebase; its output does not generalise to EQL, and conflating the
  two is exactly how the rank format was recorded wrongly on day one.
- **gamescope, MangoHud and mangoapp source** — BSD-2-Clause and MIT. `mangoapp`
  is the reference implementation for the overlay mechanism. Terms recorded in
  `THIRD_PARTY.md` when anything is vendored.

### Permitted with a record

- **`itsspin/spinips` and `JDS300/spinips`** — *behaviour only*. What it does in
  a given situation may inform a design decision. Its expression — code,
  structure, comments, naming, file organisation — may not be copied. Each
  consultation that informs a decision gets a dated entry below.

### Not consulted

- **Upstream's `loremaster/` source beyond behaviour.**
- **EQ Companion's source**, or any other third-party parser's source. Their
  *products* may be studied — features, what they surface, UX conventions.
  Reading their source would recreate the problem this project exists to
  escape, against a second party.

### Disclosed: EQBuddy

[EQBuddy](https://github.com/DranakCorps-bot/EQBuddy) is a third-party
EverQuest Legends session tracker by David Edwards, **MIT licensed**, with a
cross-platform `EQBuddy.Core` and a Linux build shipping today. JDS300 has
**62 merged commits** in it (August 2026), several on debuff durations and
spell ranks — the same problem domain as Wisp.

This is disclosed rather than omitted, because the author's familiarity with it
is real and undisclosed familiarity is what makes a provenance record worthless.

**Wisp deliberately does not draw from EQBuddy**, and is written independently
of it.

Note that this is a *choice, not an obligation*. Unlike `itsspin/spinips`,
EQBuddy is permissively licensed: its code — including JDS300's own
contributions, which are MIT-licensed to that project — could lawfully be
reused here with attribution. Wisp declines to, having chosen independence.

**If that ever changes**, it is legitimate and easy: add a `NOTICE` entry
crediting EQBuddy and David Edwards under MIT, record the decision in the log
below, and mark the affected files. There is no barrier, only a decision that
has not been taken.

---

## Log

Dated entries. Add one whenever a decision is informed by consulting something,
or whenever a file moves.

### 2026-09-08 — repository founded

- Verified upstream carries no licence (`git ls-files | grep -i licen`, empty
  across full merged history).
- Established the tier model, severance record and consultation rules in
  Spec 0.
- **Overlay feasibility spike.** Established that a third-party X11 client
  inside gamescope's XWayland accepts `GAMESCOPE_EXTERNAL_OVERLAY` and
  `GAMESCOPE_NO_FOCUS`, verified by readback from the X server. Sources
  consulted: `gamescope` 3.16.25 binary (atom enumeration and `--help`),
  `mangoapp` binary (`ldd` and symbol inspection), Flathub application-ID
  documentation. No upstream source involved.
- Read `spinips:docs/LINUX.md` §"A note on gamescope" for its claim that
  overlays cannot composite inside gamescope. **Behaviour only, and the claim
  was refuted** — it holds for host-session windows, not for clients inside
  gamescope's own XWayland.
- Inspected `spinips` file sizes, import lines and `git log` authorship to
  build the tier tables above. Metadata and import statements only; no logic
  read or carried.

### 2026-09-08 — target client corrected to EverQuest Legends

The first revision of this file and of Spec 0 assumed Wisp targeted **Project
Quarm**, and deleted two severance symbols on the reasoning that Quarm has no
ranked spells. **That was wrong. Wisp targets EverQuest Legends** — a different
client on a different codebase. Corrected by JDS300 the same day.

Established from the reference fixture (`eqlog_Daggo_freeport.txt`, EQL,
1,440,036 lines), a primary source:

- The session boundary line on EQL is `Welcome to EverQuest Legends!`
- Timestamps are `[Sat Nov 25 10:28:35 2023]`, naive local wall clock, no zone
- **EQL ranks spells with a bare trailing roman numeral** — observed:
  `You begin casting Dazzle V`, `Pacify V`, `Swift Like the Wind III`
- **`Rk. II` never appears.** Zero occurrences in 1.44M lines. It is an
  EverQuest *Live* convention, and upstream's `split_spell_rank` parses it.

Consequence: both deleted symbols return as rewrites, and one of them is
rewritten to a *different specification* than upstream's, derived from the log
rather than from upstream's code. Recorded in the severance table above.

**Standing rule from this: never generalise between EQ clients.** Quarm, Live
and Legends print differently. A behaviour is only established for the client
whose log demonstrates it.

### 2026-09-08 — test hardware

Primary handheld target is a **Lenovo Legion Go S running SteamOS**, which is
the same `gamescope-session` and AMD graphics stack as a Steam Deck. gamescope
3.16.25 ships a display profile for it (`lenovo.legiongos.lcd.lua`) alongside
`valve.steamdeck.lcd.lua`, so it is a first-class target rather than a proxy.

This closes the spike's open item as testable: the gamescope overlay backend can
be verified with real pixels on real hardware, not inferred.
- **EQBuddy assessed and declined as a dependency.** Established that it is MIT
  licensed, that `EQBuddy.Core` is UI-free `net10.0`, that a `linux-x64` build
  ships in v1.99.18, and that JDS300 has 62 merged commits in it. Inspected
  project metadata, `README.md`, `LICENSE`, `NOTICE` and `.csproj` files;
  no implementation source read. Building Wisp on `EQBuddy.Core` was considered
  and explicitly rejected in favour of independence. See the disclosure above.

### 2026-09-08 — gamescope overlay backend: stand-in verification

Task 6 (Milestone 2) implemented the gamescope X11 overlay backend
(`crates/wisp-hud/src/backend/gamescope_x11.rs`): an ordinary, unprivileged
X11 client window inside gamescope's XWayland, marked with
`GAMESCOPE_EXTERNAL_OVERLAY=1` and `GAMESCOPE_NO_FOCUS=1`, with an empty
XFixes input-shape region set belt-and-braces.

What was actually verified, against a real X server, with `glxgears` standing
in for EverQuest (per the constraints, `vkcube` is a poor stand-in and
launching EverQuest or Lutris, or synthesising input via `xdotool`/`XTEST`,
is off-limits on this machine):

- `wispd --stub` running, `gamescope -W 2560 -H 1440 -w 2560 -h 1440 -b --
  glxgears` launched cleanly on the desktop's NVIDIA 610.57.04 driver with
  those flags (no `--force-grab-cursor`, matching the brief's Step 6, not the
  full test-rig invocation).
- Gamescope's XWayland was `:1` (17 `GAMESCOPE_*` root properties; `:2` and
  `:3` had none).
- `wisp-hud` launched with `DISPLAY=:1`, chose the `GamescopeX11` backend
  automatically (via `root_atom_names()`), and logged that it selected a
  depth-32 ARGB visual (gamescope's XWayland offers one, so the depth-24
  BGRX fallback path did not run this time).
- `xwininfo -root -children` on `:1` found the HUD's window at
  `0x600000`, geometry `400x80+0+0` alongside `glxgears`'s window and
  `steamcompmgr`.
- `xwininfo -id 0x600000 -stats` confirmed `Depth: 32`, `Visual Class:
  TrueColor`, `Override Redirect State: yes`.
- `xprop -id 0x600000` read back both atoms:
  ```
  GAMESCOPE_NO_FOCUS(CARDINAL) = 1
  GAMESCOPE_EXTERNAL_OVERLAY(CARDINAL) = 1
  ```
- `wisp-hud` ran for its full window without an X protocol error (no BadMatch
  from the ARGB window/colormap setup, no failed `put_image` on repeated
  240ms-interval snapshots from the stub daemon).
- All processes (`wisp-hud`, `gamescope`, `glxgears`, `wispd`) were killed
  afterward and `pgrep` confirmed none remained; the socket file was removed.

**Not verified, and not attempted:** whether `GAMESCOPE_NO_FOCUS` or the empty
input region actually deliver click-through against a real pointer grab, and
whether the HUD is visible and non-interfering over the real game. Both
require a human playing EverQuest under gamescope with
`--force-grab-cursor` and confirming mouse-look behaves identically with the
HUD running and not running —**pending JDS300**. The spec's risk table
(`docs/specs/2026-09-08-spec-1-the-spine.md`) is left unchanged:
`GAMESCOPE_NO_FOCUS` stays recorded as unproven until that human check
happens.

### 2026-09-08 — layer-shell overlay backend: stand-in verification

Task 9 (Milestone 2) implemented the `zwlr_layer_shell_v1` overlay backend
(`crates/wisp-hud/src/backend/layer_shell.rs`), for desktop Wayland sessions
without gamescope: KDE, Sway, Hyprland, river.

What was actually verified, against the development machine's real KDE
Plasma / KWin session (`WAYLAND_DISPLAY=wayland-0`, `zwlr_layer_shell_v1`
version 5), with `wispd --stub` standing in for a real log feed (launching
EverQuest or Lutris, and synthesising input via `xdotool`/`XTEST`, are
off-limits on this machine):

- With no daemon running, `wisp-hud --backend layer-shell` attached
  successfully — past the compositor's first `configure`, since `attach()`
  blocks on that — and only the subsequent connect to the (absent) wispd
  socket failed. No panics, no Wayland protocol errors on stderr.
- With `wispd --stub` running, `wisp-hud --backend layer-shell` ran for a
  full 10-second `timeout` window with no further stderr output: `attach()`
  succeeded, the daemon connection succeeded, and `present()` ran repeatedly
  (once per stub snapshot) without incident.
- A reviewer finding during Task 9 itself — `present()` never actually read
  the Wayland socket, so `wl_buffer.release` events went unread and the
  `SlotPool` grew without bound — was fixed by replacing `flush()` +
  `dispatch_pending()` with `event_queue.roundtrip(state)`. Proved bounded
  with a temporary frame counter: `pool.len()` held flat at 256000 bytes
  (two 400x80 Argb8888 buffers) across a 55-second, ~220-frame run, instead
  of doubling roughly every frame as it did before the fix.
- All processes were killed afterward and the socket file removed.

**Not verified, and not attempted:** the actual on-screen appearance over a
fullscreen game, and click-through/no-focus behaviour during real play. Both
require a human playing EverQuest through the **no-gamescope** Lutris
configuration under a real desktop compositor — **pending JDS300**.

### 2026-09-08 — plain-window fallback backend: readback verification, and a decoration defect found and fixed

Task 10 (Milestone 2) implemented the plain-window fallback backend
(`crates/wisp-hud/src/backend/plain_window.rs`), for sessions with neither
gamescope nor layer-shell (notably GNOME), sharing the X11 plumbing with the
gamescope backend via the new `x11_common.rs`.

What was verified at the time, by `xprop`/`xwininfo` readback against a real
KDE Plasma / KWin XWayland session (`DISPLAY=:0`) with `wispd --stub`:
`_NET_WM_STATE` carried `_NET_WM_STATE_ABOVE`; `WM_HINTS` showed "Client
accepts input or input focus: False"; `_NET_WM_WINDOW_TYPE` was
`_NET_WM_WINDOW_TYPE_UTILITY` as set; `Override Redirect State: no` confirmed
the window was WM-managed, unlike the gamescope backend. Click-through itself
was recorded as pending JDS300, since it requires a human click.

**What that verification missed, found by the whole-branch final review:**
the same readback also showed `_NET_FRAME_EXTENTS(CARDINAL) = 0, 0, 28, 0`
and `_NET_WM_ALLOWED_ACTIONS` including `_NET_WM_ACTION_MOVE`, `_RESIZE` and
`_CLOSE` — KWin was decorating the `_NET_WM_WINDOW_TYPE_UTILITY` window with
a titlebar and frame, and that frame took pointer input the empty XFixes
input region on the client window could never reach. A direct violation of
the spec's "not focusable, not clickable, not draggable, not resizable by
pointer" invariant, missed at Task 10 time because nobody had read
`_NET_FRAME_EXTENTS` or `_NET_WM_ALLOWED_ACTIONS` specifically — only
`_NET_WM_STATE` and `WM_HINTS` were checked.

**Fixed in the final-review wave (F1):** switched to
`_NET_WM_WINDOW_TYPE_DOCK` (undecorated and kept-above by EWMH definition),
added `_MOTIF_WM_HINTS` (`decorations = 0`) belt-and-braces for window
managers that decorate DOCK anyway, and added `_NET_WM_STATE_SKIP_TASKBAR`
and `_NET_WM_STATE_SKIP_PAGER` alongside `_NET_WM_STATE_ABOVE`. Re-verified
live on the same `DISPLAY=:0` KWin session with `wispd --stub`:
`_NET_FRAME_EXTENTS` is now absent, `_NET_WM_ALLOWED_ACTIONS` contains only
`_NET_WM_ACTION_CHANGE_DESKTOP` (no `MOVE`/`RESIZE`/`CLOSE`), and
`_NET_WM_WINDOW_TYPE` is `_NET_WM_WINDOW_TYPE_DOCK`. Absolute and relative
window coordinates matched (no reparenting frame offset).

**Still not verified, and not attempted:** actual click-through against a
real pointer grab — requires a human, as before — **pending JDS300**.

### 2026-09-08 — layer-shell backend: verified live over EverQuest Legends by JDS300

JDS300 played EverQuest Legends on the desktop test rig (KDE Plasma on
Wayland, NVIDIA, three displays; EverQuest Legends launched through Lutris
with `gamescope`, `--force-grab-cursor`, borderless 2560x1440 — see
`docs/plans/2026-09-08-spec-1-the-spine.md`, "Global Constraints — The test
rig") with `wispd` and `wisp-hud` both running.

The exact launch: `wispd --log <live log>` and `wisp-hud`, from a normal
desktop terminal, with **no `--backend` flag and no `DISPLAY` override**.
Automatic backend selection ran against `DISPLAY=:0`, which is KDE's own
XWayland, not gamescope's — it carries no `GAMESCOPE_*` root properties, so
selection fell through to the layer-shell check, and KWin advertises
`zwlr_layer_shell_v1`. JDS300 confirmed the backend by reading `wisp-hud`'s
stderr: it printed **`WlrLayerShell`**.

In his words: "Loaded up everything, the kill counter is resting on top of
everquest. No mouse issues and I confirmed its reading the log by killing
something." He did not set anything with gamescope himself; only `wispd`
with the log path and `wisp-hud` ran.

What was observed: the layer-shell overlay surface drew the live kill
counter above the gamescope window hosting the game; mouse-look was
unaffected while playing under `--force-grab-cursor`; the counter
incremented in response to an in-game kill. No screen capture was attached.

**What this proves:** the §3 invariant ("the HUD never takes input") holds
for the `WlrLayerShell` backend against a real pointer grab, on this rig;
the full live path — log file → `wispd` → socket → `wisp-hud` — works
end-to-end against the real game, not `wispd --stub`; automatic backend
selection chooses `WlrLayerShell` on this rig when `wisp-hud` is launched
from the desktop (`DISPLAY` pointed at KDE's XWayland, not gamescope's).

**Not verified:**

- The `GamescopeX11` backend in-game. It was not the backend that ran here —
  automatic selection chose `WlrLayerShell` because `wisp-hud` inherited
  `DISPLAY=:0`, KDE's XWayland. To exercise `GamescopeX11`, `wisp-hud` must
  be launched with `DISPLAY` pointed at gamescope's own XWayland display
  (e.g. `DISPLAY=:1`).
- `GAMESCOPE_NO_FOCUS` click-through — still unproven; the spec's risk table
  is unchanged by this entry.
- The spec's Milestone 4 as literally written ("launched without
  gamescope") — the game was running under gamescope, per the test rig, not
  without it.
- Plain-window click-through.
- The Legion Go S / handheld target.

### 2026-09-08 — gamescope overlay backend: verified in-game by JDS300; Milestone 2 closed

Following the layer-shell entry above, JDS300 exercised the `GamescopeX11`
backend itself, on the same desktop test rig (see
`docs/plans/2026-09-08-spec-1-the-spine.md`, "Global Constraints — The test
rig"), with EverQuest Legends already running under his normal gamescope
session and `wispd --log <live log>` already running.

`pgrep -a gamescope` showed the session in flight:

```
gamescope -w 2560 -h 1440 -W 2560 -H 1440 -b --force-grab-cursor -- gamemoderun /usr/bin/umu-run …/EverQuest Legends/LaunchPad.exe
```

He found gamescope's own XWayland by process, not by guessing a display
number: `pgrep -a Xwayland` showed `Xwayland :1 -rootless -core -terminate
…`, and `env DISPLAY=:1 xprop -root` confirmed it as gamescope's, carrying
`GAMESCOPE_INPUT_COUNTER`, `GAMESCOPE_HDR_OUTPUT_FEEDBACK`,
`GAMESCOPE_DISPLAY_IS_EXTERNAL`, `GAMESCOPE_VRR_ENABLED`, among others.

He then ran, from `target/release`:

```
env DISPLAY=:1 ./wisp-hud
```

stderr, verbatim:

```
wisp-hud: backend GamescopeX11, scale 48px
wisp-hud: x11 backend: using a depth-32 ARGB visual
wisp-hud: connected to /run/user/1000/wisp/wispd.sock
```

In his words: "seems to overlay just fine while in game. Killed 2 things and
it updated." Asked whether mouse-look (right-click look, camera turning) was
identical to playing without the HUD, he answered: "Yes, identical."

**What this proves:** Spec 1 Milestone 2 as written ("a number over
EverQuest, in JDS300's normal gamescope session") — done. The §3 invariant
holds for the `GamescopeX11` backend against gamescope's
`--force-grab-cursor` pointer grab. `GAMESCOPE_NO_FOCUS` together with the
empty XFixes input region delivers click-through on this rig — the spec's
§7 first risk row. The two measures were set together, so this evidence does
not separate which one is doing the work. The depth-32 ARGB visual path is
the one that actually runs on this rig, not the depth-24 BGRX fallback.
Automatic backend selection picks `GamescopeX11` when `DISPLAY` points at
gamescope's own XWayland.

**Not verified:** no screen capture or photograph was attached to this run.
Plain-window click-through remains as recorded above. The Legion Go S /
handheld target (Milestone 6) is the same code on different hardware and
remains open.

**A practical finding worth recording:** on this rig, listing
`/tmp/.X11-unix/` through a fish loop gave misleading results because the
user's `ls` alias prints file-type icons, which corrupted the parsed
display numbers as zeros. `pgrep -a Xwayland` is the reliable way to find
gamescope's display number, and `env DISPLAY=<n> xprop -root` confirms it by
its `GAMESCOPE_*` root properties before anything is launched against it.

### 2026-09-08 — Spec 2 design: sources consulted

Spec 2 (timers) was designed from primary sources, with two recorded
consultations:

- **JDS300's own `loremaster/debuff_timer.py` (Tier B) and
  `debuff_spell_reference.json` (Tier A)** — behaviour and data only. Two
  behaviours carried into the spec: a prose landing is accepted only while a
  compatible local cast is pending, because slow/resist prose names the target
  and not the spell and prints for other casters too; and a DoT still ticking
  past its computed expiry is held open. The scraped duration table was read
  to compare against the client data and is **not** used by Wisp: the client's
  own `spells_us.txt` supersedes it. No code, structure or naming moved.
- **`amerzel/eql-info` (MIT)** — `SPELL_FORMAT.md` was read for the field
  layout of `spells_us.txt` / `spells_us_str.txt` and recorded in
  `THIRD_PARTY.md`. Its `spells.json` was fetched once to confirm the
  `Mesmerization` row (id 307, cap 4) matched the local file, then discarded.
  Wisp does not use that project's data, parser or site.
- **Upstream `mez_timer.py` (Tier C)** was not consulted. Mez behaviour comes
  from the fixture: ` has been mesmerized.`, ` has been awakened by `, and the
  measured 37–41 s natural expiry for Mesmerization VI.

Primary sources: the reference fixture and the EverQuest Legends client files
in JDS300's install (`spells_us.txt`, 73,975 rows; `spells_us_str.txt`). The
measurements in the spec's appendix were produced by throwaway scripts over
the fixture on this date and are not kept.

### 2026-09-08 — Spec 2 final-review fix wave: appendix counts corrected, name collisions disclosed

The whole-branch review of `spec-2-timers` found two of Spec 2's own claims
wrong against its own primary sources:

- **The appendix line-count tables were measured on the live, still-growing
  log, not the frozen fixture** (`eqlog_Daggo_freeport.1440036.txt`) the spec
  names as its source. Re-measured every row on the frozen fixture with
  `grep -cF`; several counts moved (e.g. `You begin casting` 17,286 →
  17,131; DoT ticks 22,148 → 21,660; see the spec and plan appendices for
  the full corrected tables). The mesmerized/awakened/charmed/slain counts,
  already measured from the fixture, were unaffected.
- **Spell names are not unique across eligible rows of `spells_us.txt`**, contrary
  to the spec's original claim. Measured directly against the client file:
  **706** eligible names collide (appear on more than one row), 304 of those
  with differing capitalisation. The loader already resolved this correctly
  (lowest id wins) but logged nothing; it now counts and reports the total
  once at startup, and the spec text is corrected.

No new sources were consulted; both corrections came from re-measuring the
same primary sources (the frozen fixture and the local client files) already
on record above.

### 2026-09-08 — Spec 2 timers: verified live over EverQuest Legends by JDS300 (Milestone 4)

JDS300 played EverQuest Legends on the desktop test rig (gamescope, with
`--force-grab-cursor`, as in Spec 1 — see `docs/plans/2026-09-08-spec-1-the-spine.md`,
"Global Constraints — The test rig"), running `wispd --log <live log>` and
`wisp-hud` from the `spec-2-timers` branch.

In his words: "tested and the timer for mez shows up, changes color at 10, 3
and cleared." Asked when the row turned to the critical colour, he answered:
"About 5 s, as coded."

What was observed: a mez cast in play produced a row on the landing line; the
row turned to the warning colour at 10 s remaining and the critical colour at
about 5 s remaining, matching the thresholds coded in §4; the row cleared at
expiry. The HUD took no input throughout — he played normally.

**What this proves:** Spec 2 Milestone 4 as written ("a mez in play: row
appears on landing, reaches zero within 1 s of the wear-off line, disappears")
is closed on the desktop rig. The colour precedence in §4 — critical at ≤ 5 s,
else warning at ≤ 10 s — runs correctly against a live, real-time countdown,
not just the fixture replay. The full live path (log file → `wispd` → socket →
`wisp-hud`) works end-to-end for a mez timer over the real game, and the §3
invariant continues to hold while it does.

**Not verified — not specifically exercised, not failed:**

- A broken mez removed on the awaken line.
- Kill clearing a mob's rows, and zone-change clearing the list.
- Restarting `wisp-hud` mid-fight showing the same remaining time.
- The dimmed `estimated`-confidence shade, as distinct from a `measured` row's
  white — this run did not distinguish the two.
- The handheld (Steam Deck / Legion Go S) target.

**A limit recorded, JDS300's own observation:** "spell rank goes up to X (10)
at this time and I assume it will go higher but that can be a future problem
when it does." The `ROMAN` table in `crates/wispd/src/rules.rs` (`split_rank`)
maps only `I` through `X`. A cast line naming a rank above `X` (`XI` and
beyond) does not match any entry in that table, `split_rank` returns `None`,
the cast never becomes a pending cast, and no timer appears for that spell —
silently, with no error logged. Accepted for now, since the client's current
maximum rank is X; recorded in the spec's risk table
(`docs/specs/2026-09-08-spec-2-timers.md`, §7) against the day the client
raises it.

### 2026-09-08 — Spec 3 design: sources consulted

Spec 3 (encounters) was designed from the frozen fixture, with two
consultations recorded here as the charter requires:

- **Upstream `loremaster/loremaster.py` (Tier C) — behaviour only.** A grep
  of its pet-handling comments and docstrings was read to learn *what it
  does*: pets announce themselves with `Attacking … Master`, summoned pets
  have one-word names while charmed creatures keep their own, a `My leader
  is` reply identifies a pet's owner, charm aliases are treated as ephemeral.
  The grep output also exposed two of its regular expressions for those
  lines. Wisp's classifier was written afterwards from the fixture's own
  lines (`A revultant rat told you, 'Attacking an abhorrent Master.'`) and
  shares no expression, naming or structure with them. Nothing else in that
  file was read.
- **EQBuddy `README.md` — product description only, no source.** Learned that
  it derives the player's pet name from Master messages, counts pet kills as
  the player's, and shows a charmed pet as provisional until a Master message
  confirms it. Wisp's 120 s pet time-to-live is its own rule, chosen from the
  fixture's announcement cadence. The standing decision not to read
  EQBuddy's source is unchanged.

Primary sources: the frozen fixture (line shapes and counts in the spec's
appendix) and a throwaway reference implementation of the spec's rules,
recorded in the plan, whose output is the acceptance table.

### 2026-09-09 — Spec 3 final-review fix wave: three missed line shapes, acceptance numbers re-derived

The whole-branch review of `spec-3-encounters` found one Critical, three
Important issues and a set of Minors. Fixing the Critical (an open fight
never closing on screen when the log went quiet, `Tracker::encounter`
checking only whether a fight was open rather than the estimated clock) and
the Minors changed no numbers. Re-deriving the reference implementation to
fix the Critical's sibling report — that the Important review findings
implicated the reference itself — surfaced **three line shapes the original
reference implementation had missed entirely or mismatched**, all found by
re-measuring against the same frozen fixture already on record above (no new
source consulted):

- **Other sources' DoT ticks** print `<target> has taken <N> damage from
  <Spell> by <source>.`, not the assumed `from <source>'s <Spell>.`. The old
  possessive split matched only when a spell name happened to carry its own
  apostrophe (a bard song title, e.g. `Selo's Chords of Cessation VII`) and
  credited the text before that apostrophe as a phantom player. Four such
  phantoms were found and removed: **Tuyen, Selo, Denon, Oathbreaker**. The
  correct shape appears 11,614 times in the fixture; the corrected split
  takes the source after the *last* ` by `, which is immune to an apostrophe
  anywhere in the spell name.
- **A damage shield can land on you**: `YOU are <verb> by <source>'s <thing>
  for <N> points of non-melee damage!` — note `YOU are` (not the lower-case
  `You`) and the line ends in `!`, not `.`, which is why an earlier grep for
  a period-terminated `You are` line found nothing and the plan's Task 3
  self-review recorded this as an impossible case. It appears 19,077 times
  in the fixture and is now a `taken_shield` stat.
- **A special attack can carry an `on` preposition before `YOU`**:
  `<mob> <verb>s on YOU for <N> points of damage.` (245 times, e.g. `An icy
  terror frenzies on YOU for 13 points of damage.`). The word directly
  before `YOU` is `on`, not the verb, so the existing `<verb>s YOU` shape
  missed it and it fell through to the general other-source melee shape
  instead, crediting `An icy terror` with 13 points of *damage dealt* under
  a garbage target name (`"on YOU"`) rather than counting it as damage
  *taken*. Stripping a trailing ` on` after stripping ` YOU` recovers the
  correct shape.

**The §6 acceptance numbers moved accordingly** (2,524 fights, was 2,544;
full corrected table in the spec and plan): damage taken by you across all
four categories (melee/spell/dot/shield) rose from 2,900,237 to 3,164,597
(+9.1%, entirely the new `taken_shield` category, since melee/spell/dot each
moved by at most a few thousand); other-sourced DoT damage counted in fights
rose from 141,014 (only the apostrophe-accident subset) to 330,056 as the
four phantom players were replaced by their real sources. The reference
script (Appendix A of the plan) was re-derived and re-run against the same
frozen fixture on 2026-09-09, deterministic across two runs; its output is
recorded there verbatim, superseding the 2026-09-08 output.

No new source was consulted for this correction: it is entirely a
re-measurement of the frozen fixture already on record, the same one Spec 3
was designed from. `spec-3-encounters`' final-fix brief and the reviewer's
findings drove the correction; neither reads from `spinips` or `EQBuddy`.

### 2026-09-09 — Spec 3 live-test fix: group rows limited to proven group members

JDS300 played PR #3 live and found the group meter rows wrong in two ways at
once: players outside his group appeared in the damage rows, and he appeared
in them twice — once as a damage row, once as a healing row — on top of his
own personal line. Ruling: a group row shows a name only once the log has
proven it belongs to the player's group; the player is never a row, since
the personal line already carries the player's own numbers; and when the log
has proven nobody, there are no group rows at all.

The log has no roster line — it never lists who was already in the group
before the player joined it, only who does something groupy afterward
(joins, is invited, is thanked for joining, becomes leader or Main Assist, or
talks on group chat) or who leaves or is removed. Membership is tracked as a
set, learned and forgotten from the log's own lines, and reset to empty on
`You have joined/left/been removed from the group.` or `Your group has been
disbanded.` — the reference script (re-derived against the same frozen
fixture already on record, no new source consulted) counted, over the whole
fixture: 40 lines proving membership, 4 leaves, 7 resets, and the group empty
again by end of file. These three counters are now part of the §6 acceptance
table and the replay test, alongside every number already there, which did
not move.

No new source was consulted: this is a rule derived from the fixture's own
group-chat and membership lines, the same fixture Spec 3 was designed from.

### 2026-09-09 — Spec 3 encounters: verified live over EverQuest Legends by JDS300 (Milestone 4)

JDS300 played EverQuest Legends on the desktop test rig (gamescope, as in
Spec 1 and Spec 2), running `wispd` and `wisp-hud` from `main` after PR #3
merged with the group-rows fix (merge commit `ee01845`).

In his words: "Looks like it's tracking DPS and the 'you' line has been
removed for both damage and healing." Asked about the rest of the Milestone 4
list: "I think those are all good for now and we can troubleshoot
additionally later if need be."

What was observed: the personal line tracked his DPS during a live fight, and
neither the damage rows nor the healing rows carried a row for him — the two
defects his first live test found (out-of-group players ranked; himself
listed twice) are gone.

**What this proves:** Spec 3 Milestone 4's first two clauses ("personal line
moves, group rows rank" as far as the rows' composition goes) are closed on
the desktop rig, and the full live path (log file → `wispd` → socket →
`wisp-hud`) carries the encounter block end-to-end over the real game, not
only the fixture replay.

**Not verified — not specifically exercised, not failed;** JDS300 called the
run good for now, to be revisited if something shows up:

- A groupmate appearing as a ranked row after a membership line (join,
  invite, leader, Main Assist, group chat), and staying absent otherwise.
- The 10 s close and the 30 s hold before the panel clears.
- Zoning clearing the panel at once.
- Restarting `wisp-hud` mid-fight showing the same numbers.
- The handheld (Steam Deck / Legion Go S) target, deferred by standing rule.

### 2026-09-09 — Spec 4 design and packaging: sources consulted

- **MangoHud / `mangohud %command%`** — product convention only (a launch
  option that wraps a command), and MIT source is permitted by the charter
  anyway. No source read for this.
- **AppImage documentation** — `appimagetool`'s README at tag `1.9.1` for
  `-u`, `--runtime-file` and the runtime download; the type-2 runtime's
  README for the FUSE requirement. Both quoted in the Spec 4 plan's Global
  Constraints.
- **Flatpak documentation** — the sandbox's X11 socket binding and
  `$XDG_RUNTIME_DIR/app/$FLATPAK_ID`, checked by experiment inside another
  app's sandbox rather than taken on trust.
- **`flatpak/flatpak-builder-tools`** — the cargo generator at commit
  `1fc32195e3e60fe5c97f0af646dec7a99df5962b`, MIT declared in the file itself
  (`__license__ = "MIT"`); the repository has no root `LICENSE` at that
  commit, which is recorded rather than papered over. Tool used, nothing
  vendored.
- **`AppImage/type2-runtime`** — MIT, "Copyright (c) 2004-23 probonopd". Its
  `runtime-x86_64` is embedded in every AppImage this project ships, which is
  why it is recorded.
- **`zsync`** — `LicenseRef-Artistic` per its Arch package (0.6.6-1.1). Tool
  used at build time; nothing of it is in an artifact.
- Not consulted: `itsspin/spinips`, `JDS300/spinips`, `EQBuddy`, and any
  other parser's source. Spec 4 needed no behaviour from any of them.

### 2026-09-10 — Spec 5 design: sources consulted

Spec 5 (the HUD) was designed in a brainstorm with JDS300, with four visual
decisions made on browser mockups and one live spike inside gamescope. The
consultations, as the charter requires:

- **Details! Damage Meter, EqTool, EQLogParser — product only.** Guides,
  README prose and screenshots, read by research subagents told not to open
  any source file. Learned: one window per metric with bars relative to the
  top row (Details!), timers grouped under the target as draining bars
  (EqTool), and that a numeric strip with no bars is still legible in-game
  (EQLogParser). Spec 5 adopts the first two conventions. No source read.
- **EQBuddy — README and the screenshots under `docs/screenshots/`, no
  source.** Learned that it offers per-metric breakout windows switchable
  between fight and session, which Spec 5's blocks also do; noted as
  convergent, not drawn from. The standing decision not to read its source is
  unchanged.
- **The upstream desktop application (Tier C) — one preview image,
  `docs/previews/loremaster_panel.png` in `JDS300/spinips`, at product level.**
  Looked at to know what to *avoid* reproducing as much as for its timer
  treatment (state words on rows). Nothing of its layout, CSS, themes,
  palette or typography moves; its names do not appear in Spec 5, which
  refers to it only as "the upstream desktop application".
- **gamescope source (`ValveSoftware/gamescope`, shallow clone of
  2026-09-10; BSD-2-Clause, permitted freely)** — `steamcompmgr.cpp` and
  `wlserver.cpp`, read to learn how input focus is picked and handed off
  (external overlays excluded from focus; the Steam overlay path via
  `STEAM_OVERLAY` + `STEAM_INPUT_FOCUS`; focus caches nulled on surface
  destroy). Informs Spec 5 §3.1 and Appendix A. Nothing copied.
- **MangoHud `src/keybinds.h` (MIT, permitted freely)** — read to confirm
  that its in-game hotkeys poll `XQueryKeymap` on a separate display
  connection. Wisp uses the same mechanism; no code moves.

**The spike**, 2026-09-10, on JDS300's desktop over EverQuest Legends under
gamescope launched by Lutris: keyboard polling without focus proven; an
overlay taking pointer input proven; handing input back to the game **not**
proven — the game lost mouse and keyboard until relaunched, and two repair
attempts failed. Full procedure and results in Spec 5 Appendix A. The probe
was a throwaway binary in the session scratchpad and was not committed.

Primary sources: the client's `spells_us.txt` (resist type at field 29, the
effect list at field 172; facts in Spec 5 Appendix B) and the running game.

### 2026-09-11 — Spec 5 implemented

**What moved: nothing.** Spec 5's eight implementation tasks (T1–T8) wrote
`wisp-proto` v4, `wispd`'s spell-table timer classification, `wisp-config`'s
TOML layout model, `wisp-hud`'s canvas/theme/model/paint stack and HUD-mode
key polling, and the `wisp hud` CLI verbs entirely from the design in Spec 5
and its own tasks — no file from Tier A, Tier B or any other tree entered
this repository. The Tier C names remain absent, checked with `git grep`
before this commit as before every prior one.

**The fonts.** T4 vendored two more faces from the same `ttf-dejavu 2.37`
Arch package (`2.37+18+g9b5d1b2f-8`) as the already-vendored Mono, so the
canvas layer's proportional text and the existing monospace text share one
licence file:

- `assets/DejaVuSans.ttf` sha256
  `6038a160b491e121c1f12c7bccb4a9c8730296e3adc1086a059404ed84b7451c`
- `assets/DejaVuSans-Bold.ttf` sha256
  `b5d64817b6331723b5e59eaaa6db90057cbed58e9733f65687f110638192359f`

Both re-verified against the files in `assets/` for this entry. `assets/DejaVu.LICENSE` (renamed from `assets/DejaVuSansMono.LICENSE` in T4) covers
all three faces verbatim, and `THIRD_PARTY.md`'s vendored-font entry says so.

**The spike's standing rule.** Spec 5 §3.1 restated Spec 1 §3 after the
2026-09-10 gamescope input spike (Appendix A): the HUD never changes focus —
it never asks for keyboard or pointer focus, never sets a non-empty input
region, and never sets the Steam overlay atoms. `docs/specs/2026-09-08-spec-1-the-spine.md`
carries a one-line amendment pointing to this rule; §3's own text is
unchanged, since the rule it states was right all along and only the reason
given for it needed correcting.

### 2026-09-11 — Spec 6 implemented

Nothing moved. Spec 6 is `wisp stop`, a system-tray item, HUD mode taking the
keyboard on the layer shell, and two release channels; none of it touches a
parsing or counting rule, and no file entered the repository from anywhere.

**Consultation:** none under charter §5. The work read three dependencies'
own documentation and source — `ksni` 0.3.6 (docs.rs and the published
crate, for the `Tray` trait's items and the blocking spawn API),
`smithay-client-toolkit` 0.21.1 (`src/seat/mod.rs` and `src/dispatch2.rs`,
for how to bind a `wl_seat` without the `xkbcommon` feature) and
`wayland-client` 0.31.15 (`wayland.xml`, for `wl_keyboard`'s events). Reading
a library one depends on is ordinary use of that library, not consultation of
another parser's expression, and it is recorded here so the distinction is on
the record rather than assumed. The Gear Lever facts in Spec 6 §4.4 were read
from Gear Lever's own Python source (`GithubUpdater.py`,
`UpdateManagerChecker.py`) on 2026-09-11 — a fact about how an updater
resolves a string, restated, with nothing copied.

**The tray pixmaps** (`crates/wisp-hud/icons/tray-*.argb`) are rendered from
this repository's own `packaging/io.github.jds300.Wisp.svg` by
`packaging/render-tray-icon.sh`. No third-party artwork.

**A licence correction.** Spec 6 §4.2 was designed believing `ksni` was MIT.
`cargo metadata` says its `[package] license` field is `Unlicense`; the spec
text and `THIRD_PARTY.md` are both corrected, and the spec records what was
believed at design time alongside what the tooling reports.

### 2026-09-12 — the design sheet, and the README split

**One file entered the repository.** `docs/art/wisp-sheet.jpeg` is a design
sheet JDS300 generated on 2026-09-12 with Google's Gemini image model from his
own prompt, added as `docs/Gemini_Generated_Image_hixntehixntehixn.jpeg` and
moved to its present name. It is his, it is the only artwork in the project,
and **every raster Wisp ships is cut from it** by
`packaging/render-icons.sh` — the README banner, the eight launcher PNGs and
the twenty tray pixmaps. No third-party artwork, no icon theme, no stock
image. The hand-drawn `packaging/io.github.jds300.Wisp.svg` stays as the
scalable icon and is unchanged.

**This entry supersedes the 2026-09-11 entry's "The tray pixmaps"
paragraph.** That paragraph described `tray-22.argb` and `tray-48.argb` as
rendered from the SVG by `packaging/render-tray-icon.sh`; both that script
and the two files it produced are gone, replaced by the sheet above and
`packaging/render-icons.sh`'s twenty.

**Consultation:** none under charter §5. The work read one dependency's own
source — `ksni` 0.3.6 (`src/tray.rs` and `src/lib.rs`, for `Icon`'s fields and
`Tray::icon_pixmap`'s contract) — which is ordinary use of a library one
depends on, not consultation of another parser's expression, and is recorded
here so the distinction stays on the record rather than being assumed.

**The README was split, not rewritten away.** Its roadmap, its status table and
its provenance note moved to `docs/STATUS.md` byte for byte; the README is now
a page for someone installing Wisp, and the build record is one click away.
Nothing was deleted.
