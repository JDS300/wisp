# Spec 3 — encounters

**Status:** design approved 2026-09-08; written spec reviewed by JDS300 pending
**Depends on:** [Spec 2 — timers](2026-09-08-spec-2-timers.md), [Spec 1 — the spine](2026-09-08-spec-1-the-spine.md), [Spec 0 — clean-room charter](2026-09-08-clean-room-charter.md)
**Target client:** EverQuest Legends. Not Live, not Project Quarm.

---

## 1. What this spec is for

The complete combat parser: one encounter model behind five views. For the
current fight the overlay shows your damage per second, the damage you are
taking per second, your healing per second, and ranked rows for every
player's damage and healing. All five are queries over one ledger of events
that the log already prints with a source, a target and an amount.

**Done means:** during a fight on JDS300's desktop the personal line moves as
he hits, is hit and heals; the group rows rank his groupmates; ten seconds
after the last blow the panel holds its final numbers for half a minute and
then clears; and a replay of the frozen fixture reproduces the reference
numbers in §6 exactly.

---

## 2. Non-goals

Explicitly deferred, so their absence is a decision rather than an oversight:

- Session-wide totals and per-hour rates — later. Spec 3 reports the current
  fight only.
- Per-mob, per-ability and per-spell breakdowns — later. The ledger keeps what
  it needs for them; the overlay does not show them.
- Damage taken by other players, healing received as a separate view, and
  overheal per row on the overlay — the numbers are tracked, not drawn.
- Misses, dodges, parries, ripostes, crit rates, accuracy — not parsed. Only
  lines that carry an amount matter to a meter.
- Ownership of other players' summoned pets. The fixture has no `My leader is`
  line, so an unowned pet is its own row under its own name. Warders and
  pets printed as `<Owner>\`s warder` / `<Owner>\`s pet` are attributed.
- Any control on the overlay. Nothing toggles; see §3.
- Log auto-discovery — still `--log <path>`. Bounded task, later.

---

## 3. Invariants

Spec 1's invariant stands: **the HUD never takes input, on any backend,
ever.** Spec 2's two rules stand: game data is read, never shipped (this spec
reads no new game data), and naive timestamps are subtracted, never
relabelled.

One rule is new and binding:

- **Only lines that carry an amount count.** A meter is a sum of numbers the
  game printed. Nothing is inferred, estimated, or carried from a spell
  table. Overheal is the difference between the two numbers a heal line
  prints and nothing else.

---

## 4. Architecture

Everything that decides lives in `wispd`. `wisp-hud` draws the latest
snapshot.

### Events — `wispd::combat`

Pure classifier over one line body, producing at most one event. Every shape
was read from the fixture; counts in the appendix. `<N>` is an integer;
suffixes such as ` (Critical)`, ` (Riposte)`, ` (Finishing Blow)` follow the
full stop and are ignored.

| Event | Line |
|---|---|
| your melee | `You <verb> <target> for <N> points of damage.` — any verb, including a plain `hit` with no spell |
| your spell | `You hit <target> for <N> points of <type> damage by <Spell>.` |
| your DoT tick | `<target> has taken <N> damage from your <Spell>.` |
| your damage shield | `<target> is <verb> by YOUR <thing> for <N> points of non-melee damage.` |
| other melee | `<source> <verb>s <target> for <N> points of damage.` |
| other spell | `<source> hit <target> for <N> points of <type> damage by <Spell>.` |
| other DoT tick | `<target> has taken <N> damage from <Spell> by <source>.` |
| other damage shield | `<target> is <verb> by <source>'s <thing> for <N> points of non-melee damage.` |
| damage shield on you | `YOU are <verb> by <source>'s <thing> for <N> points of non-melee damage!` |
| melee on you | `<source> <verb>s YOU for <N> points of damage.` |
| special attack on you | `<source> <verb>s on YOU for <N> points of damage.` |
| spell on you | `<source> hit you for <N> points of <type> damage by <Spell>.` |
| DoT on you | `You have taken <N> damage from <Spell> by <source>.` |
| your heal | `You healed <target>[ over time] for <N>[ (<M>)] hit points by <Spell>.` |
| other heal | `<source> healed <target>[ over time] for <N>[ (<M>)] hit points[ by <Spell>].` |
| pet announcement | `<pet> tells you, 'Attacking <target> Master.'` (also `told you`) |
| boundary | `You have entered …`, `LOADING, PLEASE WAIT…`, `Welcome to EverQuest Legends!` |

A sentence of the form `<target> has taken <N> damage by <Spell>.` — note
"damage **by**", with no "from" and no source at all — carries no source and
is ignored by decision: nothing to attribute it to, so it is not an event.

A heal's `<N>` is the amount actually healed and `<M>`, when printed, the
amount the spell could have healed; overheal is `M − N`, zero when `M` is
absent. `<target>` in a heal may be `you`, `yourself`, `himself`, `herself`,
`itself`, or a name; the target is recorded and not otherwise used by this
spec.

The classifier tries the "on you" shapes first (they name `YOU`/`you`
explicitly), then your own shapes, then heals, then other sources' shapes,
in the order of the table; that order is what the reference implementation
does.

### Actors — `wispd::combat`

Who a name is, decided per line from the name's printed form and what the log
has shown so far. Names are compared case-insensitively: the log capitalises
a mob's article at the start of a sentence (Spec 2, appendix).

1. `You`, `YOUR`, `you`, `your`, and the player's own name (from the log
   filename `eqlog_<Name>_<server>.txt`) are **you**.
2. A name that announced `Attacking … Master.` to you within the last
   **120 s** is **your pet**, and its damage and healing are yours. The
   announcement repeats on every attack command, so a pet stays yours while
   it fights and lapses two minutes after it stops — which is what happens
   when a charm breaks and the mob turns on you.
3. `<Owner>\`s warder` and `<Owner>\`s pet` belong to **Owner**; if Owner is
   you, rule 2 applies.
4. A name that starts with `a `, `an ` or `the `, or contains a space, or has
   ever damaged you, or has ever been the target of damage from you or from
   a player (a melee line whose source is not a mob; a spell line whose
   source is a plain single word or an owned pet — the reference
   implementation's exact marking rule), is a **mob**. The last clauses catch
   single-word named NPCs (`Xicotl`) once they have been in a fight.
5. Anything else is a **player**.

Damage from a mob never enters the damage rows; damage to you from anything
enters damage taken. A player hitting a player (a duel, a charmed
groupmate) counts as that player's damage: the log cannot tell it from a hit
on a single-word named mob, and it is rare.

**Group.** The log never lists who was already in your group before you
joined it; membership is learned only from lines that prove it, and forgotten
on any line that resets it:

| Meaning | Line |
|---|---|
| member | `<Name> has joined the group.` |
| member | `<Name> invites you to join a group.` |
| member | `You notify <Name> that you agree to join the group.` |
| member | `<Name> is now the leader of your group.` |
| member | `<Name> is now group Main Assist` |
| member | `<Name> tells the group, '…'` |
| leave | `<Name> has left the group.` |
| leave | `<Name> has been removed from the group.` |
| reset | `You have joined the group.` / `You have been removed from the group.` / `You have left the group.` / `Your group has been disbanded.` |

`You are now the leader of your group.` names nobody and is not a member
line. A group row shows a name only once one of the member lines has proven
it; you are never a row, since your own numbers are the personal line; and
with nobody proven, there are no group rows at all.

### Encounters — `wispd::encounter`

A **combat session**: it starts on the first damage line, dealt or taken,
that arrives while no fight is open; it ends when **10 s** of log time pass
with no damage or heal line, or on a boundary line. Heals do not start a
fight and heals outside a fight are not counted.

Inside a fight, per source: damage dealt (you, your pets merged in, each
player, each attributed pet under its owner), healing done, overheal; and for
you alone, damage taken. Duration is last damage-or-heal line minus first
damage line, at least 1 s. Rates are amount divided by duration.

A finished fight **lingers for 30 s** of estimated log time (Spec 2's model:
the last line's timestamp plus wall-clock elapsed) so the result can be read,
then the panel clears. A new fight replaces a lingering one at once.

### Protocol v3

`PROTOCOL_VERSION` becomes `3`. `wisp-hud` refuses anything else. Everything
Spec 1 and Spec 2 carried is unchanged; the snapshot gains one optional
block:

```json
"encounter": {
  "active": true, "duration_s": 42,
  "you": {"damage": 18234, "dps": 434, "taken": 2210, "taken_ps": 52,
          "healing": 900, "hps": 21, "overheal": 120},
  "damage":  [{"name": "Serenitee", "amount": 12010, "per_s": 286, "is_you": false}],
  "healing": [{"name": "Misery", "amount": 3100, "per_s": 74, "is_you": false}]
}
```

`encounter` is `null` when no fight is open or lingering. `damage` holds at
most 5 rows and `healing` at most 3, ranked by amount, restricted to sources
the log has proven are in your group (§4 Actors, Group); you are never a row,
since your own numbers are the `you` block. `is_you` stays in the wire format
so this is not a version bump, but a group row can never set it: it is always
`false`. Rates are integers, rounded.

### `wisp-hud`

Below the kill count and above the timer rows, one **personal line**; below
the timer rows, the **group rows** — group members only, you excluded:

```
7 kills
DPS  434  in  52/s  HPS  21   0:42
a jeering gargoyle   Mesmerization VI   12
Serenitee      12.0k  286/s
Misery          3100   74/s  +
```

Amounts print as integers below 10,000, as `12.3k` below 1,000,000, else as
`1.32M`. Healing rows carry a trailing ` +` so they read
differently from damage rows in the same list (a plain glyph the vendored
monospace face is certain to have); your rows draw in a distinct
colour; the personal line draws white while the fight is active and dimmed
while it lingers. Columns are fixed width in the monospace face. The window
is sized at attach for the kill line, the personal line, 8 timer rows, 5
damage rows and 3 healing rows.

When `encounter` is `null` the personal line reads
`DPS     -  in    -/s  HPS    -   -:--` and no group rows are drawn, so the
layout does not jump.

---

## 5. Milestones

| # | Milestone | Verified by |
|---|---|---|
| 1 | `combat` classifier and actor rules | Unit tests from fixture lines; counts in §6 |
| 2 | `encounter` sessions | Fixture replay reproduces §6 exactly |
| 3 | Protocol v3 + HUD panel | `wispd --stub` emits a synthetic fight; panel draws and lingers |
| 4 | Live | A fight in play on the desktop: personal line moves, group rows rank, linger and clear |

---

## 6. Acceptance criteria

Exact, not impressionistic.

**Replay**, against the frozen fixture
(`/mnt/Data4TB/Games/everquest/fixtures/eqlog_Daggo_freeport.1440036.txt`,
SHA-256 begins `70a95ca40bc701cf`) with the rules of §4, as produced by the
reference implementation recorded in the plan before any Rust was written,
**re-derived on 2026-09-09 after the whole-branch final review found three
line shapes it had missed** (deterministic; two runs identical each time):

| Counter | Expected |
|---|---|
| encounters | `2524` |
| damage dealt in fights by non-mob sources: melee / spell / DoT / damage shield | `15198942` / `9082968` / `4622907` / `447208` |
| your damage in fights: by you / by your pets / total | `18085260` / `5284285` / `23369545` |
| damage taken by you in fights: melee / spell / DoT / damage shield | `1924867` / `709376` / `273054` / `257300` |
| healing in fights by non-mob sources: actual / overheal | `2364526` / `838539` |
| your healing in fights: actual / overheal / of which HoT ticks | `1875603` / `590911` / `355478` |
| healing outside any fight (not counted) | `259121` |
| pet announcements seen / distinct pet names | `4890` / `91` |
| boundary lines that closed or found no fight | `971` |
| fight duration: min / median / max / sum (s) | `1` / `36` / `738` / `146813` |
| largest fight by your damage: damage / duration / DPS / taken | `212467` / `482` / `441` / `21416` |
| top damage sources overall (amount desc, name asc) | `you 23369545`, `Yder 1447129`, `Serenitee 1321322`, `Misery 1000700` |
| top healers overall after you | `Serenitee 196052`, `Misery 116859` |
| group membership lines seen / leaves / resets | `40` / `4` / `7` |
| group membership at end of file | empty |

**Live.**

- During a fight the personal line updates within a second of each hit.
- Group rows show groupmates ranked by damage.
- Nobody outside the group appears in the rows, and you are never one of
  them — your numbers are the personal line above them.
- Ten seconds after the last damage or heal the panel keeps its final numbers
  for 30 s, then clears to the empty form.
- Zoning clears the panel at once.
- Killing and restarting `wisp-hud` mid-fight shows the same numbers.
- The HUD still never takes focus or input on any backend.

---

## 7. Risks

| Risk | Standing |
|---|---|
| **Single-word named NPCs read as players** until they have hit you or been hit by you (`Xicotl`). | Accepted. A named boss that only fights your groupmates appears as a damage row briefly; the mob set closes the gap as soon as it touches you. |
| **Other players' summoned pets have no owner in the log.** | Accepted and stated in §2. They are rows under their own names. |
| **A charmed pet's damage after the charm breaks** is still yours for up to 120 s. | Accepted; bounded by the TTL. The `has been charmed` and `Charm spell has worn off` lines could tighten it later. |
| **The 10 s idle timeout splits a slow fight** (a caster kiting) and merges back-to-back pulls. | Accepted; it is what meters in this genre do. The value is one constant. |
| **A single-word named NPC's `on`-preposition special attack** (`<mob> <verb>s on <target> for N points of damage.`) is credited as a player's damage-out row until the mob has fought you directly — the `on` shape is exactly as exposed to the first risk above as ordinary melee is; it does not close that gap. Accepted; verified in the fixture, not merely theoretical: `Doreme frenzies on a haunted chest for 8 points of damage.` (Tue Aug 18 21:19:46) is credited to a "Doreme" row until `You strike Doreme` first marks it a mob, almost an hour later in log time. |
| **The reference was re-derived after the whole-branch final review** found three line shapes it had missed (other sources' DoT ticks print `from <Spell> by <source>`, not the assumed `from <source>'s <Spell>`; a damage shield can land on you; a special attack can carry an `on` preposition before `YOU`). The §6 numbers moved: damage taken by you (melee+spell+dot+shield) rose from 2,900,237 to 3,164,597 (+9.1%, the new `taken_shield` category alone); other-sourced DoT damage counted in fights rose from 141,014 to 330,056 as four phantom players (Tuyen, Selo, Denon, Oathbreaker) — credited by a possessive split that matched apostrophes inside bard song names — were replaced by their real sources. | Standing. The numbers above are the acceptance criteria from 2026-09-09 forward; the plan's Appendix A carries the corrected script and its output. |
| **Members present in the group before you joined it are unknown until they act.** The log has no roster line — it never lists who was already there, only who joins, is invited, leads, is Main Assist, talks on group chat, leaves, or is removed afterward. | Accepted. A quiet groupmate who never does one of those things never gets a row; this is a limit of the log, not a bug found live (JDS300, 2026-09-09; see PROVENANCE.md). |
| **The panel is tall**: 18 lines at the default scale is about 1,100 px. | Accepted for Spec 3; `--scale 32` fits comfortably. Layout is Spec 4's problem. |
| **Overheal is only known when the game prints two numbers.** Lines without `(M)` are taken as zero overheal. | Accepted; the number printed is the number counted (§3). |

---

## Appendix — verified facts

All read from primary sources on 2026-09-08: the frozen fixture and the log's
own phrasing. Counts are `grep -c` over the frozen fixture with fixed
strings; the fixture is CRLF, so no pattern is anchored at end of line.

**Line shapes and counts.**

| Shape | Count |
|---|---|
| `You <verb> X for N points of damage.` (own melee) | 88,357 |
| own melee verbs seen, most common first | slash 37,406 · pierce 18,642 · bash 6,989 · claw 6,097 · kick 5,069 · punch 4,964 · strike 4,163 · smite 3,275 · hit 1,068 · cleave 684 |
| `You hit X for N points of <type> damage by S.` (own spell) | 19,091 |
| damage types seen | magic 14,012 · cold 1,668 · prismatic 1,347 · poison 1,076 · fire 626 · disease 329 · unresistable 33 |
| `X has taken N damage from your S.` (own DoT) | 21,660 |
| `X is <verb> by YOUR <thing> for N points of non-melee damage.` (own damage shield) | 31,549 (tormented 28,079 · pierced 2,652 · burned 812) |
| suffixes on own damage lines | (Critical) 11,506 · (Riposte) 818 · (Finishing Blow) 545 · (Slay Undead) 367 · (Riposte Critical) 99 · (Crippling Blow) 47, among others (fourteen forms in the fixture) |
| `<Name> <verb>s X for N points of damage.` (other melee) | 142,648 |
| `<Name> hit X for N points of <type> damage by S.` (other spell) | 34,725 |
| `X has taken N damage from <Spell> by <Name>.` (other DoT; corrected in the final-review wave — the possessive shape `from <Name>'s <Spell>.` matched only 1,720 lines, an apostrophe accident, and credited four phantom players) | 11,614 |
| `X has taken N damage by <Spell>.` (no source at all; ignored by decision) | 1,192 |
| `YOU are <verb> by <Name>'s <thing> for N points of non-melee damage!` (damage shield on you) | 19,077 |
| `` <Owner>`s warder `` / `` <Owner>`s pet `` as a source | 8,624 |
| `X <verb>s YOU for N points of damage.` | 50,949 |
| `X <verb>s on YOU for N points of damage.` (special attack with a preposition) | 245 |
| `X hit you for N points of <type> damage by S.` | 4,793 |
| `You have taken N damage from S by X.` | 5,994 |
| `You healed X for N[ (M)] hit points by S.` | 14,085 |
| `You healed X over time for N hit points by S.` | 2,135 |
| `<Name> healed X for N[ (M)] hit points[ by S].` | 65,038 |
| heals carrying the `(M)` potential | 35,748 |
| `<Name> healed you for …` | 393 |
| self-heals (`himself`/`herself`/`itself`/`yourself`) | 63,036 |
| `<pet> (tells\|told) you, 'Attacking X Master.'` | 4,890 |
| other `… Master.` tells (`unable to wake`, `calming down`) | 2,224 |
| `You have been slain by X!` | 32 |
| `You gain experience!` / `You gain party experience!` | 3,434 / 819 |

**Your own pets in the fixture** are mostly charmed mobs and print with their
article: `A revultant rat told you, 'Attacking an abhorrent Master.'`; 91
distinct names announced.

**Overheal form.** `You healed Vibtik for 128 (197) hit points by Valor.` —
128 landed, 197 was the spell's amount. `Cauli healed himself for 0 (229) hit
points by Armor of Protection.` — a full overheal.

**No `My leader is` line** exists in the fixture. **No `X died.` line** is
needed: deaths are `You have slain X!`, `X has been slain by Y!`, and
`You have been slain by X!`.

**Consultations**, recorded in `PROVENANCE.md`: Loremaster (Tier C,
behaviour only) — pets announce `Attacking … Master`, summoned pets have
one-word names and charmed creatures keep theirs, a `My leader is` reply
identifies an owner when available; EQBuddy (README, product only) — learns
the pet's name from Master messages, counts pet kills as the player's,
shows a charmed pet as provisional until a Master message confirms it. Wisp's
rules above were then derived from the fixture; no expression was taken.
