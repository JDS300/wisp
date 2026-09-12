# Spec 7 — the face: the icon, the tray's four states, the README

**Status:** designed 2026-09-12; implemented 2026-09-12; pending JDS300: Milestone 5 (the icon in the app menu and Gear Lever after the next beta; the tray grey, green, bright and red in play)
**Depends on:** [Spec 6 — running it](2026-09-11-spec-6-running-it.md), [Spec 4 — packaging](2026-09-09-spec-4-packaging.md), [Spec 0 — clean-room charter](2026-09-08-clean-room-charter.md)
**Target client:** EverQuest Legends. Not Live, not Project Quarm.

---

## 1. What this spec is for

Wisp now runs, stops, updates and sits in the tray. What it looks like from
the outside has not caught up: the launcher icon is the hand-drawn SVG from
Spec 4, the tray shows one green wisp whatever the daemon is doing, and the
README is a narrative of how the project was built rather than a page a
player reads to install and use it.

JDS300 produced one design sheet on 2026-09-12 — a README banner, launcher
icons at two sizes, and systray tiles in four states — and asked for three
things: the icons in the app, the tray states meaning something, and a
README a user can read. This spec does exactly those three. Analytics,
which was Spec 7 on the roadmap, becomes Spec 8.

---

## 2. Non-goals

- **A new mark.** The sheet is the art. Nobody redraws the flame, the
  heartbeat or the arc; the SVG from Spec 4 stays as the scalable fallback
  and is not restyled to match.
- **Transparent tray icons.** The sheet is a JPEG with glows baked onto dark
  rounded squares. The tray tiles ship with those squares. JDS300 chose this
  over exporting transparent PNGs; a later sheet can replace the tiles by
  re-running one script.
- **Symbolic (monochrome) tray icons** for panels that recolour icons. The
  tiles are coloured and that is the point of them.
- **Screenshots in the metainfo, Flathub submission**, and any change to
  the desktop entry's `Icon=` name.
- **Behaviour changes.** No daemon rule, no HUD behaviour, no CLI verb
  changes. The tray's four states read state the HUD already holds.
- **Analytics.** Spec 8.

---

## 3. Invariants

### 3.1 Unchanged

- Spec 6 §3.2: the tray never blocks the HUD. Changing which pixmap the
  item shows goes through the same updater thread as the menu.
- Spec 5 §3.2: the config file is the layout; nothing here adds a key.
- Spec 4: three static musl binaries; the tarball's `install.sh`, the
  AppImage and the Flatpak install the same icon name,
  `io.github.jds300.Wisp`, which the desktop entry names.

### 3.2 New

- **One master, one script, committed outputs.** The sheet lives at
  `docs/art/wisp-sheet.jpeg`; `packaging/render-icons.sh` cuts every
  raster from it at fixed pixel boxes; the outputs are committed. CI needs
  neither the sheet's tools nor the script. Re-running the script on the
  same sheet reproduces the same bytes.
- **The tray icon is a pure function of `TrayState`.** Four states, decided
  in one `fn icon_state(&TrayState) -> IconState`, unit-tested, no D-Bus.
- **The README is for a user.** It contains nothing about how Wisp was
  built, which spec did what, or which agent ran; that record moves to
  `docs/STATUS.md` whole and unchanged.

---

## 4. Architecture

### 4.1 The sheet and the slicing script

`docs/art/wisp-sheet.jpeg` is the file JDS300 added as
`docs/Gemini_Generated_Image_hixntehixntehixn.jpeg`, moved and renamed. It
is 2816×1536, sRGB, and stays a JPEG: it is the record of the design, not a
build input the build runs.

`packaging/render-icons.sh` (bash, `rsvg-convert` is not needed; `magick`
from ImageMagick 7 is, and `readelf`-style byte checks as
`render-tray-icon.sh` does today) crops the sheet at these boxes, measured
on 2026-09-12 and verified by cropping:

| Cut | `-crop WxH+X+Y` | Then |
|---|---|---|
| README banner | `1570x843+47+99` | `docs/art/banner.png`, resized to 1280 wide |
| Launcher tile | `520x520+1756+200` | trimmed to the rounded square with `-fuzz 8% -trim`, then resized to 512, 256, 128, 64, 48, 32, 24, 16 → `packaging/icons/hicolor/<n>x<n>/apps/io.github.jds300.Wisp.png` |
| Tray, tailing (the sheet's "Active (Variant)") | `340x350+770+1105` | trimmed the same way, then 48, 32, 24, 22, 16 → `crates/wisp-hud/icons/tray-tailing-<n>.argb` |
| Tray, fighting (the sheet's "Active") | `340x350+80+1105` | → `tray-fighting-<n>.argb` |
| Tray, error | `340x350+1470+1105` | → `tray-error-<n>.argb` |
| Tray, waiting (the sheet's "Inactive (Idle)") | `340x350+2170+1105` | → `tray-waiting-<n>.argb` |

**Amendment, 2026-09-12 (final review):** the crop boxes above were swapped
from the original design sheet reading. The sheet never labelled which tile
was Tailing and which was Fighting; JDS300 picked by brightness, and the
first cut assigned the flatter tile ("Active") to Fighting and the glowing
one ("Active (Variant)") to Tailing — the opposite of what §4.3 and the
README say ("green" for Tailing, "bright green" for Fighting). The review
measured mean tile green at 48px: the tile at `+80+1105` averages 84.9,
the tile at `+770+1105` averages 68.0. Swapping the boxes, not the words,
makes Fighting the brighter tile, matching the spec's and the README's
prose.

The trim makes each tile exactly its rounded square; the implementer
records the post-trim sizes in the plan's ledger so a later sheet with a
different layout is caught by the script's size checks rather than shipped.
Resizing uses `-filter Lanczos` and, for the launcher PNGs, keeps the
square's own background (the tile is opaque by design). ARGB output uses
the byte order and the byte-count check `render-tray-icon.sh` established
in Spec 6; that script and the two files it produced (`tray-22.argb`,
`tray-48.argb`) are removed, replaced by this one and its twenty.

The script refuses when the sheet's dimensions are not 2816×1536, when any
tool is missing, or when an output's byte count is not `n·n·4`.

### 4.2 Installing the icon

The name stays `io.github.jds300.Wisp`. Every place that installs the SVG
today also installs the PNG set:

- `packaging/install.sh`: the eight PNGs under
  `$prefix/share/icons/hicolor/<n>x<n>/apps/`, uninstalled with the SVG.
- `packaging/release.sh`: the same eight under the AppDir's hicolor tree,
  and the 256 PNG as the AppDir root icon `io.github.jds300.Wisp.png` that
  appimagetool and Gear Lever read (the root SVG is dropped; appimagetool
  takes one root icon and prefers the raster when both exist, which is the
  ambiguity this removes). The tarball carries the SVG as before plus the
  PNG directory, and `install.sh` installs both.
- `packaging/flatpak/io.github.jds300.Wisp.yml`: the same `install -Dm644`
  lines for the PNGs.

Desktops that prefer a raster at a given size pick the PNG; those that ask
for scalable get the SVG. The visible result is the sheet's launcher tile in
the app menu, in Gear Lever, and in the AppImage's own icon.

### 4.3 The tray's four states

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState { Waiting, Tailing, Fighting, Error }

/// Pure. Error wins: a layout that failed to parse is the one thing the
/// tray can tell the user that nothing else on screen does.
pub fn icon_state(s: &TrayState) -> IconState {
    if s.layout_error { return IconState::Error; }
    match (s.log_name.is_some(), s.fight.is_some()) {
        (false, _) => IconState::Waiting,
        (true, false) => IconState::Tailing,
        (true, true) => IconState::Fighting,
    }
}
```

`TrayState` gains `layout_error: bool`, written by the render loop from
`config.layout_error().is_some()` at the same point it publishes the rest
(the value is already read there for the HUD-mode refusal). The mapping:

| State | Sheet tile | When |
|---|---|---|
| Waiting | Inactive (Idle), grey | the snapshot has no `log` — the daemon is up and no log file exists yet |
| Tailing | Active (Variant), green | a log is being tailed and no encounter is active |
| Fighting | Active, bright green | `encounter.active` |
| Error | Active (Error), red | the config's layout failed to parse; HUD mode is refused and `wisp hud` says why |

`WispTray::icon_pixmap` returns the five sizes for the current state;
`icon_name` still returns the theme name as the fallback for hosts that
prefer it. `MenuModel` gains `icon: IconState` so `TrayHandle::publish`'s
diff rule — "nudge the updater only when what the tray shows changed" —
covers the icon without a second rule. The status line's `idle`/`fighting`
words are unchanged; the icon says the same thing at a glance.

**Correction, 2026-09-12 (beta.5 defect fix, same day).** The paragraph
above is wrong and is left in place, struck through in spirit, so the
record shows what was actually shipped in beta.4 and why it changed:
`icon_name` returning `ITEM_ID` was never a harmless fallback. The
StatusNotifierItem spec leaves the choice to the host, and Plasma's SNI
watcher resolves a non-empty `IconName` against the icon theme *whenever
that name resolves*, falling back to `IconPixmap` only when it does not.
`io.github.jds300.Wisp` is exactly the name a tarball install's
`hicolor/*/apps/io.github.jds300.Wisp.png`, the AppImage's own icon, and
the Flatpak's exported icon all register under (§4.1) — so on every install
that matters the name always resolved, and JDS300's Plasma panel showed the
static launcher tile for the lifetime of the process, never the four live
states this section exists to add. It went unnoticed through review and
Milestone 5 because the box that first ran it had a leftover test Flatpak
installed whose export tree happened to be stale, so Plasma fell through to
`IconPixmap` there by accident and the four states appeared to work.

The fix (beta.5): `icon_name` is no longer overridden and returns ksni's
default, an empty string, so `IconName` is always `""` and every SNI host
falls straight to `IconPixmap`. `id()` — the well-known bus name and the
desktop-file match — is unchanged; only the *icon* lookup stops naming a
theme entry. `icon_pixmap` and everything below this paragraph stands as
designed.

There is no state for "the daemon is gone": the HUD exits when the socket
closes, and the item leaves the bus with it.

### 4.4 The README

`README.md` is rewritten as a product page, in this order, and nothing
else:

1. The banner (`docs/art/banner.png`) and one sentence: what Wisp is and
   what it is for.
2. **What it does** — the HUD over the game, the two block kinds, the
   timers coloured by kind, the tray, all in a screen's worth of prose.
   No history.
3. **Install** — Gear Lever with the AppImage first (that is how JDS300
   runs it), then the AppImage by hand, the Flatpak, the tarball with
   `install.sh`. One paragraph on the two update channels in a user's
   words: a release build updates to releases; a beta build updates to
   betas; switch by installing the other once.
4. **First run** — `wisp run`, or `wisp run -- %command%` as a launch
   option; where the log is found and how to point at it (`wisp config set
   logs_dir …`); `wisp doctor`.
5. **The HUD** — what is on screen; HUD mode's chord and keys as a table;
   `wisp hud` verbs as a table; a short config example.
6. **Running it** — the tray's entries and what its four colours mean;
   `wisp stop`; `wisp status`.
7. **Troubleshooting** — `wisp doctor` line by line; the two or three things
   that go wrong (no log yet, the HUD on the wrong monitor, HUD mode keys
   reaching the game on gamescope).
8. **Where the rest is** — three links: `docs/STATUS.md`, the specs
   directory, `docs/RELEASING.md`. Licence line.

Everything the README says must be true of the shipped build; the plan's
docs task checks each command against `wisp`'s usage text as Spec 6's did.

`docs/STATUS.md` receives, unchanged, the README's current "What this will
be" roadmap, the "Status" table, and the "Provenance" section, with a first
line saying where they came from and when. Links elsewhere in the tree that
pointed at README anchors are repointed (the plan's docs task greps for
them).

---

## 5. Milestones

| # | Milestone | Verified by |
|---|---|---|
| 1 | The script and the outputs | `packaging/render-icons.sh` run twice yields identical bytes; every PNG and ARGB has the expected dimensions; the four 48-px tray tiles and the 256-px launcher, montaged, look like the sheet (the montage is attached to the plan for JDS300) |
| 2 | The icon installed | `install.sh` into a scratch prefix lists eight PNGs and the SVG; the AppImage's root icon is the 256 PNG; the Flatpak build installs the PNGs; `--version` still prints |
| 3 | The four states | `icon_state` under unit test for every row of §4.3's table including Error over Fighting; `publish` nudges on an icon change alone; the dbusmenu/SNI `IconPixmap` property read over the bus shows five sizes |
| 4 | The README | Every command in it exists in `wisp`'s usage text with that spelling; no spec number, agent name or date-of-work appears in it; `docs/STATUS.md` holds the moved sections verbatim; no dangling anchor |
| 5 | Live, JDS300 | After the next beta: the sheet's launcher tile in the app menu and in Gear Lever; the tray grey before the game logs in, green once it does, bright in a fight, red after `wisp hud set 0 anchor sideways` breaks the layout and back to green after `wisp hud place 0 top-left 20 120` fixes it |

---

## 6. Acceptance criteria

- `packaging/render-icons.sh` exits 0 on the committed sheet and produces
  exactly 1 banner, 8 PNGs and 20 ARGB files; a second run changes no file
  (`git status --short` empty).
- `magick identify` on each PNG reports the size in its directory name;
  each `tray-<state>-<n>.argb` is `n·n·4` bytes.
- With the sheet replaced by a 100×100 image the script exits 2 and writes
  nothing.
- `icon_state` returns Error when `layout_error` is set regardless of the
  other fields; Waiting when `log_name` is `None`; Tailing and Fighting by
  `fight`.
- `busctl --user get-property <item> /StatusNotifierItem org.kde.StatusNotifierItem IconPixmap`
  on a running HUD lists five entries with widths 16, 22, 24, 32, 48.
- README.md contains no line matching `Spec [0-9]`, `Milestone`, `agent`,
  or a 2026 date; `docs/STATUS.md` contains the moved sections byte for
  byte.
- The Spec 3 replay guard and every existing test pass unchanged; no
  `PROTOCOL_VERSION` change.

---

## 7. Risks

| Risk | Answer |
|---|---|
| **JPEG artefacts at 16 px.** | Lanczos from the 340-px tile hides them at 24 and above; 16 is the sheet's own smallest target and it was drawn to survive it. If 16 looks muddy in Milestone 5, the answer is a transparent export from the generator, which is the non-goal this spec deferred, not a different resize. |
| **An opaque dark tile on a light panel.** | Accepted by JDS300 for his dark Plasma panel. The tile carries its own rounded square, so it is legible on light panels too, just heavier than a symbolic icon. |
| **Plasma caches the old icon by name.** | The name is unchanged on purpose; Plasma refreshes hicolor on install and Gear Lever re-extracts the AppImage's root icon on update. If a stale icon persists, `kbuildsycoca6` is the user-side fix and the README's troubleshooting section says so. |
| **Error masks Fighting.** | By design: a layout that will not parse is the one state the player cannot see from the HUD, and it is fixed in seconds with `wisp hud`. The status line still says `fighting`. |
| **The README drifts from the build.** | Milestone 4's command check runs in the docs task and again in the final review; RELEASING.md's checklist gains one line: read the README's install section against the release you just cut. |
