# Spec 5 — the HUD

**Status:** designed 2026-09-10 with JDS300 (brainstorm, four visual decisions made on mockups, one live spike); pending the implementation plan
**Depends on:** [Spec 4 — packaging](2026-09-09-spec-4-packaging.md), [Spec 3 — encounters](2026-09-08-spec-3-encounters.md), [Spec 2 — timers](2026-09-08-spec-2-timers.md), [Spec 1 — the spine](2026-09-08-spec-1-the-spine.md), [Spec 0 — clean-room charter](2026-09-08-clean-room-charter.md)
**Target client:** EverQuest Legends. Not Live, not Project Quarm.

---

## 1. What this spec is for

Specs 1–4 built a parser that is right and an overlay that is huge, monospaced,
one colour per line, pinned to the top-left corner and sized to the widest thing
it might ever print. JDS300's verdict after the first live sessions: "huge, very
basic, and well, ugly." This spec replaces what `wisp-hud` draws and how the
player arranges it. It changes what the parser *reports* in exactly one place —
the kind of a timer — and nothing about what it counts.

**Done means:** over EverQuest Legends on JDS300's desktop, the HUD is two kinds
of block — a meter and a timer list — placed where the player put them with the
keyboard, in the Console look chosen on 2026-09-10 (§4.2), with timers coloured
by kind and the player's own row inside the meter; a config edit moves a block
without a restart; and the whole thing still never takes focus from the game.

---

## 2. Non-goals

Explicitly deferred, so their absence is a decision rather than an oversight:

- **Analytics and reporting** — kills, XP per hour, damage by ability, session
  history. A separate component with its own spec (Spec 6). Every tool studied
  in §Appendix C keeps this outside the overlay, and so does Wisp. The two
  parser items Spec 4 promised to Spec 5 — a cleaner mob-marking rule and the
  sourceless `X has taken N damage by <Spell>.` lines — **move to Spec 6**
  (JDS300, 2026-09-10), because that spec reopens the parser anyway.
- **The Spec 4 review's two small daemon items** — the socket time-of-check
  race and the stub daemon's spell loading — carry to Spec 6 with the parser
  work (JDS300, 2026-09-10). Neither touches what the HUD draws.
- **A session block.** The kill count leaves the overlay; it is report material.
- **A personal block.** Your DPS, HPS and fight clock live in the meter now:
  you are a row, the clock is in the header. Damage taken per second leaves the
  overlay with the rest of the report material.
- **Pointer drag in HUD mode.** The spike (Appendix A) proved an overlay can
  take the pointer inside gamescope and failed to prove it can give it back.
  Parked, with the procedure to try again recorded.
- **Hotkeys on the layer-shell backend.** Wayland gives key state only to a
  focused surface, and the HUD is never focused. Config and CLI still work
  there; HUD mode needs an X display (gamescope or plain X11).
- **Handheld verification** — `PROVENANCE.md`'s standing rule.
- Class colours (the log carries no class), spell icons (no icon data is read
  from the client), sounds, triggers, per-spell colour overrides, themes beyond
  the palette table in §4.2. A second look can be a later spec; the first one
  must be right.

---

## 3. Invariants

### 3.1 Spec 1 §3, amended

Spec 1 said the HUD never takes input, and gave the pointer grab as the reason.
The spike showed the reason was wrong and the rule was right for a different
one: inside gamescope, **input goes to one window**, and a window tagged as an
external overlay is excluded from focus outright. Clearing the no-focus atom
changes nothing. A window can get input only by becoming gamescope's focus, and
the one attempt to hand focus back left the game deaf for the rest of the
session (Appendix A).

The rule from Spec 5 on: **the HUD never changes focus.** It never asks for
keyboard or pointer focus, never sets a non-empty input region, and never sets
the Steam overlay atoms. It reads the keyboard by **polling the X server's key
state** from its own connection, which needs no focus and moves none — the
mechanism MangoHud's toggle key uses on every Steam Deck. `GAMESCOPE_NO_FOCUS`
and the empty XFixes input region stay set on every X11 surface, exactly as
today.

### 3.2 Unchanged

Everything the first four specs made binding stands: game data is read at
runtime and never shipped; naive timestamps are subtracted, never relabelled;
only lines that carry an amount count; Wisp never searches the disk. The
charter's Tier C stays closed: the upstream desktop application's layout, CSS
and themes were not consulted, and its names do not appear in this spec.

### 3.3 New

- **The config file is the layout.** Every placement, size, visibility and
  display choice is a key in the config file. HUD mode and the CLI are two
  editors of the same file; neither holds state the file does not.
- **Protocol changes are additive.** v4 adds a kind and a field; a v3 reader
  that ignores unknown fields still parses a v4 snapshot's rows.

---

## 4. Architecture

### 4.1 Blocks

A block is one rectangle drawn from one part of the snapshot. Spec 5 has two
kinds, and the config may hold more than one instance of the first:

| Block | Draws | Instances |
|---|---|---|
| `meter` | Player rows with bars: `shows = damage` or `healing`, `segment = fight` or `session`. **You are a row**, sorted in place by amount and highlighted; you appear once per block, never twice. Rows are the daemon's rows plus yours, capped by the block's `rows`. Header: display type, segment, fight clock `m:ss` | any number; the default file has one, `damage`, `fight` |
| `timers` | Every active timer, grouped under its target in the order the daemon sends them, each row a draining bar coloured by kind, capped by `rows` | one |

The daemon already sends everything the meter needs: `encounter.damage` and
`encounter.healing` carry group rows, and `encounter.you` carries your own
numbers (Spec 3 keeps you out of the rows on purpose). Your row's name is
`You`; the log never prints your character's name.

`segment = session` sums the fight rows the HUD has seen since it connected.
That is a HUD-side accumulation keyed by name; the daemon is not asked to keep
history in this spec.

### 4.2 The look — Console

Chosen on 2026-09-10 from three mockups (Console, Glass, Bare), then refined:
24 px rows with a 4 px gap over Details!'s 22 px, the pastel kind palette over a
saturated one, kind labels kept, and the three-number meter row.

All sizes are at scale 1.0, which is JDS300's 2560×1440 gamescope. `scale`
multiplies every one of them.

| Element | Value |
|---|---|
| Panel | fill `rgba(8,10,14,0.74)`, 1 px border `rgba(255,255,255,0.08)`, 3 px corner radius |
| Header strip | fill `rgba(0,0,0,0.35)` over the panel, 4 px vertical / 8 px horizontal padding, 12 px text `#c9d1d9`, left title, right clock or count |
| Row | 24 px tall, 4 px gap above, 4 px inset from the panel edge, 8 px horizontal text padding |
| Row text | 13 px `#e8edf2`; numbers 12.5 px, tabular figures; name ellipsised when it does not fit |
| Bar | the row's full height behind the text, 2 px radius, kind colour at 55 % opacity; your row 80 % |
| Target label (timers) | 11.5 px `#a8b3bf`, 7 px above the first row of its group |
| Kind label (timers) | 10.5 px uppercase at 55 % white, between the name and the time |
| Font | DejaVu Sans, embedded like DejaVu Sans Mono already is (same licence file) |

**Meter numbers**, right-aligned: total (compact: `999`, `9999`, `18.2k`,
`1.32M` as today), then rate per second in bold, then share of the block's
total as a percent. The bar is relative to the top row. Damage bars are
`#6b7a8c`, healing bars `#7cd992`, your bar `#8fe3ff` in either.

**Timer rows**: name with rank as a bare roman numeral (as today), kind label,
time. Time prints as `18s` under a minute and `1:40` at or over one. The bar is
`remaining / duration` of the row's own duration.

**Kind palette** (pastel):

| Kind | Colour | | Kind | Colour |
|---|---|---|---|---|
| mez | `#ff79c6` | | dot · poison | `#7cd992` |
| slow | `#82aaff` | | dot · disease | `#c5c86a` |
| dot · fire | `#ff8a65` | | dot · magic | `#b48cff` |
| dot · cold | `#80deea` | | dot · corruption | `#d4a373` |
| debuff | `#94a3b8` | | dot · unresistable | `#94a3b8`, labelled `dot` |

**Row states**, applied over the kind:

| State | Rule | Appearance |
|---|---|---|
| estimated | `confidence = estimated` | text at 60 %, bar at 28 %, label suffixed `· est` |
| warning | `remaining ≤ 10 s` | time `#ffcc66` |
| critical | `remaining ≤ 5 s` | time `#ff5c5c` |
| gone | `remaining < 0` (the daemon's post-expiry hold) | name struck through at 50 %, no bar, time reads `gone` in `#ff5c5c` |

`WARNING_SECS` and `CRITICAL_SECS` keep their Spec 1 values (10 and 5): Wisp's
own thresholds, unchanged.

### 4.3 Timer kinds — the one parser change

Today the daemon marks a timer `mez` when the landing prose is
` has been mesmerized.`, `dot` when a tick line arrives, and `debuff` otherwise.
Spec 5 classifies **from the spell table at load time**, verified against the
client's `spells_us.txt` on 2026-09-10 (Appendix B):

| Kind | Rule | Precedence |
|---|---|---|
| `mez` | effect list carries SPA 31, **or** the landing prose is the mez prose (both signals stay) | 1 |
| `slow` | effect list carries SPA 11 with a base value below 100 (`Slow` 90, `Tepid Deeds` 80, `Turgur's Insects` 85; the haste `Alacrity` is 122) | 2 |
| `dot` | effect list carries SPA 0 with a negative base **and** the spell has a duration (`Envenomed Bolt`; `Frost Rift` has SPA 0 but no duration, so it is a direct hit, not a timer) | 3 |
| `debuff` | any other detrimental spell | 4 |

A DoT also carries its **damage type** from field 29, the resist type:
1 magic, 2 fire, 3 cold, 4 poison, 5 disease, 9 corruption, 0 unresistable.
(`Tashani` is 0.) Curses in this client are corruption-resisted, so
"corruption" is the wire word and "curse" is not.

The effect list is the row's **last** field (index 172 in the 173-field rows
of the 2026-08-24 client; eql-info's `SPELL_FORMAT.md` documents an insertion
that moved it once already, so it is read by position from the end, never by a
hard-coded index): `$`-separated entries of
`slot|effect_id|base|limit|formula|max`. `spells.rs` gains the two fields and a
`kind` on `SpellInfo`; the tick-line rule that upgrades a row to `dot` stays as
a fallback for spells the table did not classify.

**Wire, v4** (`wisp-proto`): `TimerKind` gains `Slow`; `Timer` gains
`damage_type: Option<DamageType>`, present only when `kind = dot`. The stub
daemon emits one row of each kind so the HUD can be checked without the game.

### 4.4 Rendering

`wisp-hud`'s renderer becomes a small compositor over one screen-sized
premultiplied-ARGB frame: filled rectangles with alpha, text in either embedded
face with proportional advance and tabular digits, and a block layout pass that
turns the config into rectangles. Line-based `render_lines` goes.

The surface is **screen-sized on every backend** (the frame's size is the
output's), transparent where no block is. That is what makes independent
placement, HUD mode's outlines and its help strip trivial, and what the layer
shell and gamescope expect of an overlay. Cost: a full-screen frame is 14.7 MB
at 1440p, so `present` uploads only the **dirty rectangles** — the union of the
blocks that changed since the last frame — and a snapshot that changes nothing
uploads nothing. Redraw is driven by the daemon's 250 ms snapshot tick, as
today; timers count down on that tick.

The X11 backends keep the depth-32 visual, the empty input region and the two
gamescope atoms exactly as Spec 1 set them. The layer-shell backend keeps
`Layer::Overlay`, `KeyboardInteractivity::None` and the empty input region, and
now anchors to all four edges.

### 4.5 Placement

Every block has:

| Key | Values | Meaning |
|---|---|---|
| `anchor` | `top-left`, `top`, `top-right`, `left`, `center`, `right`, `bottom-left`, `bottom`, `bottom-right` | which point of the screen the offset is measured from |
| `offset` | `[x, y]` in unscaled pixels | distance from the anchor, towards the screen's centre |
| `width` | pixels | the block's width; height follows its rows |
| `rows` | integer | maximum rows drawn |
| `hidden` | bool | drawn only as a ghost in HUD mode |

A block anchored `bottom-right` at `[20, 20]` stays in that corner on a 1440p
monitor and on an 800 px-tall handheld, so a layout survives resolution changes
and the compact preset needs no coordinates of its own.

### 4.6 The config file

The flat `key = value` file becomes **TOML**, read with the `toml` crate. The
Spec 4 keys stay at the top level and keep their meaning, so an existing file
still parses; the old `scale` in pixels (default 48) is read as a multiplier
of `px / 13` once, reported, and rewritten as `hud.scale` the first time the
file is saved.

```toml
log = "/mnt/.../eqlog_Daggo_freeport.txt"   # or logs_dir; as in Spec 4
backend = "gamescope"

[hud]
scale = 1.0          # multiplies every size in §4.2
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

**Live reload.** The HUD polls the file's mtime every 500 ms and re-lays out on
change. A file that fails to parse is reported once on stderr with the TOML
error and its line, and the last good layout stays up; the HUD never dies on a
half-saved edit.

**Precedence** is Spec 4's: a flag beats the file, the file beats the default.
`--scale` and `--backend` keep working; there are no flags for blocks.

### 4.7 HUD mode — the keyboard

`wisp-hud` polls `XQueryKeymap` every 50 ms on its own connection to the
display it draws on. Outside HUD mode exactly one thing is recognised: the
chord, `ctrl+shift+grave` by default, chosen because EverQuest binds the
function keys and plain Ctrl combinations and leaves this one alone. The chord
is edge-triggered: held keys toggle once.

In HUD mode every block is drawn with a dashed outline and a name tag
(`meter · damage · fight`), the selected block with a solid bright outline and
a halo, a hidden block as a ghost rectangle, and a help strip along the bottom
edge. The keys, all plain while the mode is on:

| Key | Action |
|---|---|
| `↑ ↓ ← →` | move the selected block by 4 px; with Shift, 24 px |
| `Tab` / `Shift+Tab` | select the next / previous block |
| `[` / `]` | a meter's `shows`: damage ↔ healing |
| `F` | a meter's `segment`: fight ↔ session |
| `+` / `-` | `rows` up or down by one |
| `H` | toggle `hidden` |
| `Esc`, or the chord | save and exit |

Saving writes the config file atomically (write a sibling, rename over), through
`wisp-config`'s writer so the file the CLI reads and the file the HUD reads are
one file with one grammar. The game keeps receiving every key it received
before: the HUD reads state, it does not consume events.

### 4.8 The CLI — `wisp hud`

For setup from a terminal beside the game, and for scripts:

| Command | Effect |
|---|---|
| `wisp hud` | prints the blocks: index, kind, shows, segment, anchor, offset, width, rows, hidden |
| `wisp hud place <n> <anchor> <x> <y>` | sets a block's anchor and offset |
| `wisp hud nudge <n> <dx> <dy>` | moves a block |
| `wisp hud set <n> <key> <value>` | any block key (`shows`, `segment`, `width`, `rows`, `hidden`) |
| `wisp hud add meter\|timers` / `wisp hud remove <n>` | instances |
| `wisp hud scale <x>` | `hud.scale` |

Every verb edits the file and exits; the running HUD picks the change up
through live reload within half a second. Nothing talks to the HUD over a
socket.

### 4.9 Layer shell — the output

`create_layer_surface` is called today with no output, so the compositor picks
one, and on JDS300's desktop it picked the wrong monitor. `hud.output` names a
`wl_output` by its name (`DP-1`); when it is set, the surface is created on that
output, and when the name matches nothing the HUD reports the names it found
and falls back to the compositor's choice. `wisp doctor` lists the outputs.

---

## 5. Milestones

| # | Milestone | Verified by |
|---|---|---|
| 1 | Timer kinds from the spell table; protocol v4 | Unit tests on hand-written rows for every rule in §4.3 including the `Frost Rift` and `Alacrity` negatives; the Spec 2 replay still reproduces its numbers; `wisp status --json` on the real table shows `slow` for `Turgur's Insects` and `damage_type = poison` for `Envenomed Bolt` |
| 2 | Renderer and the two blocks, plain window | Under `Xvfb` with `wispd --stub`, a readback of the frame matches §4.2's colours at known pixels; a screenshot is attached to the plan for JDS300 to compare against the mockups |
| 3 | Config, placement, live reload | An edit to `offset` moves the block within 500 ms without a restart; a broken edit leaves the HUD up and prints one line; a Spec 4 file with `scale = 32` loads with the reported conversion |
| 4 | HUD mode | On the gamescope backend over the game: the chord enters the mode with the game still receiving keys; arrows move a block; `Esc` writes the file; `wisp hud` shows the new numbers |
| 5 | `wisp hud` verbs | Each verb against a temp config; the running HUD follows |
| 6 | Layer-shell output | On a two-output session the surface appears on the named output; a wrong name prints the real names |
| 7 | Live, desktop | JDS300: two meters and the timer list over EverQuest Legends in the Console look; a mez, a slow and a DoT coloured as §4.2 says; the layout survives a restart |

---

## 6. Acceptance criteria

Exact, not impressionistic.

- **Kinds.** Over the frozen fixture, every timer the Spec 2 replay armed still
  arms, and no timer whose table row carries SPA 11 below 100 or a durationed
  negative SPA 0 is reported as `debuff`. The kind distribution is recorded in
  the plan.
- **You appear once.** In a snapshot with your damage and your healing both
  non-zero, a `damage` block draws `You` once and a `healing` block draws
  `You` once. No block draws `You` twice.
- **Bars.** The top meter row's bar is 100 % of the row width minus padding; a
  row with half the top row's amount is 50 %. A timer at `remaining = duration`
  is 100 %, at `0` is 0 %, at `< 0` has no bar.
- **Sizes.** At `scale = 1.0` a row is 24 px tall and the gap 4 px; at
  `scale = 1.5` they are 36 and 6.
- **No focus, ever.** On every X11 backend the window's input region is empty
  and `GAMESCOPE_NO_FOCUS = 1` before and after HUD mode; `xprop` on the running
  window shows no `STEAM_OVERLAY` and no `STEAM_INPUT_FOCUS`. On the layer
  shell, keyboard interactivity is `None`.
- **Live reload.** `wisp hud nudge 0 100 0` moves block 0 by 100 px within
  500 ms with no restart.
- **Still static.** The three binaries still report `statically linked`; the
  `toml` crate and the second font add nothing dynamic.

---

## 7. Risks

| Risk | Standing |
|---|---|
| **Key-state polling misses a tap shorter than 50 ms.** | Accepted. The chord is held, not tapped; HUD-mode keys repeat while held. |
| **In XWayland, `XQueryKeymap` reflects keys only while an X window holds the keyboard.** | The game holds it while you play, which is exactly when the HUD runs. Verified live in the spike: the chord and single keys were seen while playing. |
| **HUD and CLI both write the file.** | Atomic rename; last writer wins; both re-read before writing. A conflicting edit costs one nudge, not a corrupt file. |
| **A full-screen surface costs GPU compositing on the handheld.** | Dirty rectangles bound the upload; gamescope composites one overlay layer regardless of its size. Measured in Milestone 4: HUD CPU under 2 % of a core at the 250 ms tick. Handheld deferred. |
| **A spell qualifies for two kinds** (a slow that also ticks). | Precedence in §4.3, mez > slow > dot > debuff, because that is the order a player needs to notice them. |
| **The mez prose and SPA 31 disagree.** | Either marks a mez. The prose rule caught every mez in the fixture in Spec 2; SPA 31 adds spells whose prose the fixture never printed. |
| **DejaVu Sans's tabular digits.** | DejaVu's digits are equal-width by design; the renderer measures the widest digit once and advances every digit by it, so columns align whatever the face says. |
| **Pointer layout mode stays wanted.** | Appendix A carries the procedure and the exact failure; a second spike runs with `gamescopectl log_focus debug` and the Lutris log window open before any further design. |

---

## Appendix A — the input spike, 2026-09-10

**Question.** Inside gamescope with `--force-grab-cursor`, launched by Lutris
(no Steam): can a separate process see the keyboard without focus, can an
overlay take pointer input for a layout mode, and can it give it back?

**Method.** A throwaway `x11rb` binary on gamescope's display (`:1`), never
committed. Mode A: a 320×200 override-redirect window with
`GAMESCOPE_EXTERNAL_OVERLAY = 1`, `GAMESCOPE_NO_FOCUS = 1` and an empty XFixes
input region — what `wisp-hud` is today — polling `XQueryKeymap` every 30 ms.
On `Ctrl+Shift+grave`, mode B: delete both atoms, set `STEAM_OVERLAY = 1` and
`STEAM_INPUT_FOCUS = 1`, reset the input region, select pointer and key events.
On `Esc`, back to A. Run 2 sized the mode-B window to the screen with a
transparent depth-32 visual, because gamescope paints a Steam-overlay window
stretched to the output (its external-overlay path is the one painted unscaled).

**Results, run 1.**

- Polling saw the chord at 24.04 s while the game was being played, and single
  keys before and after. **Keyboard without focus: proven.**
- 30 ms after mode B: `FocusIn`, `EnterNotify`, `MotionNotify`, `ButtonPress`,
  a drag, `KeyPress` for a letter, with the game still rendering. **Overlay
  input: proven.**
- After `Esc` (30.89 s): one more key seen by polling at 32.06 s (Alt, the
  Alt-Tab to the terminal), then nothing for the rest of the run. When JDS300
  returned to the game it received no mouse and no keyboard. **Hand-back: not
  proven.**

**Diagnosis attempted.** gamescope's own state looked right throughout:
`GAMESCOPE_FOCUSED_WINDOW` and `_NET_ACTIVE_WINDOW` named the game, X input
focus was on the game window with `RevertToNone`, and
`GAMESCOPE_INPUT_COUNTER` kept rising (350 → 720), so gamescope was receiving
input from the desktop and forwarding none of it. Passive listeners on both
game windows saw no events; the pointer position inside gamescope did not
change. Two repairs failed: mapping and destroying an ordinary top-level window
(gamescope focused it and returned to the game), and an exact replay of the
Steam overlay's own sequence — overlay atom on, input-focus atom 1 for 2 s,
then 0, then destroy — which produced the expected `FocusIn`/`FocusOut` pair
and gamescope's cursor re-centre, and still no input reached the game. The
game was relaunched.

**From gamescope's source** (permitted freely; `steamcompmgr.cpp`,
`wlserver.cpp` at the clone of 2026-09-10): external overlays are excluded from
focus candidates; the input focus is the focus window unless a `STEAM_OVERLAY`
window with `STEAM_INPUT_FOCUS` set claims it; `wlserver` nulls its
keyboard- and mouse-focus surfaces when a surface is destroyed; focus is
re-sent to the game only when the computed focus changes. That is consistent
with what was seen and does not explain it. MangoHud's `keybinds.h` (MIT)
confirmed `XQueryKeymap` from a separate display connection as the in-game
hotkey mechanism.

**To re-run.** Rebuild the probe (its behaviour is described above; the plan
carries the source), start the game through Lutris, run
`gamescopectl log_focus debug` on `:1`, keep the Lutris log window open, and
repeat run 1. The focus log will say what gamescope did at the hand-back.

## Appendix B — spell table facts, verified 2026-09-10

Read from the client's `spells_us.txt` (38,211,219 bytes, 173 `^`-separated
fields per row, no header), never copied into the repository:

| Spell | Field 29 (resist) | Field 172 (effects, abbreviated) | Reading |
|---|---|---|---|
| Turgur's Insects | 1 | `2\|11\|85` · `3\|35\|16` | slow (SPA 11 < 100), magic |
| Slow | 1 | `2\|11\|90` | slow |
| Tepid Deeds | 1 | `2\|11\|80` · `3\|35\|9` | slow |
| Alacrity | 0 | `1\|11\|122` | haste, beneficial: not a timer |
| Mesmerization | 1 | `1\|31\|2` | mez (SPA 31) |
| Envenomed Bolt | 4 | `1\|36\|10` · `3\|0\|-295` with duration | dot, poison |
| Chilling Embrace | 4 | `2\|0\|-64` with duration | dot, poison |
| Flame Lick | 2 | `2\|0\|-1` with duration | dot, fire |
| Sicken / Plague | 5 | `2\|0\|-1` / `2\|0\|-97` with duration | dot, disease |
| Frost Rift | 3 | `1\|0\|-24`, no duration | direct damage: not a timer |
| Malaise | 1 | SPA 47/50/48/46 | debuff |
| Tashani | 0 | `1\|36\|1` · `2\|50\|-10` | debuff, unresistable |

## Appendix C — products studied, 2026-09-10

Product level only, as the charter permits; no source of any parser was read.
Recorded in `PROVENANCE.md`.

- **Details! Damage Meter** (World of Warcraft): one window per metric, rows as
  bars relative to the top row, name left and numbers right, ~22 px rows, dark
  translucent backdrop, segments (current fight / overall), skins as presets.
  The model Spec 5 adopts; its interaction is all mouse, which Wisp cannot use.
- **EqTool** (the tool people mean by "EQTools"): timers grouped under the
  target, each a draining bar, beneficial and detrimental in different colours.
  The grouping Spec 5 adopts.
- **EQLogParser's live overlay**: a numeric rank table and a text timer list on
  a faint strip. Proof that small and legible beats rich in-game.
- **EQBuddy** (README and screenshots; JDS300's own contributions are
  disclosed in `PROVENANCE.md`): breakout windows, one per metric, each
  switchable between fight and session. Not drawn from; noted as convergent.
- **The upstream desktop application** (Tier C; one preview image in its
  `docs/previews/`, no CSS, no themes): its timer panel puts state words on
  rows. Spec 5 uses kind labels and colour states instead; nothing of its
  layout, palette or typography is reused.
