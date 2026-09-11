# Spec 1 — the spine

**Status:** design approved 2026-09-08
**Depends on:** [Spec 0 — clean-room charter](2026-09-08-clean-room-charter.md)
**Target client:** EverQuest Legends. Not Live, not Project Quarm.

---

## 1. What this spec is for

A walking skeleton: log on disk → parsed → over IPC → drawn over the game.
Thin at every stage, complete end to end.

It exists to retire integration risk before depth is built. The overlay is the
only component that can fail in a way that invalidates the architecture;
everything else is understood work. So the skeleton proves the overlay first,
on the machine the author actually plays on, and carries one number that can be
checked exactly rather than judged.

**Done means:** a number, live, over EverQuest, on JDS300's desktop, in the
setup he already uses.

---

## 2. Non-goals

Explicitly deferred, so their absence is a decision rather than an oversight:

- Encounter model, DPS, damage attribution — Spec 2
- Mez, lull and debuff timers — Spec 2
- Log auto-discovery in Wine/Proton/Lutris prefixes — Spec 2. Spec 1 takes
  `--log <path>`.
- The `wisp` CLI as a separate binary — later. The socket already supports more
  than one client, so adding it needs no redesign.
- Any HUD configuration surface, theming, or layout system — Spec 3
- Packaging, AppImage, Flatpak — Spec 4

---

## 3. Invariant: the HUD never takes input

> **Amended by Spec 5 §3.1 (2026-09-10):** the rule stands as "the HUD never changes focus"; the reason given below was corrected by the spike recorded in Spec 5 Appendix A.

**On any backend, ever. Not focusable, not clickable, not draggable, not
resizable by pointer.** All configuration is out of band: CLI flags and a
config file.

This is not a simplification to be relaxed later. It is forced by the
environment:

EverQuest confines the pointer during right-click mouse-look. On multi-monitor
Linux that confinement is unreliable, and the two workarounds in circulation
are winecfg's fullscreen mouse capture and running under gamescope with
`--force-grab-cursor` — which is what JDS300 runs. **Anything that wants clicks
loses to a pointer grab.** A HUD that never wants them cannot lose.

`GAMESCOPE_NO_FOCUS` is therefore an expression of this rule, not a workaround
for a problem the design created.

This is a deliberate break from prior art in this space, which typically offers
drag-to-move, click-to-expand and click-through toggles. Every one of those
fights the grab.

---

## 4. Architecture

### Workspace

```
crates/
  wisp-proto/   snapshot types + NDJSON codec        (lib)
  wispd/        the daemon                           (bin)
  wisp-hud/     the overlay renderer                 (bin)
```

### Protocol

Newline-delimited JSON over a Unix domain socket at
`$XDG_RUNTIME_DIR/wisp/wispd.sock`.

```json
{"v":1,"seq":42,"ts":"2026-08-10T20:39:54","lines_ingested":10432,"session_kills":7}
```

- `wispd` is the server. On connect it sends the current snapshot immediately,
  then one per change, coalesced to at most 5/sec.
- `v` gates compatibility. `wisp-hud` refuses an unrecognised version loudly and
  exits non-zero; it never guesses.
- Multiple concurrent clients are supported from the start.

NDJSON over a socket is chosen for debuggability: `socat - $XDG_RUNTIME_DIR/wisp/wispd.sock`
is a complete diagnostic tool, and the boundary can be exercised without the
renderer existing. The data rate is a few snapshots a second — far below where a
binary codec would earn its opacity. Revisit only if profiling ever says
otherwise.

### `wispd`

```
wispd --log <path> [--from-start] [--stub]
```

A tailer plus three rules. Every rule below is verified against the reference
fixture; none is inferred.

| Rule | Effect |
|---|---|
| any line | `lines_ingested += 1` |
| `You have slain (.+)!` | `session_kills += 1` |
| `Welcome to EverQuest Legends!` | reset session counters |

**`X has been slain by Y!` is deliberately ignored.** It is a third-party kill,
and it accounts for 3,065 of the 6,787 "slain" lines in the fixture. Counting it
would be wrong by 45%, and getting this right on day one sets the standard for
Spec 2.

Rotation and truncation are handled by tracking `(dev, ino)` and detecting size
shrink. The file is polled at 250 ms rather than watched with inotify: fewer
filesystem edge cases, and the latency is invisible behind a 5 Hz snapshot rate.

`--stub` emits a synthetic rising counter with no file access, so `wisp-hud` can
be built before any parsing exists.

### `wisp-hud`

One renderer, one trait, three backends.

```rust
trait OverlayBackend {
    fn attach(&mut self) -> Result<Surface>;
    fn present(&mut self, frame: &Frame);
}
```

| Backend | Mechanism | Covers |
|---|---|---|
| `GamescopeX11` | X11 window inside gamescope's XWayland, with `GAMESCOPE_EXTERNAL_OVERLAY=1` and `GAMESCOPE_NO_FOCUS=1` | JDS300's desktop; Legion Go S; Steam Deck; any gamescope launch |
| `WlrLayerShell` | `zwlr_layer_shell_v1`, overlay layer, no keyboard interactivity, exclusive zone 0 | KDE, Sway, Hyprland, river |
| `PlainWindow` | Ordinary always-on-top window | GNOME, and anything without layer-shell |

**All three implementations are shaped by the trait in Spec 1 even though they
are not all written in Spec 1.** The gamescope mechanism is already verified
concretely, so the seam can be designed to fit both real backends now.
Retrofitting it later is expensive; designing it now is free.

### Backend detection

Falls directly out of the spike, and needs no configuration:

1. If `DISPLAY` resolves to an X server whose **root window carries
   `GAMESCOPE_*` properties**, we are inside gamescope → `GamescopeX11`.
   (The spike observed 17 such properties on gamescope's XWayland.)
2. Else if the Wayland compositor advertises `zwlr_layer_shell_v1` →
   `WlrLayerShell`.
3. Else → `PlainWindow`.

`--backend <name>` overrides, for testing and for users whose situation the
probe misreads.

---

## 5. Milestones

Ordered so that the riskiest thing is proven first, on hardware that is already
in daily use.

| # | Milestone | Verified by |
|---|---|---|
| 1 | `wisp-proto` + `wispd --stub` | `socat` against the socket; no renderer needed |
| 2 | `wisp-hud` + `GamescopeX11` | **A number over EverQuest, in JDS300's normal gamescope session** |
| 3 | Real tailer replaces `--stub` | Exact fixture counts, below |
| 4 | `WlrLayerShell` | Over EverQuest launched without gamescope, on KDE |
| 5 | `PlainWindow` | Any session with neither mechanism |
| 6 | Legion Go S | Same `GamescopeX11` code, different hardware. Closes the spike's open item. |

Milestone 2 is the one that matters. It needs no contrived setup: JDS300 runs
gamescope borderless at 2560×1440 with `--force-grab-cursor` as his normal
configuration, so his ordinary play session is the test rig.

---

## 6. Acceptance criteria

Exact, not impressionistic.

**Parsing.** Against `eqlog_Daggo_freeport.txt` with `--from-start`:

| Value | Expected |
|---|---|
| `lines_ingested` | `1440036` |
| `session_kills` (whole file, no resets) | `3722` |
| session resets observed | `39` |

Equality, not approximation. `X has been slain by Y!` must contribute zero.

**Overlay.**

- A photograph or screen capture of `wisp-hud` showing a live, updating number
  over EverQuest on the desktop, under gamescope.
- `wisp-hud` never takes focus and never receives pointer or keyboard input, on
  every backend built.
- Mouse-look in EverQuest behaves identically with the HUD running and not
  running. This is the real test of the §3 invariant.

**Daemon.**

- `wispd` survives log truncation and file replacement without restarting.
- Killing and restarting `wisp-hud` loses no daemon state.

---

## 7. Risks

| Risk | Standing |
|---|---|
| **`GAMESCOPE_NO_FOCUS` may not deliver click-through.** The spike proved the atom is *accepted*, never that input passes through. | **Proven on the desktop rig, 2026-09-08.** With `GAMESCOPE_NO_FOCUS` set and an empty XFixes input region on the window, mouse-look under `--force-grab-cursor` was identical with and without the HUD, and the counter updated live (see PROVENANCE.md). The two measures are set together, so this does not separate them. The handheld (Milestone 6) remains open. |
| **Text legibility at handheld scale.** A 7" 1920×1080 panel is a different design problem from a 2560×1440 desktop. | Scale must be an explicit parameter from the first frame, never inherited from a desktop default. |
| **gamescope on NVIDIA is fragile.** A default-backend invocation failed hard here with `vkCreateComputePipelines … VK_ERROR_INVALID_SHADER_NV`; JDS300's flags start cleanly. | Understood, not solved. Pin testing to the known-good invocation and record it. |
| **Wine prefix paths** contain spaces and vendor names. The reference fixture lives under `.../drive_c/users/Public/Daybreak Game Company/Installed Games/...`. | Path handling must be `OsStr`-clean throughout. No `String` round-trips. |
| **Text rendering crate is unchosen.** | Deliberately open. Settle during milestone 2 in favour of the smallest dependency that draws legible text; it is not an architectural decision. |

---

## Appendix — verified facts

Everything here was read from primary sources, per the charter. Nothing is
inferred from another parser's behaviour.

**Reference fixture.** `eqlog_Daggo_freeport.txt`, EverQuest Legends,
117 MB, 1,440,036 lines.

**Line format.** `[Mon Aug 10 20:39:54 2026] <text>` — naive local wall clock,
no timezone. Any UTC label applied to this without conversion is a bug.

**Counts in the fixture.**

| Pattern | Count |
|---|---|
| `You have slain …!` | 3,722 |
| `… has been slain by …!` | 3,065 |
| `Welcome to EverQuest Legends!` | 39 |
| `Rk. II` (Live rank convention) | **0** |

**Spell ranks are bare trailing roman numerals** — `Dazzle V`, `Pacify V`,
`Swift Like the Wind III`. Not `Rk. II`, which is an EverQuest *Live*
convention and does not appear in EQL at all.

**The gamescope overlay mechanism.** An ordinary, unprivileged third-party X11
client launched into gamescope's XWayland accepts both atoms; verified by
readback from the X server:

```
   overlay candidate window: 0x400002       <- plain glxgears
GAMESCOPE_EXTERNAL_OVERLAY(CARDINAL) = 1
GAMESCOPE_NO_FOCUS(CARDINAL) = 1
```

`mangoapp`, shipping on every Steam Deck, is exactly this shape: a separate
process linking `libX11`, `libGL` and GLFW, using those two atoms. gamescope's
own help recommends it over drawing inside the game.

**`zwlr_layer_shell_v1` version 5** is advertised by KWin on the development
machine. GNOME/Mutter implements no layer-shell and is not expected to.

**gamescope ships display profiles** for `valve.steamdeck.lcd.lua` and
`lenovo.legiongos.lcd.lua`, among others — the Legion Go S is a first-class
gamescope target, not a proxy for one.
