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
| `spinips:tools/appimage_update_info.py` | 302 lines | Sole git author JDS300, added in `d16a2f1`. Imports `argparse`, `struct`, `sys`, `pathlib`, `tempfile`, `fnmatch` — stdlib only. | no |
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
