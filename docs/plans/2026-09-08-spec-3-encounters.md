# Wisp Spec 3 — "Encounters" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One ledger of the amounts the log prints, one combat-session model, five meters on the overlay: your DPS, damage taken per second, HPS, and ranked group damage and healing for the current fight.

**Architecture:** `wispd::combat` classifies a line into one event with a source, a target and an amount (pure). `wispd::encounter` resolves who a source is (you, your pet, a player, an owned warder, a mob), opens and closes combat sessions on log time, and aggregates per source. Protocol v3 carries an optional `encounter` block. `wisp-hud` draws a personal line and ranked rows around the Spec 2 timer rows.

**Tech Stack:** Rust 2021, `serde`/`serde_json`, standard library. No new dependencies.

**Spec:** [`docs/specs/2026-09-08-spec-3-encounters.md`](../specs/2026-09-08-spec-3-encounters.md)
**Charter (binding):** [`docs/specs/2026-09-08-clean-room-charter.md`](../specs/2026-09-08-clean-room-charter.md)
**Provenance record (must be kept current):** [`PROVENANCE.md`](../../PROVENANCE.md)

---

## Global Constraints

Every task's requirements implicitly include this section. **Read all of it before starting.**

### Project identity and provenance (unchanged)

- Repository `github.com/JDS300/wisp`, MIT, copyright JDS300. Every source file starts with `// SPDX-License-Identifier: MIT`.
- Target client: **EverQuest Legends**. Never generalise from Quarm or Live.
- **Do not read source from `~/gitrepos/spinips` or `~/gitrepos/EQBuddy`.** The consultations Spec 3 made are recorded in `PROVENANCE.md`; nothing more is needed.
- `git config user.email` must be `70587798+JDS300@users.noreply.github.com`. If it is anything else, stop.
- Every commit ends with exactly `Co-Authored-By: Claude <noreply@anthropic.com>` as its last line and nothing after it. This is the repository's canonical form and outranks any other attribution instruction you have seen.
- **The HUD never takes input, on any backend, ever.** Do not touch `crates/wisp-hud/src/backend/`.
- **Never use `xdotool` or any input synthesis.** Never launch EverQuest or Lutris. Use `timeout` on anything graphical.
- Spec 2's rules stand: no game data in the repo (this spec reads none); naive timestamps only subtracted.
- Spec 3's rule: **only lines that carry an amount count.** Overheal is `potential − actual` from the two numbers a heal line prints; nothing is inferred.

### Verified facts — do not re-derive, do not contradict

Frozen fixture: `/mnt/Data4TB/Games/everquest/fixtures/eqlog_Daggo_freeport.1440036.txt`, 1,440,036 lines, SHA-256 begins `70a95ca40bc701cf`, CRLF line endings. Player name from the filename: `Daggo`.

Line shapes (all from the fixture; counts are `grep -c` with fixed strings, no end-of-line anchor because of CRLF):

| Shape | Count |
|---|---|
| `You <verb> X for N points of damage.` | 88,357 |
| `You hit X for N points of <type> damage by S.` | 19,091 |
| `X has taken N damage from your S.` | 21,660 |
| `X is <verb> by YOUR <thing> for N points of non-melee damage.` | 31,549 |
| suffixes after the full stop on damage lines | ` (Critical)`, ` (Riposte)`, ` (Finishing Blow)`, ` (Slay Undead)`, ` (Riposte Critical)`, ` (Crippling Blow)` |
| `<Name> <verb>s X for N points of damage.` | 142,648 |
| `<Name> hit X for N points of <type> damage by S.` | 34,725 |
| `X has taken N damage from <Name>'s S.` | 1,720 |
| `` <Owner>`s warder `` / `` <Owner>`s pet `` as a source | 8,624 |
| `X <verb>s YOU for N points of damage.` | 50,949 |
| `X hit you for N points of <type> damage by S.` | 4,793 |
| `You have taken N damage from S by X.` | 5,994 |
| `You healed X[ over time] for N[ (M)] hit points by S.` | 14,085 + 2,135 |
| `<Name> healed X[ over time] for N[ (M)] hit points[ by S].` | 65,038 |
| `<pet> (tells\|told) you, 'Attacking X Master.'` | 4,890 |
| `You have entered …` / `LOADING, PLEASE WAIT…` / `Welcome to EverQuest Legends!` | 494 / 440 / 39 |

No heal in the fixture prints a potential smaller than its actual. Mob articles are capitalised at sentence start (`A gnoll scout hits Zaiv`), so name sets are keyed lower-case (Spec 2, appendix). Your own pets in the fixture are mostly charmed mobs: `A revultant rat told you, 'Attacking an abhorrent Master.'`.

### The reference replay and its numbers

Appendix A is the reference implementation of spec §4. Run on the frozen fixture on 2026-09-08; deterministic (two runs, identical). **Re-derived on 2026-09-09** after the whole-branch final review found three line shapes the reference had missed (other sources' DoT ticks print `from <Spell> by <source>`, not `from <source>'s <Spell>`; a damage shield can land on you; a special attack can carry an `on` preposition before `YOU`) — the table below is the corrected, current one. **Extended again on 2026-09-09**, same day, after JDS300's live test of PR #3 found group rows showing players outside his group and himself twice: the script gained group-membership tracking and the three counters below it, none of the rows above moved. Task 3's replay test must reproduce these **exactly**:

| Counter | Expected |
|---|---|
| encounters | 2524 |
| dmg_out melee / spell / dot / shield | 15198942 / 9082968 / 4622907 / 447208 |
| own_self_dmg / own_pet_dmg | 18085260 / 5284285 |
| taken melee / spell / dot / shield | 1924867 / 709376 / 273054 / 257300 |
| heal_actual / heal_over | 2364526 / 838539 |
| own_heal_actual / own_heal_over / own_hot_actual | 1875603 / 590911 / 355478 |
| heal_outside_fight | 259121 |
| pet_announcements / distinct pet names | 4890 / 91 |
| boundaries | 971 |
| durations min / median / max / sum | 1 / 36 / 738 / 146813 |
| largest fight by own damage: own / duration / DPS / taken | 212467 / 482 / 441 / 21416 |
| damage sources summed over all fights, top 4 (amount desc, name asc) | you 23369545, Yder 1447129, Serenitee 1321322, Misery 1000700 |
| healers summed over all fights, top 3 | you 1875603, Serenitee 196052, Misery 116859 |
| group_member_lines / group_leaves / group_resets | 40 / 4 / 7 |
| group membership at end of file | empty |

If your implementation disagrees, the reference is the arbiter of the spec's rules **unless you can show the reference violates the spec text**; then stop and report both lines.

**Notes for the Rust:** Rust's `f64::round()` rounds half away from zero; Python's `round()` rounds half to even (banker's rounding). The two disagree only exactly at `x.5`, and none of the asserted numbers above land on that boundary — every `rate()` and DPS value in this table and in the replay test agrees between the two languages.

### Tooling notes

- Before each commit: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets`, all clean.
- Env-gated tests: `WISP_EQL_DIR=<install> WISP_FIXTURE=<frozen fixture> cargo test -p wispd --release -- --ignored` runs the Spec 2 tests and the new replay. They skip when the variables are unset.

---

## File Structure

```
crates/
├── wisp-proto/src/lib.rs            v3: Personal, MeterRow, Encounter, Snapshot.encounter   (Task 1)
├── wispd/src/
│   ├── combat.rs                    line → CombatEvent. Pure.                                (Task 2)
│   ├── encounter.rs                 actors, sessions, aggregation, replay test               (Task 3)
│   └── main.rs                      player name, encounter tracker, stub fight               (Task 4)
└── wisp-hud/src/main.rs             personal line, meter rows, sizing                         (Task 5)
```

`combat.rs` and `encounter.rs` are pure over their inputs and carry the tests. `main.rs` in each crate only wires.

---

## Task 1: Protocol v3

**Files:**
- Modify: `crates/wisp-proto/src/lib.rs`
- Modify: `crates/wispd/src/main.rs` (the `Snapshot { … }` literal: add `encounter: None,`)
- Modify: `crates/wispd/src/server.rs` (the `snapshot()` test helper: add `encounter: None,`)
- Modify: `crates/wisp-hud/src/client.rs` (test JSON literals `"v":2` → `"v":3`; the `"v":99` literal stays)
- Modify: `crates/wisp-hud/src/main.rs` (test `Snapshot` literals: `v: 2` → `v: 3`, add `encounter: None,`)

**Interfaces:**
- Produces: `PROTOCOL_VERSION == 3`; `Personal { damage, dps, taken, taken_ps, healing, hps, overheal: u64 }`; `MeterRow { name: String, amount: u64, per_s: u64, is_you: bool }`; `Encounter { active: bool, duration_s: u64, you: Personal, damage: Vec<MeterRow>, healing: Vec<MeterRow> }`; `Snapshot.encounter: Option<Encounter>` (serde default).

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/wisp-proto/src/lib.rs` (`sample()` gains `encounter: None`; the Spec 2 test `a_v1_line_is_refused_by_version_not_by_shape` changes its expected `expected` to `3`):

```rust
    fn fight() -> Encounter {
        Encounter {
            active: true,
            duration_s: 42,
            you: Personal { damage: 18_234, dps: 434, taken: 2_210, taken_ps: 52, healing: 900, hps: 21, overheal: 120 },
            damage: vec![
                MeterRow { name: "you".to_string(), amount: 18_234, per_s: 434, is_you: true },
                MeterRow { name: "Serenitee".to_string(), amount: 12_010, per_s: 286, is_you: false },
            ],
            healing: vec![MeterRow { name: "Misery".to_string(), amount: 3_100, per_s: 74, is_you: false }],
        }
    }

    #[test]
    fn an_encounter_round_trips() {
        let mut s = sample();
        s.encounter = Some(fight());
        let decoded = decode(&encode(&s)).unwrap();
        assert_eq!(decoded, s);
        assert_eq!(decoded.encounter.unwrap().damage[1].name, "Serenitee");
    }

    #[test]
    fn a_missing_encounter_decodes_as_none_and_none_encodes_as_null() {
        let line = r#"{"v":3,"seq":1,"ts":"x","lines_ingested":0,"session_kills":0,"timers":[]}"#;
        assert_eq!(decode(line).unwrap().encounter, None);
        assert!(encode(&sample()).contains(r#""encounter":null"#));
    }

    #[test]
    fn a_v2_line_is_refused_by_version() {
        let v2 = r#"{"v":2,"seq":1,"ts":"x","lines_ingested":0,"session_kills":0,"timers":[]}"#;
        match decode(v2) {
            Err(ProtoError::Version { found: 2, expected: 3 }) => {}
            other => panic!("expected a version error, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wisp-proto`
Expected: FAIL to compile — `Encounter`, `Personal`, `MeterRow` do not exist.

- [ ] **Step 3: Implement**

```rust
/// Bumped whenever the snapshot shape changes incompatibly.
/// 1: Spec 1 counters. 2: Spec 2 adds `timers`. 3: Spec 3 adds `encounter`.
pub const PROTOCOL_VERSION: u32 = 3;

/// Your own numbers for the current fight. Rates are per second of fight
/// duration, rounded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Personal {
    pub damage: u64,
    pub dps: u64,
    pub taken: u64,
    pub taken_ps: u64,
    pub healing: u64,
    pub hps: u64,
    pub overheal: u64,
}

/// One ranked row of a group meter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeterRow {
    /// `you`, or the source as the log printed it; an owned warder or pet
    /// is folded into its owner.
    pub name: String,
    pub amount: u64,
    pub per_s: u64,
    pub is_you: bool,
}

/// The current fight, or the last one while it lingers. `active` is false
/// while lingering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encounter {
    pub active: bool,
    pub duration_s: u64,
    pub you: Personal,
    /// At most 5 rows, amount descending; `you` always present if you dealt any.
    pub damage: Vec<MeterRow>,
    /// At most 3 rows, amount descending.
    pub healing: Vec<MeterRow>,
}
```

and in `Snapshot`, after `timers`:

```rust
    /// The current or lingering fight; `null` when there is none. Absent on
    /// v1 and v2 lines.
    #[serde(default)]
    pub encounter: Option<Encounter>,
```

Then the four downstream edits listed under Files so the workspace compiles.

- [ ] **Step 4: Run the whole workspace**

Run: `cargo test --workspace`
Expected: PASS — wisp-proto 11, wispd 57 (+2 ignored), wisp-hud 25. Clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/wisp-proto crates/wispd/src/main.rs crates/wispd/src/server.rs crates/wisp-hud/src/client.rs crates/wisp-hud/src/main.rs
git commit -m "$(cat <<'EOF'
Protocol v3: snapshots carry the current fight

One optional block: whether a fight is open or merely lingering, its
duration, your damage, damage taken and healing with their per-second
rates, and ranked rows for every player's damage and healing. Null when
there is nothing to show, so the HUD can draw an empty panel of the same
shape.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: The combat classifier

**Files:**
- Create: `crates/wispd/src/combat.rs`
- Modify: `crates/wispd/src/main.rs` (add `#[allow(dead_code)] // wired in Task 4` + `mod combat;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `combat::DamageKind { Melee, Spell, Dot, Shield }`, `combat::Source<'a> { You, Named(&'a str) }`, `combat::CombatEvent<'a> { Damage { source, target: &'a str, amount: u64, kind }, Taken { source: &'a str, amount: u64, kind }, Heal { source, target: &'a str, actual: u64, potential: u64, hot: bool }, PetAnnounce { pet: &'a str }, Boundary }`, `combat::classify(body: &str) -> Option<CombatEvent<'_>>`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/wispd/src/combat.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;
    use CombatEvent::*;
    use DamageKind::*;

    fn n(s: &str) -> Source<'_> { Source::Named(s) }

    #[test]
    fn your_melee_spell_dot_and_shield() {
        assert_eq!(classify("You kick a spiderling for 17 points of damage."),
            Some(Damage { source: Source::You, target: "a spiderling", amount: 17, kind: Melee }));
        assert_eq!(classify("You kick a bixie drone for 32 points of damage. (Critical)"),
            Some(Damage { source: Source::You, target: "a bixie drone", amount: 32, kind: Melee }));
        assert_eq!(classify("You hit a large wood spider for 44 points of damage."),
            Some(Damage { source: Source::You, target: "a large wood spider", amount: 44, kind: Melee }),
            "a plain melee 'hit' has no spell and counts as melee");
        assert_eq!(classify("You hit a spite golem for 961 points of magic damage by Discordant Mind. (Critical)"),
            Some(Damage { source: Source::You, target: "a spite golem", amount: 961, kind: Spell }));
        assert_eq!(classify("Xicotl has taken 88 damage from your Gasping Embrace."),
            Some(Damage { source: Source::You, target: "Xicotl", amount: 88, kind: Dot }));
        assert_eq!(classify("An abhorrent is pierced by YOUR thorns for 15 points of non-melee damage."),
            Some(Damage { source: Source::You, target: "An abhorrent", amount: 15, kind: Shield }));
    }

    #[test]
    fn other_sources_melee_spell_dot_and_shield() {
        assert_eq!(classify("Zaiv slashes a gnoll scout for 21 points of damage."),
            Some(Damage { source: n("Zaiv"), target: "a gnoll scout", amount: 21, kind: Melee }));
        assert_eq!(classify("Innoruuk`s Chosen slashes Zaiv for 30 points of damage."),
            Some(Damage { source: n("Innoruuk`s Chosen"), target: "Zaiv", amount: 30, kind: Melee }),
            "the verb is the first plain word ending in s, so a backtick name stays whole");
        assert_eq!(classify("A gnoll scout hits Zaiv for 5 points of damage."),
            Some(Damage { source: n("A gnoll scout"), target: "Zaiv", amount: 5, kind: Melee }));
        assert_eq!(classify("Kebanab hit Guard Wytiffin for 250 points of magic damage by Life Leech."),
            Some(Damage { source: n("Kebanab"), target: "Guard Wytiffin", amount: 250, kind: Spell }));
        assert_eq!(classify("a rat has taken 12 damage from Serenitee's Envenomed Bolt."),
            Some(Damage { source: n("Serenitee"), target: "a rat", amount: 12, kind: Dot }));
        assert_eq!(classify("Guard Xyxax is burned by Skullgrinder's flames for 15 points of non-melee damage."),
            Some(Damage { source: n("Skullgrinder"), target: "Guard Xyxax", amount: 15, kind: Shield }));
        assert_eq!(classify("Jennie`s warder claws a rat for 40 points of damage."),
            Some(Damage { source: n("Jennie`s warder"), target: "a rat", amount: 40, kind: Melee }));
    }

    #[test]
    fn damage_taken_by_you() {
        assert_eq!(classify("A giant thicket rat bites YOU for 2 points of damage. (Riposte)"),
            Some(Taken { source: "A giant thicket rat", amount: 2, kind: Melee }));
        assert_eq!(classify("Princess Cherista hit you for 175 points of poison damage by Shock of Poison."),
            Some(Taken { source: "Princess Cherista", amount: 175, kind: Spell }));
        assert_eq!(classify("You have taken 30 damage from Deadly Poison by a revultant rat."),
            Some(Taken { source: "a revultant rat", amount: 30, kind: Dot }));
    }

    #[test]
    fn heals_with_and_without_overheal_and_over_time() {
        assert_eq!(classify("You healed Vibtik for 128 (197) hit points by Valor."),
            Some(Heal { source: Source::You, target: "Vibtik", actual: 128, potential: 197, hot: false }));
        assert_eq!(classify("You healed Daggo over time for 102 hit points by Ethereal Cleansing."),
            Some(Heal { source: Source::You, target: "Daggo", actual: 102, potential: 102, hot: true }));
        assert_eq!(classify("Alpharius healed himself for 73 hit points."),
            Some(Heal { source: n("Alpharius"), target: "himself", actual: 73, potential: 73, hot: false }));
        assert_eq!(classify("Cauli healed himself for 0 (229) hit points by Armor of Protection."),
            Some(Heal { source: n("Cauli"), target: "himself", actual: 0, potential: 229, hot: false }));
        assert_eq!(classify("Penuche healed you for 535 (556) hit points by Symbol of Naltron."),
            Some(Heal { source: n("Penuche"), target: "you", actual: 535, potential: 556, hot: false }));
    }

    #[test]
    fn pet_announcements_and_boundaries() {
        assert_eq!(classify("A revultant rat told you, 'Attacking an abhorrent Master.'"), Some(PetAnnounce { pet: "A revultant rat" }));
        assert_eq!(classify("Jarann tells you, 'Attacking a rat Master.'"), Some(PetAnnounce { pet: "Jarann" }));
        assert_eq!(classify("Vibtik told you, 'I am unable to wake Brother Jentry, Master.'"), None, "only attack announcements name a pet");
        assert_eq!(classify("You have entered The Northern Desert of Ro."), Some(Boundary));
        assert_eq!(classify("LOADING, PLEASE WAIT..."), Some(Boundary));
        assert_eq!(classify("Welcome to EverQuest Legends!"), Some(Boundary));
    }

    #[test]
    fn lines_without_an_amount_are_nothing() {
        assert_eq!(classify("You try to punch a halfling skeleton, but miss!"), None);
        assert_eq!(classify("A large rat bites YOU for 4 points of damage"), None, "no full stop: not the shape");
        assert_eq!(classify("Barrin tells general2:1, 'for 100 points of damage.'"), None);
        assert_eq!(classify("Your Mesmerization spell has worn off of a rat."), None);
        assert_eq!(classify("You gain experience! (0.002%)"), None);
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wispd combat`
Expected: FAIL to compile — `classify` does not exist.

- [ ] **Step 3: Implement**

```rust
// SPDX-License-Identifier: MIT
//! One line of combat or healing -> one event carrying an amount. Pure.
//!
//! Every shape here was read from the reference fixture. Nothing without an
//! amount is an event: misses, dodges and crit rates are not this spec's
//! business. A suffix such as ` (Critical)` after the full stop is dropped
//! before matching.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Melee,
    Spell,
    Dot,
    Shield,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source<'a> {
    You,
    Named(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombatEvent<'a> {
    /// `source` dealt `amount` to `target`. The target is never you here.
    Damage { source: Source<'a>, target: &'a str, amount: u64, kind: DamageKind },
    /// `source` dealt `amount` to you.
    Taken { source: &'a str, amount: u64, kind: DamageKind },
    /// `actual` landed; `potential` is what the spell could have healed
    /// (equal to `actual` when the log printed one number).
    Heal { source: Source<'a>, target: &'a str, actual: u64, potential: u64, hot: bool },
    /// Your pet told you it is attacking; `pet` is its printed name.
    PetAnnounce { pet: &'a str },
    /// Zone change or session start.
    Boundary,
}

/// `You kick a rat for 17 points of damage. (Critical)` -> without the note.
fn strip_note(body: &str) -> &str {
    match body.rfind(". (") {
        Some(i) if body.ends_with(')') => &body[..i + 1],
        _ => body,
    }
}

fn amount(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn is_plain_word(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `<source> <verb>s <target>` -> (source, target). The verb is the first
/// plain word (letters, digits, underscore) ending in `s` that has a source
/// before it and a target after it; a backtick name like `Innoruuk`s Chosen`
/// is not a plain word and stays part of the source. Matches the reference
/// replay's leftmost split.
fn split_at_verb(left: &str) -> Option<(&str, &str)> {
    for (p, _) in left.match_indices(' ') {
        if p == 0 {
            continue;
        }
        let rest = &left[p + 1..];
        let (word, after) = rest.split_once(' ')?;
        if word.ends_with('s') && is_plain_word(word) && !after.is_empty() {
            return Some((&left[..p], after));
        }
    }
    None
}

fn source_of(name: &str) -> Source<'_> {
    if name == "You" { Source::You } else { Source::Named(name) }
}

pub fn classify(body: &str) -> Option<CombatEvent<'_>> {
    use CombatEvent::*;
    use DamageKind::*;

    if body.ends_with(" Master.'") {
        for sep in [" tells you, 'Attacking ", " told you, 'Attacking "] {
            if let Some((pet, _)) = body.split_once(sep) {
                return Some(PetAnnounce { pet });
            }
        }
        return None;
    }
    if body.starts_with("You have entered ")
        || body.starts_with("LOADING, PLEASE WAIT")
        || body == "Welcome to EverQuest Legends!"
    {
        return Some(Boundary);
    }

    let b = strip_note(body);

    // Melee: `… for N points of damage.` -- on you, yours, or another source's.
    if let Some(head) = b.strip_suffix(" points of damage.") {
        let (left, n) = head.rsplit_once(" for ")?;
        let amount = amount(n)?;
        if let Some(src) = left.strip_suffix(" YOU") {
            let (source, _verb) = src.rsplit_once(' ')?;
            return Some(Taken { source, amount, kind: Melee });
        }
        if let Some(rest) = left.strip_prefix("You ") {
            let (_verb, target) = rest.split_once(' ')?;
            return Some(Damage { source: Source::You, target, amount, kind: Melee });
        }
        let (source, target) = split_at_verb(left)?;
        return Some(Damage { source: Source::Named(source), target, amount, kind: Melee });
    }

    // Damage shield: `<target> is <verb> by YOUR|<source>'s <thing> for N points of non-melee damage.`
    if let Some(head) = b.strip_suffix(" points of non-melee damage.") {
        let (left, n) = head.rsplit_once(" for ")?;
        let amount = amount(n)?;
        let (target, rest) = left.split_once(" is ")?;
        let (_verb, by) = rest.split_once(" by ")?;
        if by.strip_prefix("YOUR ").is_some() {
            return Some(Damage { source: Source::You, target, amount, kind: Shield });
        }
        let (source, _thing) = by.split_once("'s ")?;
        return Some(Damage { source: Source::Named(source), target, amount, kind: Shield });
    }

    let head = b.strip_suffix('.')?;

    // Spell: `… for N points of <type> damage by <Spell>.`
    if let Some((left, _spell)) = head.rsplit_once(" damage by ") {
        let (left, _ty) = left.rsplit_once(" points of ")?;
        let (who, n) = left.rsplit_once(" for ")?;
        let amount = amount(n)?;
        if let Some(target) = who.strip_prefix("You hit ") {
            return Some(Damage { source: Source::You, target, amount, kind: Spell });
        }
        if let Some(source) = who.strip_suffix(" hit you") {
            return Some(Taken { source, amount, kind: Spell });
        }
        let (source, target) = who.split_once(" hit ")?;
        return Some(Damage { source: Source::Named(source), target, amount, kind: Spell });
    }

    // DoT ticks: `You have taken N damage from <Spell> by <source>.`,
    // `<target> has taken N damage from your <Spell>.`, `… from <source>'s <Spell>.`
    if let Some(rest) = head.strip_prefix("You have taken ") {
        let (n, rest) = rest.split_once(" damage from ")?;
        let (_spell, source) = rest.split_once(" by ")?;
        return Some(Taken { source, amount: amount(n)?, kind: Dot });
    }
    if let Some((target, rest)) = head.split_once(" has taken ") {
        if let Some((n, from)) = rest.split_once(" damage from ") {
            let amount = amount(n)?;
            if from.strip_prefix("your ").is_some() {
                return Some(Damage { source: Source::You, target, amount, kind: Dot });
            }
            let (source, _spell) = from.split_once("'s ")?;
            return Some(Damage { source: Source::Named(source), target, amount, kind: Dot });
        }
    }

    // Heals: `<source> healed <target>[ over time] for N[ (M)] hit points[ by <Spell>].`
    if head.contains(" hit points") {
        let left = match head.rsplit_once(" hit points by ") {
            Some((l, _spell)) => l,
            None => head.strip_suffix(" hit points")?,
        };
        let (who, amt) = left.rsplit_once(" for ")?;
        let (actual, potential) = match amt.split_once(" (") {
            Some((a, p)) => (amount(a)?, amount(p.strip_suffix(')')?)?),
            None => {
                let a = amount(amt)?;
                (a, a)
            }
        };
        let (source, target) = who.split_once(" healed ")?;
        let (target, hot) = match target.strip_suffix(" over time") {
            Some(t) => (t, true),
            None => (target, false),
        };
        return Some(Heal { source: source_of(source), target, actual, potential, hot });
    }

    None
}
```

Add to `crates/wispd/src/main.rs`: `#[allow(dead_code)] // wired into the pipeline in Task 4` above `mod combat;`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wispd`
Expected: PASS — 63 tests (57 + 6). Clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/wispd
git commit -m "$(cat <<'EOF'
Add the combat classifier: one line, one amount, one event

Fourteen shapes read from the fixture: your melee, spells, DoT ticks and
damage shield; the same four from other sources; melee, spells and DoTs
on you; heals with the two numbers the game prints; the pet's attack
announcement; and the zone and session boundaries. A suffix such as
"(Critical)" is dropped before matching. Anything without an amount is
not an event.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Actors, sessions and the replay

**Files:**
- Create: `crates/wispd/src/encounter.rs`
- Modify: `crates/wispd/src/main.rs` (add `#[allow(dead_code)] // wired in Task 4` + `mod encounter;`; remove the allow on `mod combat;` since `encounter` consumes it — keep the allow on `mod encounter;` only)

**Interfaces:**
- Consumes: `combat::{classify, CombatEvent, DamageKind, Source}`, `rules::{timestamp_text, body, parse_log_time}`, `wisp_proto::{Encounter, MeterRow, Personal}`.
- Produces: `encounter::{IDLE_SECS, PET_TTL_SECS, LINGER_SECS, MAX_DAMAGE_ROWS, MAX_HEALING_ROWS}`, `encounter::player_name_from_log(log: &Path) -> Option<String>`, `encounter::EncounterStats` (pub `u64` fields: `encounters, dmg_out_melee, dmg_out_spell, dmg_out_dot, dmg_out_shield, own_self_dmg, own_pet_dmg, taken_melee, taken_spell, taken_dot, heal_actual, heal_over, own_heal_actual, own_heal_over, own_hot_actual, heal_outside_fight, pet_announcements, boundaries`), `encounter::FightSummary { start: i64, duration_s: u64, own_damage: u64, taken: u64, damage: Vec<(String, u64)>, healing: Vec<(String, u64)> }`, `encounter::Tracker::{new(player: &str) -> Self, with_history(player: &str) -> Self, observe(&mut self, line: &str), encounter(&self, now_secs: f64) -> Option<Encounter>, last_time(&self) -> Option<i64>, stats(&self) -> &EncounterStats, history(&self) -> &[FightSummary], pet_count(&self) -> usize}`.

Semantics are Appendix A's, transition for transition. `last_time()` and `now_secs` are on the tracker's own clock: seconds since the first timestamped line it observed (as Spec 2's tracker). Log timestamps are used for nothing else.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/wispd/src/encounter.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64, body: &str) -> String {
        let t = crate::rules::parse_log_time("Mon Aug 10 20:00:00 2026").unwrap() + secs;
        let (h, m, s) = ((t / 3600) % 24, (t / 60) % 60, t % 60);
        format!("[Mon Aug 10 {h:02}:{m:02}:{s:02} 2026] {body}")
    }

    fn tracker() -> Tracker {
        Tracker::with_history("Daggo")
    }

    fn feed(t: &mut Tracker, lines: &[(i64, &str)]) {
        for (s, b) in lines {
            t.observe(&at(*s, b));
        }
    }

    #[test]
    fn the_player_name_comes_from_the_log_filename() {
        use std::path::Path;
        assert_eq!(player_name_from_log(Path::new("/x/Logs/eqlog_Daggo_freeport.txt")), Some("Daggo".to_string()));
        assert_eq!(player_name_from_log(Path::new("/f/eqlog_Daggo_freeport.1440036.txt")), Some("Daggo".to_string()));
        assert_eq!(player_name_from_log(Path::new("/f/notes.txt")), None);
    }

    #[test]
    fn a_fight_opens_on_damage_and_your_numbers_accumulate() {
        let mut t = tracker();
        feed(&mut t, &[
            (0, "You kick a rat for 100 points of damage."),
            (2, "You hit a rat for 300 points of magic damage by Shock."),
            (4, "A rat bites YOU for 50 points of damage."),
            (5, "You healed Daggo for 40 (60) hit points by Valor."),
        ]);
        let e = t.encounter(5.0).unwrap();
        assert!(e.active);
        assert_eq!(e.duration_s, 5);
        assert_eq!((e.you.damage, e.you.dps), (400, 80));
        assert_eq!((e.you.taken, e.you.taken_ps), (50, 10));
        assert_eq!((e.you.healing, e.you.hps, e.you.overheal), (40, 8, 20));
        assert_eq!(e.damage.len(), 1);
        assert!(e.damage[0].is_you);
        assert_eq!(e.damage[0].name, "you");
    }

    #[test]
    fn a_heal_does_not_open_a_fight_and_is_not_counted_outside_one() {
        let mut t = tracker();
        feed(&mut t, &[(0, "You healed Daggo for 40 hit points by Valor.")]);
        assert!(t.encounter(0.0).is_none());
        assert_eq!(t.stats().heal_outside_fight, 40);
        assert_eq!(t.stats().encounters, 0);
    }

    #[test]
    fn ten_idle_seconds_close_the_fight_and_it_lingers_for_thirty() {
        let mut t = tracker();
        feed(&mut t, &[(0, "You kick a rat for 100 points of damage."), (3, "You kick a rat for 100 points of damage.")]);
        // still open at 13 (last + 10 inclusive), closed by the line at 14
        feed(&mut t, &[(13, "Unrelated chatter."), (14, "Unrelated chatter.")]);
        assert_eq!(t.stats().encounters, 1);
        let e = t.encounter(14.0).unwrap();
        assert!(!e.active, "lingering");
        assert_eq!(e.duration_s, 3);
        assert_eq!(e.you.dps, 67, "200 / 3 rounded");
        assert!(t.encounter(43.0).is_some(), "3 + 10 + 30 = 43 still visible");
        assert!(t.encounter(43.5).is_none(), "gone after the linger");
        assert_eq!(t.history().len(), 1);
        assert_eq!(t.history()[0].duration_s, 3);
    }

    #[test]
    fn zoning_closes_a_fight_with_no_linger() {
        let mut t = tracker();
        feed(&mut t, &[(0, "You kick a rat for 100 points of damage."), (1, "You have entered Somewhere.")]);
        assert!(t.encounter(1.0).is_none());
        assert_eq!(t.stats().boundaries, 1);
        assert_eq!(t.stats().encounters, 1);
    }

    #[test]
    fn players_are_ranked_and_mobs_are_not_sources() {
        let mut t = tracker();
        feed(&mut t, &[
            (0, "You kick a rat for 100 points of damage."),
            (1, "Serenitee slashes a rat for 500 points of damage."),
            (2, "Misery hit a rat for 250 points of magic damage by Bolt."),
            (3, "A rat bites Serenitee for 900 points of damage."),
            (4, "Guard Xyxax slashes Zaiv for 800 points of damage."),
        ]);
        let e = t.encounter(4.0).unwrap();
        let names: Vec<_> = e.damage.iter().map(|r| (r.name.as_str(), r.amount)).collect();
        assert_eq!(names, vec![("Serenitee", 500), ("Misery", 250), ("you", 100)]);
        assert_eq!(t.stats().dmg_out_melee, 600);
        assert_eq!(t.stats().dmg_out_spell, 250);
    }

    #[test]
    fn your_pet_is_you_for_two_minutes_after_it_announces() {
        let mut t = tracker();
        feed(&mut t, &[
            (0, "A revultant rat told you, 'Attacking an abhorrent Master.'"),
            (1, "A revultant rat bites an abhorrent for 70 points of damage."),
            (2, "a revultant rat bites an abhorrent for 30 points of damage."),
        ]);
        let e = t.encounter(2.0).unwrap();
        assert_eq!(e.you.damage, 100, "case-insensitive pet name");
        assert_eq!(t.stats().own_pet_dmg, 100);
        assert_eq!(t.pet_count(), 1);
        feed(&mut t, &[(121, "A revultant rat bites an abhorrent for 5 points of damage.")]);
        // 121 - 0 > 120: the announcement has lapsed and the rat is a mob again
        assert_eq!(t.stats().own_pet_dmg, 100);
    }

    #[test]
    fn a_warder_belongs_to_its_owner() {
        let mut t = tracker();
        feed(&mut t, &[
            (0, "Jennie`s warder claws a rat for 40 points of damage."),
            (1, "Jennie slashes a rat for 10 points of damage."),
            (2, "Daggo`s warder claws a rat for 7 points of damage."),
        ]);
        let e = t.encounter(2.0).unwrap();
        let names: Vec<_> = e.damage.iter().map(|r| (r.name.as_str(), r.amount)).collect();
        assert_eq!(names, vec![("Jennie", 50), ("you", 7)]);
        assert_eq!(t.stats().own_pet_dmg, 7);
    }

    #[test]
    fn a_named_mob_becomes_a_mob_once_it_touches_you() {
        let mut t = tracker();
        feed(&mut t, &[
            (0, "Xicotl slashes Zaiv for 100 points of damage."),
            (1, "Xicotl slashes YOU for 20 points of damage."),
            (2, "Xicotl slashes Zaiv for 100 points of damage."),
        ]);
        let e = t.encounter(2.0).unwrap();
        assert_eq!(e.damage.iter().find(|r| r.name == "Xicotl").map(|r| r.amount), Some(100), "counted until it hit you, not after");
        assert_eq!(e.you.taken, 20);
    }

    #[test]
    fn rows_are_capped_and_you_are_always_in_the_damage_rows() {
        let mut t = tracker();
        let lines: Vec<(i64, String)> = (0..7)
            .map(|i| (i, format!("Player{i} slashes a rat for {} points of damage.", 1000 - i)))
            .chain(std::iter::once((8, "You kick a rat for 1 points of damage.".to_string())))
            .chain((9..12).map(|i| (i, format!("Healer{i} healed himself for {} hit points.", 100 - i))))
            .chain(std::iter::once((13, "Player0 healed himself for 1 hit points.".to_string())))
            .collect();
        for (s, b) in &lines {
            t.observe(&at(*s, b));
        }
        let e = t.encounter(13.0).unwrap();
        assert_eq!(e.damage.len(), MAX_DAMAGE_ROWS);
        assert!(e.damage.last().unwrap().is_you, "you replace the last row when outside the top");
        assert_eq!(e.damage[0].name, "Player0");
        assert_eq!(e.healing.len(), MAX_HEALING_ROWS);
        assert_eq!(e.healing[0].name, "Healer9");
    }

    #[test]
    fn a_new_fight_replaces_a_lingering_one() {
        let mut t = tracker();
        feed(&mut t, &[(0, "You kick a rat for 100 points of damage."), (20, "Chatter."), (21, "You kick a rat for 7 points of damage.")]);
        let e = t.encounter(21.0).unwrap();
        assert!(e.active);
        assert_eq!(e.you.damage, 7);
        assert_eq!(t.stats().encounters, 2);
    }

    /// The acceptance replay. Skipped unless both variables are set:
    /// `WISP_EQL_DIR=<install> WISP_FIXTURE=<frozen fixture> cargo test -p wispd --release -- --ignored encounter`
    #[test]
    #[ignore]
    fn fixture_replay_matches_the_reference_exactly() {
        let Some(fixture) = std::env::var_os("WISP_FIXTURE") else { return };
        let mut t = Tracker::with_history("Daggo");
        let text = std::fs::read_to_string(&fixture).unwrap();
        for line in text.lines() {
            t.observe(line.trim_end_matches('\r'));
        }
        assert_eq!(
            *t.stats(),
            EncounterStats {
                encounters: 2524,
                dmg_out_melee: 15198942,
                dmg_out_spell: 9082968,
                dmg_out_dot: 4622907,
                dmg_out_shield: 447208,
                own_self_dmg: 18085260,
                own_pet_dmg: 5284285,
                taken_melee: 1924867,
                taken_spell: 709376,
                taken_dot: 273054,
                taken_shield: 257300,
                heal_actual: 2364526,
                heal_over: 838539,
                own_heal_actual: 1875603,
                own_heal_over: 590911,
                own_hot_actual: 355478,
                heal_outside_fight: 259121,
                pet_announcements: 4890,
                boundaries: 971,
                group_member_lines: 40,
                group_leaves: 4,
                group_resets: 7,
            }
        );
        assert_eq!(t.pet_count(), 91);
        assert_eq!(t.group_size(), 0, "the group is empty again at end of file");
        let h = t.history();
        assert_eq!(h.len(), 2524);
        let mut durs: Vec<u64> = h.iter().map(|f| f.duration_s).collect();
        durs.sort_unstable();
        assert_eq!((durs[0], durs[durs.len() / 2], *durs.last().unwrap(), durs.iter().sum::<u64>()), (1, 36, 738, 146813));
        let big = h.iter().max_by_key(|f| f.own_damage).unwrap();
        assert_eq!((big.own_damage, big.duration_s, (big.own_damage as f64 / big.duration_s as f64).round() as u64, big.taken), (212467, 482, 441, 21416));
        let mut dmg: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        let mut heal: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for f in h {
            for (n, a) in &f.damage { *dmg.entry(n.clone()).or_default() += a; }
            for (n, a) in &f.healing { *heal.entry(n.clone()).or_default() += a; }
        }
        let top = |m: &std::collections::HashMap<String, u64>, k: usize| {
            let mut v: Vec<(String, u64)> = m.iter().map(|(n, a)| (n.clone(), *a)).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            v.truncate(k);
            v
        };
        assert_eq!(top(&dmg, 4), vec![
            ("you".to_string(), 23369545), ("Yder".to_string(), 1447129),
            ("Serenitee".to_string(), 1321322), ("Misery".to_string(), 1000700)]);
        assert_eq!(top(&heal, 3), vec![
            ("you".to_string(), 1875603), ("Serenitee".to_string(), 196052), ("Misery".to_string(), 116859)]);
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wispd encounter`
Expected: FAIL to compile — `Tracker` does not exist.

- [ ] **Step 3: Implement**

```rust
// SPDX-License-Identifier: MIT
//! Who a source is, when a fight starts and ends, and what each source did
//! in it. Pure over classified lines; runs on log time so a replay of an
//! old log behaves exactly as the live session did.
//!
//! Attribution comes from the log's own phrasing: your pet announces
//! "Attacking … Master." to you, a warder or pet is printed with its owner's
//! name, a mob starts with an article or has a space in its name or has
//! fought you. Names are keyed lower-case because the log capitalises an
//! article at the start of a sentence.

use crate::combat::{classify, CombatEvent, DamageKind, Source};
use crate::rules::{body, parse_log_time, timestamp_text};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use wisp_proto::{Encounter, MeterRow, Personal};

/// A fight ends after this long without damage or a heal.
pub const IDLE_SECS: i64 = 10;
/// A pet is yours for this long after each attack announcement.
pub const PET_TTL_SECS: i64 = 120;
/// A finished fight stays on screen this long after it ends.
pub const LINGER_SECS: i64 = 30;
pub const MAX_DAMAGE_ROWS: usize = 5;
pub const MAX_HEALING_ROWS: usize = 3;

const ARTICLES: [&str; 3] = ["a ", "an ", "the "];

/// `eqlog_<Name>_<server>.txt` -> `Name`.
pub fn player_name_from_log(log: &Path) -> Option<String> {
    let stem = log.file_name()?.to_str()?;
    let rest = stem.strip_prefix("eqlog_")?;
    let (name, _) = rest.split_once('_')?;
    if name.is_empty() { None } else { Some(name.to_string()) }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EncounterStats {
    pub encounters: u64,
    pub dmg_out_melee: u64,
    pub dmg_out_spell: u64,
    pub dmg_out_dot: u64,
    pub dmg_out_shield: u64,
    pub own_self_dmg: u64,
    pub own_pet_dmg: u64,
    pub taken_melee: u64,
    pub taken_spell: u64,
    pub taken_dot: u64,
    pub heal_actual: u64,
    pub heal_over: u64,
    pub own_heal_actual: u64,
    pub own_heal_over: u64,
    pub own_hot_actual: u64,
    pub heal_outside_fight: u64,
    pub pet_announcements: u64,
    pub boundaries: u64,
}

/// One finished fight, for tests and replays. Rows are (name, amount),
/// amount descending then name ascending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FightSummary {
    pub start: i64,
    pub duration_s: u64,
    pub own_damage: u64,
    pub taken: u64,
    pub damage: Vec<(String, u64)>,
    pub healing: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    You,
    Pet,
    Player,
    Mob,
}

struct Fight {
    start: i64,
    last: i64,
    damage: HashMap<String, u64>,
    healing: HashMap<String, u64>,
    overheal: HashMap<String, u64>,
    taken: u64,
}

impl Fight {
    fn new(start: i64) -> Self {
        Fight { start, last: start, damage: HashMap::new(), healing: HashMap::new(), overheal: HashMap::new(), taken: 0 }
    }
    fn duration(&self) -> u64 {
        (self.last - self.start).max(1) as u64
    }
    fn rows(map: &HashMap<String, u64>) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = map.iter().map(|(n, a)| (n.clone(), *a)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }
    fn summary(&self) -> FightSummary {
        FightSummary {
            start: self.start,
            duration_s: self.duration(),
            own_damage: self.damage.get("you").copied().unwrap_or(0),
            taken: self.taken,
            damage: Self::rows(&self.damage),
            healing: Self::rows(&self.healing),
        }
    }
}

fn rate(amount: u64, duration_s: u64) -> u64 {
    (amount as f64 / duration_s.max(1) as f64).round() as u64
}

fn meter_rows(rows: &[(String, u64)], duration_s: u64, max: usize, force_you: bool) -> Vec<MeterRow> {
    let mut out: Vec<MeterRow> = rows
        .iter()
        .take(max)
        .map(|(n, a)| MeterRow { name: n.clone(), amount: *a, per_s: rate(*a, duration_s), is_you: n == "you" })
        .collect();
    if force_you && !out.iter().any(|r| r.is_you) {
        if let Some((n, a)) = rows.iter().find(|(n, _)| n == "you") {
            if out.len() == max {
                out.pop();
            }
            out.push(MeterRow { name: n.clone(), amount: *a, per_s: rate(*a, duration_s), is_you: true });
        }
    }
    out
}

fn to_encounter(f: &Fight, active: bool) -> Encounter {
    let d = f.duration();
    let you_dmg = f.damage.get("you").copied().unwrap_or(0);
    let you_heal = f.healing.get("you").copied().unwrap_or(0);
    Encounter {
        active,
        duration_s: d,
        you: Personal {
            damage: you_dmg,
            dps: rate(you_dmg, d),
            taken: f.taken,
            taken_ps: rate(f.taken, d),
            healing: you_heal,
            hps: rate(you_heal, d),
            overheal: f.overheal.get("you").copied().unwrap_or(0),
        },
        damage: meter_rows(&Fight::rows(&f.damage), d, MAX_DAMAGE_ROWS, true),
        healing: meter_rows(&Fight::rows(&f.healing), d, MAX_HEALING_ROWS, false),
    }
}

pub struct Tracker {
    player: String,
    pets: HashMap<String, i64>,
    mobs: HashSet<String>,
    current: Option<Fight>,
    /// A finished fight and the tracker time until which it is shown.
    lingering: Option<(Fight, i64)>,
    stats: EncounterStats,
    history: Option<Vec<FightSummary>>,
    epoch: Option<i64>,
    last_time: Option<i64>,
}

fn owner_of(name: &str) -> Option<&str> {
    name.strip_suffix("`s warder").or_else(|| name.strip_suffix("`s pet"))
}

fn is_mobish(name: &str) -> bool {
    let lower = name.to_lowercase();
    ARTICLES.iter().any(|a| lower.starts_with(a))
}

impl Tracker {
    pub fn new(player: &str) -> Self {
        Tracker {
            player: player.to_lowercase(),
            pets: HashMap::new(),
            mobs: HashSet::new(),
            current: None,
            lingering: None,
            stats: EncounterStats::default(),
            history: None,
            epoch: None,
            last_time: None,
        }
    }

    /// Keeps a summary of every finished fight; for tests and replays.
    pub fn with_history(player: &str) -> Self {
        let mut t = Tracker::new(player);
        t.history = Some(Vec::new());
        t
    }

    pub fn stats(&self) -> &EncounterStats {
        &self.stats
    }

    pub fn history(&self) -> &[FightSummary] {
        self.history.as_deref().unwrap_or(&[])
    }

    pub fn pet_count(&self) -> usize {
        self.pets.len()
    }

    /// Seconds since the first timestamped line, on the tracker's own clock.
    pub fn last_time(&self) -> Option<i64> {
        self.last_time
    }

    /// Who `name` is right now, and the key its amounts are booked under.
    fn resolve(&self, name: &str, now: i64) -> (String, Role) {
        let lower = name.to_lowercase();
        if lower == self.player {
            return ("you".to_string(), Role::You);
        }
        if let Some(&t) = self.pets.get(&lower) {
            if now - t <= PET_TTL_SECS {
                return ("you".to_string(), Role::Pet);
            }
        }
        if let Some(owner) = owner_of(name) {
            return if owner.to_lowercase() == self.player {
                ("you".to_string(), Role::Pet)
            } else {
                (owner.to_string(), Role::Pet)
            };
        }
        if is_mobish(name) || self.mobs.contains(&lower) || name.contains(' ') {
            return (name.to_string(), Role::Mob);
        }
        (name.to_string(), Role::Player)
    }

    fn resolve_source(&self, source: Source<'_>, now: i64) -> (String, Role) {
        match source {
            Source::You => ("you".to_string(), Role::You),
            Source::Named(n) => self.resolve(n, now),
        }
    }

    /// Close the open fight. With `linger`, keep it on screen until
    /// `last + IDLE_SECS + LINGER_SECS`; a boundary closes without lingering.
    fn close(&mut self, linger: bool) {
        if let Some(f) = self.current.take() {
            if let Some(h) = self.history.as_mut() {
                h.push(f.summary());
            }
            self.lingering = if linger {
                let until = f.last + IDLE_SECS + LINGER_SECS;
                Some((f, until))
            } else {
                None
            };
        }
    }

    fn expire(&mut self, now: i64) {
        if self.current.as_ref().is_some_and(|f| now - f.last > IDLE_SECS) {
            self.close(true);
        }
        if self.lingering.as_ref().is_some_and(|(_, until)| now > *until) {
            self.lingering = None;
        }
    }

    /// The open fight, opening one on damage. Heals never open a fight.
    fn touch(&mut self, now: i64, is_heal: bool) -> Option<&mut Fight> {
        if self.current.is_none() {
            if is_heal {
                return None;
            }
            self.current = Some(Fight::new(now));
            self.lingering = None;
            self.stats.encounters += 1;
        }
        let f = self.current.as_mut().expect("just opened");
        f.last = now;
        Some(f)
    }

    fn damage_out(&mut self, source: Source<'_>, amount: u64, kind: DamageKind, now: i64) {
        let (key, role) = self.resolve_source(source, now);
        if role == Role::Mob {
            return;
        }
        let f = self.touch(now, false).expect("damage opens a fight");
        *f.damage.entry(key.clone()).or_default() += amount;
        match kind {
            DamageKind::Melee => self.stats.dmg_out_melee += amount,
            DamageKind::Spell => self.stats.dmg_out_spell += amount,
            DamageKind::Dot => self.stats.dmg_out_dot += amount,
            DamageKind::Shield => self.stats.dmg_out_shield += amount,
        }
        if key == "you" {
            match role {
                Role::Pet => self.stats.own_pet_dmg += amount,
                _ => self.stats.own_self_dmg += amount,
            }
        }
    }

    /// Feed one raw log line. Lines without a parseable timestamp are ignored.
    pub fn observe(&mut self, line: &str) {
        let Some(raw) = timestamp_text(line).and_then(parse_log_time) else {
            return;
        };
        let epoch = *self.epoch.get_or_insert(raw);
        let now = raw - epoch;
        self.last_time = Some(now);
        self.expire(now);

        let Some(event) = classify(body(line)) else {
            return;
        };
        match event {
            CombatEvent::PetAnnounce { pet } => {
                self.pets.insert(pet.to_lowercase(), now);
                self.stats.pet_announcements += 1;
            }
            CombatEvent::Boundary => {
                self.close(false);
                self.lingering = None;
                self.stats.boundaries += 1;
            }
            CombatEvent::Taken { source, amount, kind } => {
                self.mobs.insert(source.to_lowercase());
                let f = self.touch(now, false).expect("damage opens a fight");
                f.taken += amount;
                match kind {
                    DamageKind::Melee => self.stats.taken_melee += amount,
                    DamageKind::Spell => self.stats.taken_spell += amount,
                    DamageKind::Dot => self.stats.taken_dot += amount,
                    DamageKind::Shield => {}
                }
            }
            CombatEvent::Damage { source, target, amount, kind } => {
                // The reference marks the target as a mob on your lines, on
                // other sources' melee when the source is not a mob, and on
                // other sources' spells when the source is a plain single
                // word or an owned pet. Not on DoT ticks or shields.
                let mark = match (source, kind) {
                    (Source::You, _) => true,
                    (Source::Named(s), DamageKind::Melee) => self.resolve(s, now).1 != Role::Mob,
                    (Source::Named(s), DamageKind::Spell) => (!is_mobish(s) && !s.contains(' ')) || owner_of(s).is_some(),
                    _ => false,
                };
                if mark {
                    self.mobs.insert(target.to_lowercase());
                }
                self.damage_out(source, amount, kind, now);
            }
            CombatEvent::Heal { source, actual, potential, hot, .. } => {
                let (key, role) = self.resolve_source(source, now);
                if role == Role::Mob {
                    return;
                }
                let over = potential.saturating_sub(actual);
                let Some(f) = self.touch(now, true) else {
                    self.stats.heal_outside_fight += actual;
                    return;
                };
                *f.healing.entry(key.clone()).or_default() += actual;
                *f.overheal.entry(key.clone()).or_default() += over;
                self.stats.heal_actual += actual;
                self.stats.heal_over += over;
                if key == "you" {
                    self.stats.own_heal_actual += actual;
                    self.stats.own_heal_over += over;
                    if hot {
                        self.stats.own_hot_actual += actual;
                    }
                }
            }
        }
    }

    /// The current fight, or the last one while it lingers.
    pub fn encounter(&self, now_secs: f64) -> Option<Encounter> {
        if let Some(f) = &self.current {
            return Some(to_encounter(f, true));
        }
        match &self.lingering {
            Some((f, until)) if now_secs <= *until as f64 => Some(to_encounter(f, false)),
            _ => None,
        }
    }
}
```

Two places that decide the replay numbers, both from the reference: `expire` runs before classification on every timestamped line, and `touch` for a heal returns `None` when no fight is open (the heal is then counted as outside any fight and does not open one). `close(false)` on a boundary drops the fight without lingering; `close(true)` on idle lingers until `last + IDLE + LINGER`.

`main.rs`: `#[allow(dead_code)] // wired into the pipeline in Task 4` above `mod encounter;`; remove the allow from `mod combat;`.

- [ ] **Step 4: Run the unit tests, then the replay**

Run: `cargo test -p wispd`
Expected: PASS — 74 tests (63 + 11). Clippy clean.

Run:
```bash
WISP_EQL_DIR="/mnt/Data4TB/Games/everquest/prefix/drive_c/users/Public/Daybreak Game Company/Installed Games/EverQuest Legends" \
WISP_FIXTURE=/mnt/Data4TB/Games/everquest/fixtures/eqlog_Daggo_freeport.1440036.txt \
cargo test -p wispd --release -- --ignored encounter
```
Expected: PASS. If a counter differs, Appendix A is the arbiter: copy it to a scratch file outside the repo, add a print of the first divergent line on both sides, and fix the Rust. Do not edit the expected numbers.

- [ ] **Step 5: Commit**

```bash
git add crates/wispd
git commit -m "$(cat <<'EOF'
Add the encounter tracker: actors, combat sessions, per-source totals

A fight opens on the first damage line, dealt or taken, and closes after
ten seconds without damage or a heal, or on a boundary. Heals never open
one. Inside it every non-mob source's damage and healing is summed, and
your own damage taken with it.

Who a source is comes from the log: you, by name or pronoun; your pet,
for two minutes after it says "Attacking ... Master."; a warder or pet
printed with its owner's name; a mob by its article, a space in its name,
or having fought you; else a player. Names are keyed lower-case.

A finished fight lingers for thirty seconds so the result can be read.
The acceptance replay reproduces the reference implementation's counters
exactly.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Wire the daemon

**Files:**
- Modify: `crates/wispd/src/main.rs`

**Interfaces:**
- Consumes: `encounter::{Tracker, player_name_from_log}`, `wisp_proto::{Encounter, MeterRow, Personal}`.
- Produces: every snapshot carries `encounter`; `--stub` carries a synthetic fight.

- [ ] **Step 1: Edit `main.rs`**

Add `mod combat;` and `mod encounter;` without attributes (delete the `#[allow(dead_code)]` line above `mod encounter;`). Then, as Spec 2 did for test-only accessors, mark `Tracker::with_history`, `Tracker::stats`, `Tracker::history` and `Tracker::pet_count` `#[cfg(test)]` in `encounter.rs`: nothing in production reads them, and a `#[allow(dead_code)]` is not acceptable. `EncounterStats` and `FightSummary` stay as they are (their fields are written by production code). Extend the `use wisp_proto::{…}` line with `Encounter, MeterRow, Personal`.

After the `tracker` (timers) construction, add:

```rust
    // The encounter tracker needs only the player's name, read from the
    // log's filename; without one, "you" is still recognised by pronoun and
    // only a `Daggo`-style self-reference would be missed.
    let mut fights = match &log {
        Some(p) => {
            let name = encounter::player_name_from_log(p).unwrap_or_default();
            if name.is_empty() {
                eprintln!("wispd: could not read the player's name from the log filename; self-heals by name will not count as yours");
            }
            Some(encounter::Tracker::new(&name))
        }
        None => None,
    };
```

In the poll loop, after `tr.observe(&line);` for timers, add:

```rust
                        if let Some(fx) = fights.as_mut() {
                            fx.observe(&line);
                        }
```

After `timers_now`, add:

```rust
        let encounter_now: Option<Encounter> = match fights.as_ref().and_then(|fx| fx.last_time().map(|t| (fx, t))) {
            Some((fx, last)) => fx.encounter(last as f64 + last_line_arrival.elapsed().as_secs_f64()),
            None if stub => Some(stub_encounter(seq)),
            None => None,
        };
```

and `encounter: encounter_now,` in the `Snapshot { … }` literal. Then add:

```rust
/// A synthetic fight that runs for 45 s and lingers, so the HUD panel can be
/// built and eyeballed without a log.
fn stub_encounter(seq: u64) -> Encounter {
    let t = (seq * 250 / 1000) % 90; // 0..90 s cycle: 45 s fight, 45 s linger-ish
    let active = t < 45;
    let d = t.clamp(1, 45);
    let you = 400 * d;
    let ser = 290 * d;
    let mis = 75 * d;
    Encounter {
        active,
        duration_s: d,
        you: Personal { damage: you, dps: 400, taken: 50 * d, taken_ps: 50, healing: 20 * d, hps: 20, overheal: 4 * d },
        damage: vec![
            MeterRow { name: "you".to_string(), amount: you, per_s: 400, is_you: true },
            MeterRow { name: "Serenitee".to_string(), amount: ser, per_s: 290, is_you: false },
            MeterRow { name: "Misery".to_string(), amount: mis, per_s: 75, is_you: false },
        ],
        healing: vec![
            MeterRow { name: "Misery".to_string(), amount: 74 * d, per_s: 74, is_you: false },
            MeterRow { name: "you".to_string(), amount: 20 * d, per_s: 20, is_you: true },
        ],
    }
}
```

- [ ] **Step 2: Build and test**

Run: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: clean.

- [ ] **Step 3: Verify by socket**

`cargo run -p wispd -- --stub` and `timeout 3 socat - "$XDG_RUNTIME_DIR/wisp/wispd.sock"`: every line carries `"encounter":{…}` with `duration_s` rising. Then the fixture (release binary, temp `XDG_DATA_HOME`, `--spells <install>`): after 20 s the first snapshot shows `"encounter":null` (the fixture's last fight ended more than 40 s before its last line) and the Spec 2 numbers unchanged (`lines_ingested:1440036`, `session_kills:880`). Kill everything, remove the socket and temp dir; paste the outputs into the report.

- [ ] **Step 4: Commit**

```bash
git add crates/wispd
git commit -m "$(cat <<'EOF'
wispd: publish the current fight

The encounter tracker is fed every line beside the timer tracker and
reads the player's name from the log filename. The snapshot's encounter
block is computed against the same estimated log time the timers use.
--stub carries a synthetic 45-second fight that lingers, so the HUD panel
can be built against it.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: The HUD panel

**Files:**
- Modify: `crates/wisp-hud/src/main.rs`

**Interfaces:**
- Consumes: `Snapshot.encounter`, `text::{Line, Renderer}` as they are.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/wisp-hud/src/main.rs`:

```rust
    use wisp_proto::{Encounter, MeterRow, Personal};

    fn fight(active: bool) -> Encounter {
        Encounter {
            active,
            duration_s: 42,
            you: Personal { damage: 18_234, dps: 434, taken: 2_210, taken_ps: 52, healing: 900, hps: 21, overheal: 120 },
            damage: vec![
                MeterRow { name: "you".to_string(), amount: 18_234, per_s: 434, is_you: true },
                MeterRow { name: "Serenitee".to_string(), amount: 1_320_500, per_s: 286, is_you: false },
            ],
            healing: vec![MeterRow { name: "Misery".to_string(), amount: 3_100, per_s: 74, is_you: false }],
        }
    }

    fn snap(encounter: Option<Encounter>) -> Snapshot {
        Snapshot { v: 3, seq: 1, ts: String::new(), lines_ingested: 0, session_kills: 7, timers: vec![], encounter }
    }

    #[test]
    fn amounts_print_compactly() {
        assert_eq!(compact(999), "999");
        assert_eq!(compact(9_999), "9999");
        assert_eq!(compact(18_234), "18.2k");
        assert_eq!(compact(999_949), "999.9k");
        assert_eq!(compact(1_320_500), "1.32M");
    }

    #[test]
    fn the_personal_line_and_rows_are_laid_out_around_the_timers() {
        let lines = hud_lines(&snap(Some(fight(true))));
        assert_eq!(lines[0].text, "7 kills");
        assert_eq!(lines[1].text, "DPS   434  in   52/s  HPS   21   0:42");
        assert_eq!(lines[1].rgb, WHITE);
        assert_eq!(lines[2].text, format!("{} {:>7} {:>5}/s", fit("you", NAME_COLS), "18.2k", 434));
        assert_eq!(lines[2].rgb, YOU_ROW);
        assert_eq!(lines[3].text, format!("{} {:>7} {:>5}/s", fit("Serenitee", NAME_COLS), "1.32M", 286));
        assert_eq!(lines[4].text, format!("{} {:>7} {:>5}/s  +", fit("Misery", NAME_COLS), "3.1k", 74));
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn a_lingering_fight_dims_the_personal_line() {
        let lines = hud_lines(&snap(Some(fight(false))));
        assert_eq!(lines[1].rgb, DIM);
    }

    #[test]
    fn no_fight_draws_the_empty_personal_line_and_no_rows() {
        let lines = hud_lines(&snap(None));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].text, "DPS     -  in    -/s  HPS    -   -:--");
        assert_eq!(lines[1].rgb, DIM);
    }

    #[test]
    fn timer_rows_sit_between_the_personal_line_and_the_meter_rows() {
        let mut s = snap(Some(fight(true)));
        s.timers = vec![timer(11_800, Confidence::Measured)];
        let lines = hud_lines(&s);
        assert!(lines[1].text.starts_with("DPS"));
        assert!(lines[2].text.contains("Mesmerization"));
        assert!(lines[3].text.starts_with(&fit("you", NAME_COLS)));
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wisp-hud`
Expected: FAIL to compile — `compact`, `NAME_COLS`, `YOU_ROW` do not exist.

- [ ] **Step 3: Implement**

Add the constants and helpers next to the existing ones:

```rust
const MAX_DAMAGE_ROWS: usize = 5;
const MAX_HEALING_ROWS: usize = 3;
const NAME_COLS: usize = 14;
const YOU_ROW: [u8; 3] = [140, 200, 255];
const EMPTY_PERSONAL: &str = "DPS     -  in    -/s  HPS    -   -:--";

/// 999 -> "999", 18234 -> "18.2k", 1320500 -> "1.32M". Fits the 7-column amount slot.
fn compact(n: u64) -> String {
    if n < 10_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    }
}

fn personal_line(e: &Encounter) -> String {
    format!(
        "DPS {:>5}  in {:>4}/s  HPS {:>4}   {}:{:02}",
        e.you.dps, e.you.taken_ps, e.you.hps, e.duration_s / 60, e.duration_s % 60
    )
}

fn meter_line(r: &MeterRow, heal: bool) -> text::Line {
    let text = format!("{} {:>7} {:>5}/s{}", fit(&r.name, NAME_COLS), compact(r.amount), r.per_s, if heal { "  +" } else { "" });
    text::Line { text, rgb: if r.is_you { YOU_ROW } else { WHITE } }
}
```

Replace `hud_lines` with:

```rust
/// Kill count, personal line, timer rows, damage rows, healing rows.
fn hud_lines(snap: &Snapshot) -> Vec<text::Line> {
    let mut lines = vec![text::Line { text: format!("{} kills", snap.session_kills), rgb: WHITE }];
    match &snap.encounter {
        Some(e) => lines.push(text::Line { text: personal_line(e), rgb: if e.active { WHITE } else { DIM } }),
        None => lines.push(text::Line { text: EMPTY_PERSONAL.to_string(), rgb: DIM }),
    }
    for t in snap.timers.iter().take(MAX_ROWS) {
        let spell = if t.rank == 0 { t.spell.clone() } else { format!("{} {}", t.spell, roman(t.rank)) };
        let secs = t.remaining_ms.div_euclid(1000).clamp(0, 9999);
        lines.push(text::Line { text: format_row(&t.target, &spell, secs), rgb: row_colour(t) });
    }
    if let Some(e) = &snap.encounter {
        lines.extend(e.damage.iter().take(MAX_DAMAGE_ROWS).map(|r| meter_line(r, false)));
        lines.extend(e.healing.iter().take(MAX_HEALING_ROWS).map(|r| meter_line(r, true)));
    }
    lines
}
```

Update the sizing probe in `main` so `widest` is: the kill line, the personal line at its widest (`"DPS 99999  in 9999/s  HPS 9999   99:59"`), `MAX_ROWS` timer rows as now, then `MAX_DAMAGE_ROWS + MAX_HEALING_ROWS` meter rows of `format!("{} {:>7} {:>5}/s  +", "W".repeat(NAME_COLS), "999.9k", 99999)`. Add `use wisp_proto::{Encounter, MeterRow};` to the existing `use` line.

- [ ] **Step 4: Build, test, look**

Run: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: clean; wisp-hud 30 tests.

With `cargo run -p wispd -- --stub` in the background: `timeout 10 cargo run -p wisp-hud -- --backend layer-shell`. Expected on stderr: the backend line and no errors. The panel itself (three damage rows, two healing rows, the personal line dimming after 45 s) is JDS300's to see; say so in the report. Kill everything.

- [ ] **Step 5: Commit**

```bash
git add crates/wisp-hud
git commit -m "$(cat <<'EOF'
wisp-hud: the fight panel

One personal line under the kill count -- DPS, damage taken per second,
HPS, fight time -- then the timer rows, then ranked damage and healing rows
for the fight, healing rows marked with a trailing plus, your rows in their
own colour. Amounts print compactly so the columns never move; the empty
panel has the same shape so the layout never jumps. The window is sized at
attach for the full set.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Milestone 4 — live (JDS300)

**Files:** `PROVENANCE.md`, `README.md`

No new code. With EverQuest Legends up: `wispd --log <live log>` and `wisp-hud`. Fight something with the group.

**Acceptance (spec §6, live):** the personal line updates within a second of each hit; group rows show groupmates ranked, your row highlighted; ten seconds after the last blow the panel holds for 30 s then clears; zoning clears it at once; restarting `wisp-hud` mid-fight shows the same numbers; the HUD still takes no input.

Record the outcome in `PROVENANCE.md` (dated) and the README's Spec 3 row, exactly as observed. Commit with the canonical trailer.

---

## Self-review

**Spec coverage.**

| Spec section | Task |
|---|---|
| §4 events (fourteen shapes, suffix stripping, amounts only) | 2 |
| §4 actors (five rules, case-insensitive keys, pet TTL, warder owner) | 3 |
| §4 encounters (open on damage, 10 s idle, boundary close, linger 30 s, replace) | 3 |
| §4 protocol v3 | 1 |
| §4 HUD (personal line, rows, compact amounts, `+`, colours, empty shape, sizing) | 5 |
| §5 milestones 1–3 | 2, 3, 4–5 |
| §5 milestone 4 / §6 live | 6 |
| §6 replay acceptance (every row of the table) | 3 |
| §3 amounts only; no game data; timestamps subtracted | 2, 3 (tracker clock as Spec 2) |

**Known gaps, stated:**

1. ~~`CombatEvent::Taken` with `DamageKind::Shield` cannot occur (no "You are … by X's" line in the fixture); the match arm is empty on purpose.~~ **Corrected in the final-review wave:** the line does exist — `YOU are <verb> by <source>'s <thing> for N points of non-melee damage!` (note `YOU are`, and the line ends in `!`, which is why the earlier grep for "You are" with a lower-case "You" and a trailing "." found nothing) — 19,077 times in the fixture. It is now classified and counted in a new `taken_shield` stat; the match arm is no longer empty.
2. Task 4's `stub_encounter` is decorative; its numbers are not asserted anywhere.
3. The reference marks targets as mobs inconsistently across line kinds (melee: when the source is not a mob; spells: when the source is a plain single word or an owned pet; DoT and shield: never). The tracker reproduces that exactly because the acceptance numbers depend on it; a cleaner rule is Spec 4's to make, with a new reference.

**Type consistency:** `Encounter`, `MeterRow`, `Personal` (proto) match Tasks 3, 4, 5; `Tracker::encounter(now_secs: f64)` and `last_time()` on the tracker's own clock in Tasks 3 and 4; `CombatEvent` variants and field names identical in Tasks 2 and 3; `EncounterStats` fields identical between the struct and the replay assertion; `fit`, `format_row`, `row_colour`, `MAX_ROWS` reused from Spec 2's `main.rs`.

---

## Appendix A — the reference replay

Run on 2026-09-08 against the frozen fixture; deterministic across runs. **Re-derived on 2026-09-09**, after the whole-branch final review found three line shapes this script had missed: other sources' DoT ticks print `from <Spell> by <source>`, not `from <source>'s <Spell>` (the old `oth_dot` pattern matched only when a spell name itself happened to carry an apostrophe — a bard song title — and credited the text before it as a phantom source); a damage shield can land on you (`YOU are <verb> by <source>'s <thing> for N points of non-melee damage!`); and a special attack can carry an `on` preposition before `YOU` (`<mob> <verb>s on YOU for N points of damage.`). **Extended again on 2026-09-09**, same day, after JDS300's live test of PR #3 found the group rows showing players outside his group and himself twice (once for damage, once for healing): the script now also tracks group membership, learned and forgotten from the log's own lines, and prints the group left standing at end of file. Throwaway Python; the arbiter of the spec's rules for Task 3; recorded here, not shipped.

```python
#!/usr/bin/env python3
"""Reference implementation of Spec 3 (encounters, damage, healing) over the frozen fixture.

Independent of the Rust code. Every rule here is the spec's rule; the numbers it
prints are the acceptance numbers. Throwaway; recorded in the plan, never shipped.
"""
import re, datetime, collections

FIXTURE = "/mnt/Data4TB/Games/everquest/fixtures/eqlog_Daggo_freeport.1440036.txt"
PLAYER = "Daggo"            # from the log filename eqlog_Daggo_freeport.txt
IDLE_SECS = 10              # a fight ends after this long without damage or heal
PET_TTL_SECS = 120          # a name is "your pet" for this long after each Master announcement
ARTICLES = ("a ", "an ", "the ")

ts_re = re.compile(r"^\[(.*?)\] (.*)$")
# damage out / taken
own_melee = re.compile(r"^You (\w+) (.+?) for (\d+) points of damage\.")
own_nuke = re.compile(r"^You hit (.+?) for (\d+) points of ([a-z]+) damage by (.+?)\.")
own_dot = re.compile(r"^(.+?) has taken (\d+) damage from your (.+?)\.")
own_ds = re.compile(r"^(.+?) is (\w+) by YOUR (.+?) for (\d+) points of non-melee damage\.")
oth_melee = re.compile(r"^(.+?) (\w+?)s (.+?) for (\d+) points of damage\.")
oth_nuke = re.compile(r"^(.+?) hit (.+?) for (\d+) points of ([a-z]+) damage by (.+?)\.")
oth_dot = re.compile(r"^(.+?) has taken (\d+) damage from (.+?) by (.+?)\.")
oth_ds = re.compile(r"^(.+?) is (\w+) by (.+?)'s (.+?) for (\d+) points of non-melee damage\.")
taken_melee = re.compile(r"^(.+?) (\w+?)s(?: on)? YOU for (\d+) points of damage\.")
taken_nuke = re.compile(r"^(.+?) hit you for (\d+) points of ([a-z]+) damage by (.+?)\.")
taken_dot = re.compile(r"^You have taken (\d+) damage from (.+?) by (.+?)\.")
taken_ds = re.compile(r"^YOU are (\w+) by (.+?)'s (.+?) for (\d+) points of non-melee damage!")
# heals
own_heal = re.compile(r"^You healed (.+?)( over time)? for (\d+)(?: \((\d+)\))? hit points by (.+?)\.")
oth_heal = re.compile(r"^(.+?) healed (.+?)( over time)? for (\d+)(?: \((\d+)\))? hit points(?: by (.+?))?\.")
# boundaries / pets
pet_announce = re.compile(r"^(.+?) (?:tells|told) you, 'Attacking .* Master\.'")
zone = re.compile(r"^(You have entered |LOADING, PLEASE WAIT|Welcome to EverQuest Legends!)")
# group membership, learned from the log's own lines
grp_member = re.compile(r"^(?:(.+?) has joined the group\.|(.+?) invites you to join a group\.|You notify (.+?) that you agree to join the group\.|(.+?) is now the leader of your group\.|(.+?) is now group Main Assist|(.+?) tells the group, )")
grp_leave = re.compile(r"^(.+?) (?:has left the group|has been removed from the group)\.")
grp_reset = re.compile(r"^(You have joined the group\.|You have been removed from the group\.|You have left the group\.|Your group has been disbanded\.)")

def logtime(ts):
    return int(datetime.datetime.strptime(ts, "%a %b %d %H:%M:%S %Y").timestamp())

def is_mobish(name):
    return name.lower().startswith(ARTICLES)

def owner_of(name):
    # "Jennie`s warder" / "Jennie`s pet" -> Jennie
    m = re.match(r"^(.+?)`s (warder|pet)$", name)
    return m.group(1) if m else None

class Ref:
    def __init__(self):
        self.pets = {}              # lower-cased pet name -> last announcement time
        self.mobs = set()           # lower-cased names ever targeted by self/pet or that hit YOU
        self.c = collections.Counter()
        self.enc = None
        self.finished = []
        self.group = set()
    # ---- actor resolution
    def source(self, name, now):
        if name in ("You", "YOUR", "you", "your") or name == PLAYER:
            return "you", "self"
        if name.lower() in self.pets and now - self.pets[name.lower()] <= PET_TTL_SECS:
            return "you", "pet"
        o = owner_of(name)
        if o:
            return (("you", "pet") if o == PLAYER else (o, "pet"))
        if is_mobish(name) or name.lower() in self.mobs or " " in name:
            return name, "mob"
        return name, "player"
    # ---- encounter
    def touch(self, now, kind):
        if self.enc is None:
            if kind == "heal":
                return None
            self.enc = {"start": now, "last": now, "dmg": collections.Counter(), "taken": 0, "taken_by": collections.Counter(),
                        "heal": collections.Counter(), "over": collections.Counter(), "n": 0}
            self.c["encounters"] += 1
        self.enc["last"] = now
        return self.enc
    def expire(self, now):
        if self.enc and now - self.enc["last"] > IDLE_SECS:
            self.close()
    def close(self):
        if self.enc:
            e = self.enc; e["dur"] = max(1, e["last"] - e["start"]); self.finished.append(e); self.enc = None
    # ---- events
    def damage_out(self, src, amount, now, kind):
        who, role = self.source(src, now)
        if role == "mob":
            return
        e = self.touch(now, "damage")
        e["dmg"][who] += amount
        self.c["dmg_out_" + kind] += amount
        if who == "you":
            self.c["own_" + role + "_dmg"] += amount
    def damage_taken(self, src, amount, now, kind):
        self.mobs.add(src.lower())
        e = self.touch(now, "damage")
        e["taken"] += amount; e["taken_by"][src] += amount
        self.c["taken_" + kind] += amount
    def heal(self, src, tgt, actual, potential, now, hot):
        who, role = self.source(src, now)
        if role == "mob":
            return
        e = self.touch(now, "heal")
        if e is None:
            self.c["heal_outside_fight"] += actual
            return
        e["heal"][who] += actual; e["over"][who] += (potential - actual)
        self.c["heal_actual"] += actual; self.c["heal_over"] += (potential - actual)
        if who == "you":
            self.c["own_heal_actual"] += actual; self.c["own_heal_over"] += (potential - actual)
            if hot: self.c["own_hot_actual"] += actual

    def line(self, now, body):
        self.expire(now)
        m = pet_announce.match(body)
        if m:
            self.pets[m.group(1).lower()] = now; self.c["pet_announcements"] += 1; return
        if zone.match(body):
            self.close(); self.c["zone_or_session"] += 1; return
        if grp_reset.match(body):
            self.group.clear(); self.c["group_resets"] += 1; return
        m = grp_leave.match(body)
        if m:
            self.group.discard(m.group(1).lower()); self.c["group_leaves"] += 1; return
        m = grp_member.match(body)
        if m:
            name = next(g for g in m.groups() if g)
            self.group.add(name.lower()); self.c["group_member_lines"] += 1; return
        # damage taken (target YOU) first: these mention YOU explicitly
        m = taken_melee.match(body)
        if m: self.damage_taken(m.group(1), int(m.group(3)), now, "melee"); return
        m = taken_nuke.match(body)
        if m: self.damage_taken(m.group(1), int(m.group(2)), now, "spell"); return
        m = taken_dot.match(body)
        if m: self.damage_taken(m.group(3), int(m.group(1)), now, "dot"); return
        m = taken_ds.match(body)
        if m: self.damage_taken(m.group(2), int(m.group(4)), now, "shield"); return
        # own damage out
        m = own_nuke.match(body)
        if m: self.mobs.add(m.group(1).lower()); self.damage_out("You", int(m.group(2)), now, "spell"); return
        m = own_melee.match(body)
        if m:
            self.mobs.add(m.group(2).lower()); self.damage_out("You", int(m.group(3)), now, "melee"); return
        m = own_dot.match(body)
        if m: self.mobs.add(m.group(1).lower()); self.damage_out("You", int(m.group(2)), now, "dot"); return
        m = own_ds.match(body)
        if m: self.mobs.add(m.group(1).lower()); self.damage_out("You", int(m.group(4)), now, "ds"); return
        # heals
        m = own_heal.match(body)
        if m: self.heal("You", m.group(1), int(m.group(3)), int(m.group(4) or m.group(3)), now, bool(m.group(2))); return
        m = oth_heal.match(body)
        if m: self.heal(m.group(1), m.group(2), int(m.group(4)), int(m.group(5) or m.group(4)), now, bool(m.group(3))); return
        # others' damage out (target must not be YOU; handled above)
        m = oth_nuke.match(body)
        if m:
            src, tgt = m.group(1), m.group(2)
            if not is_mobish(src) and " " not in src or owner_of(src):
                self.mobs.add(tgt.lower())
            self.damage_out(src, int(m.group(3)), now, "spell"); return
        m = oth_dot.match(body)
        if m: self.damage_out(m.group(4), int(m.group(2)), now, "dot"); return
        m = oth_ds.match(body)
        if m: self.damage_out(m.group(3), int(m.group(5)), now, "ds"); return
        m = oth_melee.match(body)
        if m:
            src, tgt = m.group(1), m.group(3)
            who, role = self.source(src, now)
            if role != "mob":
                self.mobs.add(tgt.lower())
            self.damage_out(src, int(m.group(4)), now, "melee"); return

def main():
    r = Ref()
    with open(FIXTURE, encoding="utf-8", errors="replace") as f:
        for raw in f:
            m = ts_re.match(raw.rstrip("\r\n"))
            if not m: continue
            r.line(logtime(m.group(1)), m.group(2))
    r.close()
    c = r.c
    print("encounters", c["encounters"], "finished", len(r.finished))
    for k in sorted(c): print(f"  {k} = {c[k]}")
    own_total = sum(e["dmg"]["you"] for e in r.finished)
    print("own damage in fights (all kinds, incl. pet) =", own_total)
    big = max(r.finished, key=lambda e: e["dmg"]["you"])
    print("largest own-damage fight: own", big["dmg"]["you"], "dur", big["dur"], "dps", round(big["dmg"]["you"]/big["dur"]),
          "taken", big["taken"], "top sources", big["dmg"].most_common(3), "top healers", big["heal"].most_common(2))
    longest = max(r.finished, key=lambda e: e["dur"])
    print("longest fight: dur", longest["dur"], "sources", len(longest["dmg"]))
    allsrc = collections.Counter()
    for e in r.finished: allsrc.update(e["dmg"])
    print("top damage sources overall:", allsrc.most_common(6))
    allheal = collections.Counter()
    for e in r.finished: allheal.update(e["heal"])
    print("top healers overall:", allheal.most_common(4))
    print("own pet names seen (lower-cased):", sorted(r.pets)[:8], "count", len(r.pets))
    top3 = sorted(allsrc.items(), key=lambda kv: (-kv[1], kv[0]))[:4]
    print("top damage sources (amount desc, name asc):", top3)
    durs = [e["dur"] for e in r.finished]
    print("group at end", sorted(r.group))
    print("fight duration: min", min(durs), "median", sorted(durs)[len(durs)//2], "max", max(durs), "sum", sum(durs))

main()
```

Its output on 2026-09-09 (run directly against Python 3.14.7; deterministic across two runs):

```
encounters 2524 finished 2524
  dmg_out_dot = 4622907
  dmg_out_ds = 447208
  dmg_out_melee = 15198942
  dmg_out_spell = 9082968
  encounters = 2524
  group_leaves = 4
  group_member_lines = 40
  group_resets = 7
  heal_actual = 2364526
  heal_outside_fight = 259121
  heal_over = 838539
  own_heal_actual = 1875603
  own_heal_over = 590911
  own_hot_actual = 355478
  own_pet_dmg = 5284285
  own_self_dmg = 18085260
  pet_announcements = 4890
  taken_dot = 273054
  taken_melee = 1924867
  taken_shield = 257300
  taken_spell = 709376
  zone_or_session = 971
own damage in fights (all kinds, incl. pet) = 23369545
largest own-damage fight: own 212467 dur 482 dps 441 taken 21416 top sources [('you', 212467)] top healers [('you', 14023)]
longest fight: dur 738 sources 1
top damage sources overall: [('you', 23369545), ('Yder', 1447129), ('Serenitee', 1321322), ('Misery', 1000700), ('Jennie', 420686), ('Ludaxe', 378122)]
top healers overall: [('you', 1875603), ('Serenitee', 196052), ('Misery', 116859), ('Jennie', 41346)]
own pet names seen (lower-cased): ['a barbed bone skeleton', 'a carrion ghoul', 'a cauldron hammerhead', 'a cauldron shark', 'a dry bone skeleton', 'a fetid fiend', 'a fire giant warrior', 'a flouting gargoyle'] count 91
top damage sources (amount desc, name asc): [('you', 23369545), ('Yder', 1447129), ('Serenitee', 1321322), ('Misery', 1000700)]
group at end []
fight duration: min 1 median 36 max 738 sum 146813
```

Notes for the Rust: Python's `\w` in the melee verb is any word character; the fixture's verbs are ASCII letters. `(.+?) for (\d+)` is a leftmost split; the Rust uses the rightmost ` for ` before the amount, which agrees on every fixture line (no name contains ` for `). The heal target is captured but never used by either. The other-DoT split now takes the *last* ` by ` in the remainder after `damage from`, matching the Rust's `rsplit_once(" by ")`; both agree because no spell name in the fixture contains its own ` by `. Rust's `f64::round()` rounds half away from zero; Python's `round()` rounds half to even — the two disagree only exactly at `x.5`, and none of the numbers above land on that boundary, so every rate and DPS value here agrees between the two languages. The group patterns are checked in the order reset, leave, member, so `You have joined the group.` is a reset and never a member line; `grp_member`'s six alternatives capture into different groups, so `next(g for g in m.groups() if g)` takes whichever one matched — the Rust's `classify` does the equivalent with separate `if let` arms.

---

## Execution handoff

Plan complete. Execute with `superpowers:subagent-driven-development`. Task 1 first; Tasks 2 and 5 may run in parallel after it (Task 5 needs only the proto types and the stub of Task 4 for eyeballing, which can wait); Task 3 needs 2; Task 4 needs 3; Task 6 is JDS300's.
