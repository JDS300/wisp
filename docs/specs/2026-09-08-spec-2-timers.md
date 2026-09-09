# Spec 2 — timers

**Status:** design approved 2026-09-08 (approach A); written spec reviewed by JDS300 pending
**Depends on:** [Spec 1 — the spine](2026-09-08-spec-1-the-spine.md), [Spec 0 — clean-room charter](2026-09-08-clean-room-charter.md)
**Target client:** EverQuest Legends. Not Live, not Project Quarm.

---

## 1. What this spec is for

Countdown timers for the spells the player lands on mobs: mesmerise, lull,
slows, damage-over-time, resist debuffs, charm. One row per active timer on
the overlay, soonest expiry first, colour shifting as it runs out.

The spine carries one number. This spec carries the first thing JDS300 actually
plays with. It exists to prove three mechanisms on real play: correlating a
local cast with the prose line that says it landed, reading durations from the
game's own data files, and correcting those durations from what the log
actually shows.

**Done means:** a mez cast in play produces a row whose countdown reaches zero
within one second of the "has worn off" line, on JDS300's desktop, in the
setup he already uses.

---

## 2. Non-goals

Explicitly deferred, so their absence is a decision rather than an oversight:

- Encounter model, DPS, damage attribution — Spec 3.
- Buffs on the player or the group (Illusion, Holy Steed, "Cauli looks
  protected") — a different display and a different question. Later.
- Log auto-discovery — still `--log <path>`. Bounded task, later.
- A bundled spell table. Wisp reads the client's own `spells_us.txt` and never
  redistributes it. If the file cannot be found, timers do not run and the
  daemon says so. `--spells <dir>` overrides the search.
- Modelling focus items, AA extensions, or the caster's level. Learned durations
  absorb all of them (see §4, durations).
- Timers for spells cast by other players. Only a local cast can arm a row.
- Any HUD configuration surface, theming or layout system — Spec 3 territory.
  This spec adds one list under the existing number and two colour
  thresholds, nothing more.

---

## 3. Invariants

Spec 1's invariant stands unchanged: **the HUD never takes input, on any
backend, ever.** Nothing here adds a control.

Two rules are new and binding:

1. **Game data is read, never shipped.** `spells_us.txt` and
   `spells_us_str.txt` are Daybreak's. Wisp opens them from the user's own
   install at runtime. No copy, excerpt or derived table of them enters the
   repository, its tests, or its releases. Test fixtures are hand-written
   rows in the same format, a dozen at most, covering only the spells the
   tests name.
2. **Naive timestamps are subtracted, never relabelled.** The log's
   `[Mon Aug 10 20:39:54 2026]` is local wall clock with no zone. This spec
   parses it for one purpose: measuring the interval between two lines in the
   same log. Differences are safe; absolute times are never produced or
   displayed. A DST transition inside a single timer's life is accepted as a
   known one-hour error, once or twice a year.

---

## 4. Architecture

Everything that decides lives in `wispd`. `wisp-hud` draws rows from the
latest snapshot and nothing more. Restarting the HUD loses nothing,
as Spec 1 promises.

### Spell data — `wispd::spells`

Two files in the game install, located from `--log`: the log lives in
`<install>/Logs/`, so the data files are `<install>/spells_us.txt` and
`<install>/spells_us_str.txt`. `--spells <dir>` overrides.

`spells_us.txt`: `^`-separated, 173 fields, 73,975 rows on the current
client. Fields consumed (0-based): `0` id, `1` name, `8` cast time (ms),
`12` buff duration cap (ticks), `28` good_effect (0 = detrimental). Nothing
else is parsed. `spells_us_str.txt`: `^`-separated, header
`#SPELLINDEX^CASTERMETXT^CASTEROTHERTXT^CASTEDMETXT^CASTEDOTHERTXT^SPELLGONE^`;
field `4` is the landing prose printed on the target, with its leading space
(` has been mesmerized.`).

The loader produces one `SpellInfo { id, name, cast_ms, cap_ticks, detrimental,
lands_as: Option<String> }` per spell name that is **eligible**:

- `cap_ticks > 0` (it has a duration), and
- `detrimental`, **or** `lands_as == " looks less aggressive."` (the lull
  family: beneficial in the data, cast on mobs in practice).

Names are **not** unique across eligible rows on the current client: 706 of
them collide (appear on more than one eligible row), 304 of those with
differing capitalisation. When two rows share a name, the lower id wins;
the loader counts the collisions and reports the total once, at startup.
Ranks are not rows in this file: the log prints `Mesmerization VI`, the data
has one `Mesmerization`. The loader is indexed by base name; rank is parsed
off the log line.

The loader is pure over its two inputs and is tested with hand-written rows.
Against the real client it is verified by count and by three known rows (see
§6).

### Rules — `wispd::rules` (extended)

Pure classifiers over one line. All established from the fixture; counts in
the appendix. `<name>` is the log's own rendering of a mob or player name,
including the article (`a jeering gargoyle`, `Guard Drazden`).

| Line | Meaning |
|---|---|
| `You begin casting <Spell>[ <ROMAN>].` | local cast started; rank from the numeral, 0 if absent |
| `Your <Spell> spell fizzles!` | pending cast cancelled |
| `Your <Spell> spell is interrupted.` | pending cast cancelled |
| `<name> resisted your <Spell>!` | pending cast cancelled |
| `<name><lands_as>` | landing, if a pending cast's prose matches |
| `<name> has taken <N> damage from your <Spell>.` | DoT tick: landing for `<Spell>` if pending, heartbeat if active |
| `Your <Spell> spell has worn off of <name>.` | natural end |
| `<name> has been awakened by <who>.` | mez broken |
| `You have slain <name>!` / `<name> has been slain by <who>!` | target dead |
| `You have entered <zone>.` / `LOADING, PLEASE WAIT...` | zone change |

Rank numerals: a trailing token matching `I|II|III|IV|V|VI|VII|VIII|IX|X`
preceded by a space. Names that legitimately end in such a token are the trap
the charter names; the loader's name index decides — `Togor's Insects VI`
parses as `Togor's Insects` + 6 only because `Togor's Insects` is an eligible
name and `Togor's Insects VI` is not.

**Names are compared case-insensitively.** The log capitalises a mob's
article at the start of some sentences and not others: `A Pickclaw guard has
been awakened by Downslap.` but `a jeering gargoyle has been mesmerized.`
(appendix). A row keys its target by the lower-cased name and displays the
form the landing line printed.

The player's own name is not needed: `has been awakened by <anyone>` breaks a
mez whoever did it.

### The state machine — `wispd::timers`

Pure. Consumes classified lines with their log time; produces the list of
active timers on demand. Tested exhaustively; this is where correctness lives.

**Pending casts.** `You begin casting` arms a pending cast for that spell,
replacing any earlier pending cast of the same spell. It lives for
`ceil(cast_ms / 1000) + 6` seconds of log time (one server tick of slack)
from the cast line. A spell the loader does not know is never pending. Fizzle, interrupt and resist retire it. Only a
pending cast can create a timer, so a landing line for someone else's spell
does nothing.

**Landing.** A landing line whose prose matches a pending cast's `lands_as`,
or a DoT tick naming a pending spell, converts the pending cast into an
**active timer** `{ target, spell, rank, landed_at, duration_s, kind,
confidence }`. `kind` is `mez` if `lands_as` is ` has been mesmerized.`,
`dot` if the row was armed by a tick line, else `debuff`; a `debuff` row
becomes `dot` on its first tick line (most DoTs print their prose before their
first tick). The pending
cast is consumed. Four slows share ` yawns.`; the correlation with the pending
cast is what makes the row name the right spell.

**Same-name targets.** Two mobs can print the same name. Each landing makes its
own row. An end signal for a name retires **one** row for that name: the one
with the earliest expiry. This is the log's information limit, stated rather
than hidden.

**Ending.** In priority order for a given target name:

1. `has worn off of <name>` — retires the earliest-expiring row for that
   target and spell; this is a **natural end** and feeds duration learning.
2. `has been awakened by` — retires the earliest-expiring `mez` row for that
   target.
3. slain — retires the earliest-expiring row for that target, of any spell.
4. zone change — retires everything, pending casts included.
5. Expiry — a row whose countdown has passed zero is held for one tick
   (6 s) waiting for its wear-off line, then retired. A `dot` row is held
   open past expiry while tick lines keep arriving: a DoT still ticking has
   been extended by something the log cannot see (JDS300's own prior
   observation, recorded below).

**Re-landing.** A second landing for the same target and spell adds a second
row; the log cannot say whether it was the same mob re-mezzed or a twin. The
overwritten row never gets its own wear-off line and retires at its expiry
hold. In play the overlap is a few seconds; for twins it is correct.

### Durations — `wispd::durations`

Every timer needs a duration at landing time. Two sources, one rule:
**measured wins; the seed fills gaps.**

**Seed.** `round(cap_ticks × 6 × rank_factor)` seconds, where
`rank_factor = (10 + min(rank, 6)) / 10`. This is what the fixture shows
(appendix): rank VI matches the charter's 10%-per-rank rule exactly; ranks
above VI measure at the rank-VI factor, not higher. The seed carries
`confidence: estimated`.

**Measured.** When a row of any kind ends by its own wear-off line (a break,
death or zone change retires a row without one), the interval from the landing
line's timestamp to the wear-off line's timestamp is one **sample** for
`(spell, rank)`. Whole seconds, from the log's own timestamps. Samples shorter
than half the seed are discarded as breaks the log did not narrate. The last
nine samples are kept; the median is the measured duration once three exist,
and a row armed with it carries `confidence: measured`. The last tick of a DoT is not a substitute for its
wear-off line.

The nine-sample window is deliberate: the fixture shows Mesmerization VI
lasting 37–41 s in the early sessions and 19–28 s in the latest ones, on the
same character (appendix). Whatever changed — class, level, an upgrade — the
log cannot say, and a recency window follows it without being told.

Learning runs identically live and under `--from-start`, so replaying an old
log teaches the daemon before the first live session.

**Store.** `$XDG_DATA_HOME/wisp/durations.json` (fallback
`~/.local/share/wisp/`): a map of `"<spell>|<rank>"` to its sample list.
Written atomically (temp file, rename) after each new sample, read at start.
A missing or unreadable store is an empty one. The format is the store's only
contract; it is versioned with a top-level `"v": 1`.

### Protocol v2

`PROTOCOL_VERSION` becomes `2`. `wisp-hud` refuses anything else, as before.

```json
{"v":2,"seq":42,"ts":"Mon Aug 10 20:39:54 2026","lines_ingested":10432,"session_kills":7,
 "timers":[
   {"target":"a jeering gargoyle","spell":"Mesmerization","rank":6,"kind":"mez",
    "remaining_ms":11800,"duration_ms":38000,"confidence":"measured"}
 ]}
```

`timers` is sorted by `remaining_ms` ascending and capped at 16 entries; the
HUD shows at most 8. `remaining_ms` is computed from the estimated log time
at snapshot time (see the timing model) and may be negative during the
post-expiry hold.
Everything Spec 1 carried is unchanged.

### Timing model

The tracker runs on **log time**, in whole seconds: every line carries a
timestamp, and every state transition — pending-cast windows, landings, holds,
expiry — is decided against the timestamp of the line being processed. This
makes a `--from-start` replay of an old log behave exactly as the live session
did, and makes the acceptance replay deterministic.

For display, the daemon estimates the current log time as *the timestamp of
the most recent line, plus the wall-clock time elapsed since that line
arrived* (monotonic clock, at most the 250 ms poll late). `remaining_ms` is
`(expiry − estimated now) × 1000`. Live, this advances smoothly between lines;
under replay it is simply the last line's time. Precision is one second,
which is the log's own.

Rows expire only when a line arrives, because `expire` is a function of the
line being processed, not of the wall clock: during a quiet log (or after a
`--from-start` replay ends) a row already past its expiry keeps being
published, with `remaining_ms` clamped by the HUD to 0, until the next line
gives the tracker something to expire it against — and in play, the wear-off
line is itself that next line.

Snapshots are published at the existing 250 ms tick. Log timestamps are never
mixed with the monotonic clock except in that one estimate, and never
converted to absolute time.

### `wisp-hud`

Below the kill count, one line per timer, soonest first, at most 8:

```
7 kills
a jeering gargoyle   Mesmerization VI   12
Guard Drazden        Pacify V           48
an ice giant         Togor's Insects    131
```

Columns are padded to fixed widths in the monospace face so rows do not
reflow. The renderer takes one colour per line, so the **whole row** is
coloured by its remaining seconds, not just the seconds column, in this
precedence: **critical colour at ≤ 5 s, else warning colour at ≤ 10 s, else
a dimmer shade for an `estimated` row** the daemon hasn't verified yet,
**else white** — Wisp's own thresholds, constants in one place. An
estimated row inside the warning or critical window shows in that urgency
colour, not the dim one; the dim shade only applies once neither threshold
is crossed.

The HUD draws what the latest snapshot says and nothing more: at four
snapshots a second and a one-second display resolution there is nothing to
interpolate. The overlay window is sized at attach for the kill
line plus 8 rows at the chosen scale, on every backend. Nothing else in the
renderer changes.

---

## 5. Milestones

| # | Milestone | Verified by |
|---|---|---|
| 1 | `spells` loader | Row count and three known rows against the real client files; hand-written fixture rows in tests |
| 2 | `rules` + `timers` + `durations` | Fixture replay under `--from-start` reproduces the counts in §6 exactly; unit tests for every transition |
| 3 | Protocol v2 + HUD rows | `wispd --stub` emits synthetic timers; rows draw and count down on the desktop |
| 4 | Live | A mez in play: row appears on landing, reaches zero within 1 s of the wear-off line, disappears; a broken mez disappears on the awaken line (verified 2026-09-08, mez row/thresholds) |

Milestone 4 is the one that matters, and Milestone 2's replay is what makes it
likely to pass first time.

---

## 6. Acceptance criteria

Exact, not impressionistic.

**Loader**, against the current client files:

| Value | Expected |
|---|---|
| rows parsed from `spells_us.txt` | `73975` |
| `Mesmerization` | id `307`, cap `4` ticks, detrimental, lands as ` has been mesmerized.` |
| `Pacify` | id `45`, cap `7` ticks, beneficial, lands as ` looks less aggressive.`, eligible via the lull rule |
| `Togor's Insects` | id `507`, cap `35` ticks, detrimental, lands as ` yawns.` |

**Replay**, against the **frozen fixture** and the real client files. The
live log grows while JDS300 plays, so the fixture is its first 1,440,036 lines
frozen on 2026-09-08 at
`/mnt/Data4TB/Games/everquest/fixtures/eqlog_Daggo_freeport.1440036.txt`
(SHA-256 begins `70a95ca40bc701cf`; Spec 1's counts hold on it). The tracker's
final counters must **equal** the numbers below, produced by an independent
reference implementation of §4 (recorded in the plan) before any Rust was
written, so the implementation is checked against numbers it did not produce.

| Counter | Expected |
|---|---|
| eligible spells loaded | `12245` |
| pending casts armed / cancelled / expired | `8042` / `730` / `399` |
| timers armed: total / mez / dot / debuff | `5972` / `822` / `214` / `4936` |
| rows promoted debuff→dot on first tick | `2097` |
| tick heartbeats on active rows | `12814` |
| ended by wear-off / awaken / death / expiry hold | `1660` / `19` / `2042` / `2107` |
| rows cleared by zone change | `144` |
| samples recorded / discarded as short | `1060` / `600` |
| active rows and pending casts at end of file | `0` / `0` |

Learned durations at end of replay (median of the last nine samples):
`Mesmerization|6` = `24` (samples `22 27 21 27 19 28 24 22 28`),
`Pacify|5` = `69`, `Venom of the Snake|0` = `38`, `Envenomed Bolt|10` = `57`,
`Odium|10` = `50`.

**Live.**

- A mez cast in play produces a row on the landing line and the row reaches
  zero within 1 s of `Your Mesmerization spell has worn off of <name>.`
- Breaking a mez by damage removes the row on the awaken line.
- Killing a mob removes its rows.
- Zoning clears the list.
- Killing and restarting `wisp-hud` mid-fight shows the same rows with the same
  remaining time, within one second.
- The HUD still never takes focus or input on any backend.

---

## 7. Risks

| Risk | Standing |
|---|---|
| **Rank scaling above VI is an observation, not a published rule.** Odium X and Envenomed Bolt X measure at the rank-VI factor; only four spells were checked. | Accepted. Learned durations make the seed's error temporary: wrong for the first few casts of a spell, then measured. |
| **Prose collisions.** Four slows share ` yawns.`; ` has been poisoned.` is shared by several DoTs. | Handled by design: only a pending local cast can arm a row, and it names the spell. Two different pending spells with the same prose landing within one window is the residual case; the earlier pending cast wins. |
| **Same-name mobs.** The log cannot tell two `a flouting gargoyle` apart. | Stated limit: one row per landing, an end signal retires the earliest expiry. Wrong at most by which of two identical rows disappears. |
| **`spells_us.txt` is 38 MB.** Parsing it at every daemon start costs time. | Parse only five fields per row; measure in Milestone 1. If it exceeds one second in release, cache the eligible subset under `$XDG_CACHE_HOME/wisp/` keyed by the file's size and mtime. Not a spec decision until measured. |
| **The client updates and the format shifts.** The field layout was reverse-engineered by a third party. | The loader fails loudly on a header or field-count mismatch rather than reading garbage, and names the file. |
| **Multi-class levels.** The log's `Welcome to level N!` cannot say which class levelled. | Not used. The seed is the cap; learning does the rest. This is why level is a non-goal. |
| **Spell ranks above X.** The log prints bare roman numerals and the client currently upgrades to X; the parser's numeral table stops at X. | Accepted. A rank-XI cast would be silently ignored (no pending cast, no row). When EverQuest Legends ships a rank above X, extend the table in `rules.rs` and add the numeral to the rank test; nothing else changes. |

---

## Appendix — verified facts

All read from primary sources on 2026-09-08, per the charter: the reference
fixture (`eqlog_Daggo_freeport.txt`, 1,440,036 lines) and the EverQuest
Legends client files in JDS300's install. Nothing here is inferred from
another parser.

**Line counts on the frozen fixture.**

| Pattern | Count |
|---|---|
| `You begin casting ` | 17,131 |
| ` spell has worn off of ` (own, spell named, target named) | 3,055 |
| ` has been mesmerized.` | 2,730 |
| ` has been awakened by ` | 775 |
| ` has taken N damage from your ` (own DoT ticks) | 21,660 |
| ` resisted your ` | 1,814 |
| `Your … spell is interrupted.` | 801 |
| `Your … spell fizzles!` | 15 |
| ` yawns.` | 790 |
| ` looks less aggressive.` (Pacify landing) | present; follows `You begin casting Pacify V` |
| ` has been charmed.` | 284 |
| `You have entered ` | 494 |
| `LOADING, PLEASE WAIT` | 440 |
| `You have gained a level! Welcome to level N!` | 96 |

**Wear-off lines strip the rank.** Cast: `You begin casting Mesmerization VI.`
Wear-off: `Your Mesmerization spell has worn off of a jeering gargoyle.`

**Article capitalisation depends on the sentence.** In the fixture, ` has
been awakened by ` (534 of 775), ` has taken N damage from your ` (all 9,261
with an article) and ` has been slain by ` (all 1,893) print `A …`; ` has
been mesmerized.` (all 1,971), ` yawns.` (all 349), `spell has worn off of `
(all 2,009) and `You have slain ` (all 2,936) print `a …`. Hence
case-insensitive target keys.

**Duration drift on one character.** Pairing landings to wear-offs by the §4
rules, Mesmerization VI's natural expiries cluster at 37–45 s in the August 10
sessions and at 19–28 s in the latest sessions of the fixture; the last nine
samples give a median of 24 s. The seed rule would say 38 s. Learning with a
recency window is what makes the countdown right in both periods.

**Measured natural expiry, same target, whole seconds (August 10 sessions).**
Mesmerization VI's cluster is measured landing to wear-off; the other four
rows are measured cast start to wear-off (cast time included), as each row's
"Measured cluster" cell says.

| Spell, rank | Client cap | Seed by the §4 rule | Measured cluster |
|---|---|---|---|
| Mesmerization VI | 4 ticks = 24 s | × 1.6 = 38.4 s | 37–41 s (n ≈ 130 at the cluster; 861 pairs total, the rest are breaks) |
| Pacify V | 7 ticks = 42 s | × 1.5 = 63 s | 66–74 s from cast start (≈ 3 s cast time included) |
| Venom of the Snake (no rank) | 6 ticks = 36 s | × 1.0 = 36 s | 39–43 s from cast start |
| Envenomed Bolt X | 6 ticks = 36 s | × 1.6 = 57.6 s | 55–60 s from cast start |
| Odium X | 5 ticks = 30 s | × 1.6 = 48 s | 50–54 s from cast start |
| Odium VII | 5 ticks = 30 s | × 1.6 = 48 s (rank VII, factor capped) | 45–48 s from cast start |

A flat 10%-per-rank factor would predict 72 s and 60 s for the two rank-X
spells; the log says otherwise. Hence `min(rank, 6)`.

**Client data files.** `spells_us.txt`: 73,975 rows, 173 `^`-separated
fields, 38,211,219 bytes. `spells_us_str.txt`: header
`#SPELLINDEX^CASTERMETXT^CASTEROTHERTXT^CASTEDMETXT^CASTEDOTHERTXT^SPELLGONE^`.
Row 307 `Mesmerization`: cap 4, formula 1. Row 45 `Pacify`: cap 7, formula 8,
beneficial, cast-on-other ` looks less aggressive.`, spell-gone
`You feel your aggression subside.` on the target's own screen. Row 507
`Togor's Insects`: cap 35, formula 6. Row 435 `Venom of the Snake`: cap 6.
Ranks are not rows; `Mesmerization` has exactly one row.

**Field layout** was read from `SPELL_FORMAT.md` in `amerzel/eql-info` (MIT,
recorded in `THIRD_PARTY.md`), then confirmed against the local file by
reading the rows above. Wisp uses the layout, not that project's parser or
database.

**Multi-class levelling.** The fixture's level-up lines run 48, 49, 50 and
then 12 on the same character. EverQuest Legends levels classes independently
on one character; the log does not say which class a level belongs to.

**Prior observation, JDS300's own (Tier B, `debuff_timer.py`, behaviour
only):** slows and resist debuffs print prose that names the target but not
the spell, and the same prose is printed for other casters, so a landing is
accepted only while a compatible local cast is pending; a DoT still ticking
past its computed expiry has been extended by a focus item and its row is held
open. Both behaviours are carried into §4. No code, structure or naming was
taken.
