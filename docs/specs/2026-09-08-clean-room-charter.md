# Wisp — clean-room charter

**Status:** design approved 2026-09-08, pending written review
**Applies to:** `github.com/JDS300/wisp`, from its first commit onward
**Supersedes:** nothing. This is Spec 0.

---

## 1. Why this document exists

`itsspin/spinips` carries **no licence**. Not a restrictive one — none at all.
Verified 2026-09-08: `git ls-files | grep -i licen` returns nothing across the
entire merged history, and no README or doc mentions one.

Under default copyright that means no rights are granted, so `JDS300/spinips`
exists on GitHub's Terms of Service alone — which licenses forking and viewing
*within GitHub* and nothing further. Redistribution off-platform, relicensing,
and inviting others to redistribute are not covered. Upstream was asked to
support Linux and did not reply; silence is neither permission nor refusal, and
the fork has been living in that gap.

Wisp exists to leave the gap. It is an independent, Linux-only EverQuest log
parser and in-game overlay, owned outright by its author and licensed so that
others may actually use it.

This charter fixes the rules **before** any code is written, because provenance
decisions are cheap now and expensive to unwind later.

---

## 2. What Wisp is

A headless parser daemon plus a thin in-game overlay renderer, for Linux only,
including handhelds running gamescope.

| Component | Role |
|---|---|
| `wispd` | Headless parser. Tails the EQ text log, emits state snapshots over IPC. No UI, no display connection. |
| `wisp` | CLI and control client. Configuration, diagnostics, status. |
| `wisp-hud` | Overlay renderer. Attaches to whichever display the game is on and draws the HUD. |

- **Repository:** `github.com/JDS300/wisp`
- **Flatpak application ID:** `io.github.jds300.Wisp`
- **Store subtitle:** *Wisp — EverQuest log parser and in-game overlay for Linux and Steam Deck*
- **Tagline:** *A light over the fight.*

The daemon/renderer split is not stylistic. The
[overlay spike](#appendix-a--why-the-split-is-mandatory) established that on
gamescope the overlay must run as a client *inside the game's own XWayland
instance*, on a different display from everything else. A monolithic GUI cannot
satisfy that.

---

## 3. The provenance model

Three tiers, fixed at the start. Every file entering Wisp is assigned one.

> **Authorship note applying to every file in Tiers A and B.** All five were
> written by JDS300 *with AI assistance throughout*: git names JDS300 as sole
> author, but all 14 commits touching them carry a
> `Co-Authored-By: Claude` trailer. This is stated here rather than left to be
> discovered. It does not affect tier assignment — what matters for provenance
> is that these files contain none of upstream's expression, and machine-written
> code is independently generated rather than copied. It does affect the
> strength of the copyright claim over them; see §9.

### Tier A — moves verbatim

Sole git author JDS300, stdlib-only or pure data, no upstream imports.

| File | Size | Evidence |
|---|---|---|
| `tools/appimage_update_info.py` | 302 lines | Sole author JDS300, added in `d16a2f1`. Imports `pathlib` only. |
| `tools/scrape_debuff_spells.py` | 111 lines | Sole author JDS300. No third-party imports at all. |
| `loremaster/tests/fixtures/debuff_spell_reference.json` | 87 entries, 19 KB | Data, not code. Scraped facts plus durations measured from JDS300's own logs. |

Copied with authorship intact: export each file's history with
`git log --follow --format=...` into `PROVENANCE.md` rather than asserting
authorship in prose.

### Tier B — moves after severance

Sole git author JDS300, carrying exactly one upstream coupling.

| File | Size | Coupling |
|---|---|---|
| `loremaster/debuff_timer.py` | 628 lines, 6 commits, all JDS300 | `from mez_timer import (...)` — five symbols |
| `tools/diagnose_debuff_timers.py` | 258 lines | Imports `debuff_timer` only; clean once the above is |

**The rule is sever-then-import.** The replacement happens before the file
lands in Wisp, so no commit in Wisp's history ever contains an upstream
reference. There is no window in which the log shows a dependency that was
later removed.

### Tier C — never moves, at any point

- Everything else under `loremaster/` — including `loremaster.py`,
  `mez_timer.py`, `lull_timer.py`, `desktop_worker.py`, `engine_protocol.py`
- All of `spinui_reloaded/`, `spinui_glass/`, `layouts/`, `installer/`
- The entire `loremaster-desktop/` Electron application, its CSS and its themes
- `linux_capture.py` and `hover_ocr.py` — not by contamination but by
  irrelevance: Wisp performs no screen capture and no OCR, so they have no
  reason to move
- **The names:** Loremaster, Rune Seed, SpinUI, Lore Lens, Vellum & Ember,
  Midnight Frost Glass, Adventurer's Chronicle, Spin's Loremaster

Names are trademark exposure independent of copyright, and they are the part
people actually recognise. None of them appear in Wisp, including in comments,
commit messages, or documentation, except where this charter discusses them.

---

## 4. Severance record

The five symbols `debuff_timer.py` imports from upstream's `mez_timer.py`, and
what replaces each.

> **Corrected 2026-09-08, same day.** An earlier revision deleted two of these
> on the reasoning that Wisp targeted Project Quarm, which has no ranked
> spells. **Wisp targets EverQuest Legends**, a different client and codebase.
> EQL does rank spells, so both symbols return as rewrites. See the log entry
> in `PROVENANCE.md`.

| Symbol | Upstream value | Replacement in Wisp |
|---|---|---|
| `DEFAULT_WARNING_SECONDS` | `10.0` | Wisp's own UI threshold constant. An arbitrary presentation choice, not a derived value. |
| `DEFAULT_CRITICAL_SECONDS` | `5.0` | As above. |
| `SERVER_TICK_SECONDS` | `6` | EverQuest's server tick. A fact about the game; restated, not copied. |
| `scaled_duration_ticks(base, rank)` | `(base*(10+rank)+5)//10` | **Rewritten.** EQL scales duration 10% per rank. The rule is published and arithmetic; Wisp implements it from the rule, not from this expression. |
| `split_spell_rank(name)` | parses `Rk. II` suffixes | **Rewritten, and differently.** `Rk. II` is an EverQuest *Live* convention that does not occur in EQL: 1.44M lines of EQL log contain zero instances. EQL ranks with a bare trailing roman numeral — `Dazzle V`, `Pacify V`, `Swift Like the Wind III`. Wisp parses that, derived from the log rather than from upstream. Names ending in numeral letters are the false-positive trap. |

Net: two constants Wisp chooses for itself, one integer that is a property of
the game, and two small functions rewritten from published rules and from the
log — one of which upstream appears to implement for the wrong client.

---

## 5. Consultation rules

What may be looked at, and what gets written down.

| Source | Permitted | Recorded |
|---|---|---|
| **`itsspin/spinips` and `JDS300/spinips`** | Behaviour only — *what* it does in a given situation. Never expression: no copying code, structure, comments, naming, or file organisation. | Yes. Any consultation informing a design decision gets a dated line in `PROVENANCE.md`. |
| **EQ Companion and other third-party parsers** | Product only — features, what they surface, UX conventions. **Source is off-limits.** Reading it would recreate this exact problem against a second party. | Yes, when it informs a feature decision. |
| **MangoHud, gamescope, mangoapp** | Source freely. MIT and BSD-2-Clause respectively, and `mangoapp` is the reference implementation for the overlay mechanism. | In `THIRD_PARTY.md`, with their licence terms honoured. |
| **EverQuest itself** | Always. Log output, the EQL wiki, and JDS300's own captured logs are primary sources. | No record needed; these are the default. |

**The operative distinction:** facts and mechanics are not protectable by
anyone. Six-second ticks, `<mob> yawns.`, 10%-per-rank scaling, the shape of an
`eqlog_*.txt` line — none of that is authorship. Expression is. The tiers exist
to keep expression out while letting facts flow freely.

**Prefer primary sources.** Where a behaviour can be established from the game's
own output rather than from reading how someone else handled it, do that. It is
both cleaner and usually more accurate — upstream's Allakhazam-derived durations
were wrong for Quarm in several places that a real log corrected.

---

## 6. Licence

**MIT.**

- `LICENSE` at the repository root, verbatim, **in the first commit**. No
  published commit is ever unlicensed.
- An SPDX header in each source file:
  `# SPDX-License-Identifier: MIT`
- Copyright holder: JDS300, personally.
- `THIRD_PARTY.md` records the terms of anything vendored or studied closely.

Chosen deliberately over copyleft. Wisp is built for its author's own use and
published because others may benefit; it is not sold, and no value is placed on
preventing anyone from taking it in another direction. Copyleft exists to stop
a work being closed, and that is not an outcome this project objects to. MIT
imposes essentially nothing on anyone, which is the intent.

The consequence, accepted knowingly: upstream — or anyone — may absorb Wisp's
code without reciprocating, while `itsspin/spinips` remains unlicensed and
unusable in the other direction. That asymmetry is a deliberate non-concern.

**What matters far more than which permissive licence is chosen is that one
exists at all.** The absence of exactly this file is what made the fork a dead
end.

---

## 7. Attribution and git identity

Three rules. The first was urgent and is already done.

1. **Global identity fixed.** The machine's global git identity was
   `OpenCode Assistant <opencode@localhost>` — a stale agent default that maps
   to no account and had already authored 34 commits in `spinips`. Any new
   repository would have inherited it from commit #1. Corrected by JDS300 on
   2026-09-08 before Wisp exists.

2. **Set identity repo-locally as well**, so a later global change cannot
   silently reassign authorship:

   ```
   git config user.name  "JDS300"
   git config user.email "70587798+JDS300@users.noreply.github.com"
   ```

   The GitHub noreply address is used deliberately. It attributes identically
   to the account while keeping a real address out of a public repository that
   will be posted publicly.

3. **One AI trailer, used consistently.** Every commit containing
   machine-written code carries exactly:

   ```
   Co-Authored-By: Claude <noreply@anthropic.com>
   ```

   `spinips` currently has three variants across 125 commits. One form is
   greppable, and greppable is what makes it evidence:
   `git log --grep="Co-Authored-By: Claude"` becomes an honest answer to
   "which parts were AI-written?"

   **This matters for provenance, not just courtesy.** The copyright status of
   machine-generated code is unsettled. A contemporaneous record of which
   commits were AI-assisted is worth more than a claim made afterwards.

`OpenCode Assistant` does not appear in Wisp at all.

---

## 8. Evidence: `PROVENANCE.md`

Written in the first commit, updated as a matter of course, never reconstructed.

Contents:

1. The Tier A/B/C table — every file that moved, and why it was permitted to
2. The severance record from §4, stating what replaced each symbol
3. Standing sources: EQ log output, EQL wiki, JDS300's captured logs,
   gamescope/mangoapp source
4. Deliberately **not** consulted: upstream's `loremaster/` beyond behaviour,
   EQ Companion's source
5. Dated entries whenever a decision is informed by consulting something

Its value is that it is contemporaneous. Written alongside the work it
describes, it is a record. Assembled afterwards, it is an argument.

---

## 9. What this charter does not claim

Stated plainly so nobody relies on more than it offers.

- **It is not legal advice**, and it is not a guarantee against someone
  choosing to be difficult.
- **It does not make the author unaware of upstream.** JDS300 has read
  `loremaster.py`; so has the assistant. Clean-room in its strict sense — an
  implementer with no exposure — is not what is being claimed. What is claimed
  is documented independent development: upstream's expression stays out, its
  facts are re-derived from primary sources, and the record of how is kept as
  the work happens.
- **It does not claim strong copyright over the ported files.** All 14 commits
  behind Tiers A and B are AI-assisted (§3). The copyright status of
  machine-generated code is unsettled, and a purely generated work may attract
  thin protection or none — human direction, selection and arrangement are what
  carry a claim. Under MIT this is close to immaterial: the licence asks almost
  nothing of anyone, so there is little to enforce and little lost if it proves
  unenforceable over some files. It would have mattered under copyleft. It has
  no bearing on the upstream question either way, which is about not copying
  someone else's expression.

- **It does not resolve the EverQuest question.** Wisp reads a log file the
  game already writes. Log parsers are a twenty-year-old genre and this one
  neither injects nor reads process memory, but Daybreak's marks are Daybreak's
  and Wisp uses none of them in its name or branding.

---

## 10. Open items

| Item | Blocking? |
|---|---|
| Confirm gamescope overlay compositing on real AMD hardware or a Steam Deck | Not for this spec. Blocks Spec 1's acceptance criteria. |
| Domain registration | No. `io.github.jds300.Wisp` needs none. |

---

## Appendix A — why the split is mandatory

From the overlay feasibility spike, 2026-09-08:

An ordinary, unprivileged third-party X11 client launched into gamescope's own
XWayland accepted both overlay atoms, verified by readback from the X server:

```
   overlay candidate window: 0x400002       <- plain glxgears
GAMESCOPE_EXTERNAL_OVERLAY(CARDINAL) = 1
GAMESCOPE_NO_FOCUS(CARDINAL) = 1
```

`mangoapp` — shipping on every Steam Deck — is exactly this: a separate process
linking `libX11`, `libGL` and GLFW, using those same two atoms. gamescope's own
help recommends it *over* the in-game Vulkan layer.

Consequences carried into Spec 1:

1. The overlay runs inside the game's gamescope instance, on a different
   display from the parser. Daemon and renderer must be separate processes with
   an IPC boundary.
2. The renderer is small and native. Electron is the wrong tool for a gamescope
   overlay plane.
3. Two attach backends, one renderer: `gamescope-xwayland` (atoms) for
   handhelds, `wlr-layer-shell` for desktop Wayland. KWin advertises
   `zwlr_layer_shell_v1` v5; GNOME implements no layer-shell and falls back to
   an ordinary always-on-top window.
4. **The non-injecting principle survives intact.** Nothing reads game memory,
   preloads into the game, or layers into its render pipeline. Wisp is a
   sibling window the compositor is asked to raise — and that claim can now be
   made about the Steam Deck too.

Not verified: pixel compositing, because gamescope's GPU path fails on the
development machine's NVIDIA driver (`vkCreateComputePipelines` →
`VK_ERROR_INVALID_SHADER_NV`). The Deck is AMD/RADV where this is the shipping
path, so the failure is almost certainly NVIDIA-specific — but one confirmation
on real hardware is owed before Spec 1 is called done.
