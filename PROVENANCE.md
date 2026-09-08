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
Three disappear outright on a Project Quarm target — Quarm is classic through
Velious with no ranked spells.

| Symbol | Upstream | Replacement | Done |
|---|---|---|---|
| `DEFAULT_WARNING_SECONDS` | `10.0` | Wisp's own UI threshold. A presentation choice, not a derived value. | no |
| `DEFAULT_CRITICAL_SECONDS` | `5.0` | As above. | no |
| `SERVER_TICK_SECONDS` | `6` | EverQuest's server tick. A fact about the game; restated, not copied. | no |
| `scaled_duration_ticks(base, rank)` | `(base*(10+rank)+5)//10` | **Deleted.** EQL's 10%-per-rank scaling; on Quarm `rank` is always 0, making it the identity function. | no |
| `split_spell_rank(name)` | parses `Rk. II` | **Deleted.** Quarm has no ranked spells; the path never fires. | no |

---

## Sources

### Standing — always permitted, no per-use record needed

- **EverQuest's own log output.** The primary source. Preferred over any
  second-hand description of behaviour.
- **The EQL wiki** — published spell data.
- **JDS300's own captured game logs** — the only source that reflects Project
  Quarm rather than Live. Upstream's Allakhazam-derived durations were wrong
  for Quarm in several measured cases.
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
