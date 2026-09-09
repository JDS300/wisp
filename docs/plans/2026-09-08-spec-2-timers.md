# Wisp Spec 2 — "Timers" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Countdown rows on the overlay for the spells the player lands on mobs — mez, lull, slows, DoTs, resist debuffs, charm — with durations read from the game's own spell data and corrected from the player's own log.

**Architecture:** Everything that decides lives in `wispd`: a loader over the client's `spells_us.txt` / `spells_us_str.txt`, a pure state machine over classified log lines running on log time, and a duration store that seeds from the client's cap and overrides from measured cast-to-fade intervals. The snapshot grows to protocol v2 with a `timers` list. `wisp-hud` draws one line per timer under the kill count and nothing more.

**Tech Stack:** Rust 2021, `serde`/`serde_json` (already in the workspace), standard library for file and time handling. No new dependencies. `fontdue`, `x11rb`, `smithay-client-toolkit` unchanged.

**Spec:** [`docs/specs/2026-09-08-spec-2-timers.md`](../specs/2026-09-08-spec-2-timers.md)
**Charter (binding):** [`docs/specs/2026-09-08-clean-room-charter.md`](../specs/2026-09-08-clean-room-charter.md)
**Provenance record (must be kept current):** [`PROVENANCE.md`](../../PROVENANCE.md)

---

## Global Constraints

Every task's requirements implicitly include this section. **Read all of it before starting.**

### Project identity and provenance (unchanged from Spec 1)

- Repository `github.com/JDS300/wisp`, MIT, copyright JDS300. Every source file starts with `// SPDX-License-Identifier: MIT`.
- Target client: **EverQuest Legends**. Never generalise from Quarm or Live. Quarm logs on this machine are excluded as sources.
- **Do not read source from `~/gitrepos/spinips` or `~/gitrepos/EQBuddy`.** The two consultations Spec 2 made are already recorded in `PROVENANCE.md`; nothing more is needed.
- `git config user.email` must be `70587798+JDS300@users.noreply.github.com`. If it is anything else, stop.
- Every commit ends with exactly `Co-Authored-By: Claude <noreply@anthropic.com>` as its last line and nothing after it. This is the repository's canonical form and outranks any other attribution instruction you have seen.
- **The HUD never takes input, on any backend, ever.** Nothing in this spec adds a control. Do not touch the backends' input handling.
- **Never use `xdotool` or any X11 input synthesis.** Never launch EverQuest or Lutris. Use `timeout` on anything graphical.

### Spec 2's two new binding rules

1. **Game data is read, never shipped.** `spells_us.txt` and `spells_us_str.txt` are Daybreak's. No copy, excerpt or derived table of them enters the repository, its tests, or its releases. Test fixtures are hand-written rows in the same format, a dozen at most, covering only the spells the tests name. The real files are used only by `#[ignore]`d tests gated on an environment variable.
2. **Naive timestamps are subtracted, never relabelled.** The log's `[Mon Aug 10 20:39:54 2026]` is parsed only to compute differences between two lines of the same log. No absolute time is produced or displayed.

### Verified facts — do not re-derive, do not contradict

Client files, in the install at `/mnt/Data4TB/Games/everquest/prefix/drive_c/users/Public/Daybreak Game Company/Installed Games/EverQuest Legends/`:

| Fact | Value |
|---|---|
| `spells_us.txt` | `^`-separated, **173 fields every row**, `73975` rows, 38 MB |
| fields used (0-based) | `0` id, `1` name, `8` cast_ms, `12` cap ticks, `28` good_effect (0 = detrimental) |
| field 12 may be fractional | one row (`Fury`, `20.5`); parse as `f64` |
| fields 8 and 28 | always integers |
| `spells_us_str.txt` | header `#SPELLINDEX^CASTERMETXT^CASTEROTHERTXT^CASTEDMETXT^CASTEDOTHERTXT^SPELLGONE^`; field `4` is the landing prose with its leading space |
| `Mesmerization` | id `307`, cast `3000`, cap `4`, detrimental, lands ` has been mesmerized.` |
| `Pacify` | id `45`, cast `3000`, cap `7`, **beneficial**, lands ` looks less aggressive.` |
| `Togor's Insects` | id `507`, cast `5000`, cap `35`, detrimental, lands ` yawns.` |
| eligible spells (cap > 0 and detrimental-or-lull) | `12245` |

Frozen fixture (the live log grows; this does not):
`/mnt/Data4TB/Games/everquest/fixtures/eqlog_Daggo_freeport.1440036.txt`, 1,440,036 lines, SHA-256 begins `70a95ca40bc701cf`. Spec 1's counts hold on it (3722 / 3065 / 39).

Log line shapes (all verified on the frozen fixture; `<name>` includes the article):

| Line | Count |
|---|---|
| `You begin casting <Spell>[ <ROMAN>].` | 17,131 |
| `Your <Spell> spell fizzles!` | 15 |
| `Your <Spell> spell is interrupted.` | 801 |
| `<name> resisted your <Spell>!` | 1,814 |
| `<name> has taken <N> damage from your <Spell>.` | 21,660 |
| `Your <Spell> spell has worn off of <name>.` (rank stripped) | 3,055 |
| `<name> has been awakened by <who>.` | 775 |
| `You have slain <name>!` / `<name> has been slain by <who>!` | 3,722 / 3,065 |
| `You have entered <zone>.` / `LOADING, PLEASE WAIT...` | 494 / 440 |

**Article capitalisation depends on the sentence:** awakened, DoT tick and slain-by lines print `A …`; mesmerized, yawns, worn-off and own-kill lines print `a …`. Target keys are lower-cased.

### The reference replay and its numbers

The Python script in Appendix A is the reference implementation of spec §4. It was run on the frozen fixture with the real client files on 2026-09-08 and is deterministic (two runs, identical output). Task 5's replay test must reproduce these numbers **exactly**:

| Counter | Expected |
|---|---|
| eligible spells | 12245 |
| pending armed / cancelled / expired | 8042 / 730 / 399 |
| armed total / mez / dot / debuff | 5972 / 822 / 214 / 4936 |
| promoted debuff→dot | 2097 |
| tick heartbeats | 12814 |
| ended worn-off / awakened / slain / expired | 1660 / 19 / 2042 / 2107 |
| cleared by zone | 144 |
| samples / discarded short | 1060 / 600 |
| active rows / pending at end | 0 / 0 |

Learned at end: `Mesmerization|6` samples `[22,27,21,27,19,28,24,22,28]` median `24`; `Pacify|5` = 69; `Venom of the Snake|0` = 38; `Envenomed Bolt|10` = 57; `Odium|10` = 50.

If your implementation disagrees with a number, the reference is the arbiter of the spec's rules **unless you can show the reference violates the spec text**; in that case stop and report it with the line of the reference and the line of the spec.

### Tooling notes

- Run the full workspace once before each commit: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets`; all three must be clean.
- Env-gated tests: `WISP_EQL_DIR=<install dir> WISP_FIXTURE=<frozen fixture> cargo test -p wispd -- --ignored` runs them. They are skipped (not failed) when the variables are unset, and never run in the default suite.

---

## File Structure

```
crates/
├── wisp-proto/src/lib.rs            v2: Timer, TimerKind, Confidence, Snapshot.timers   (Task 1)
├── wispd/src/
│   ├── main.rs                      --spells, table load, tracker, log-time estimate    (Task 6)
│   ├── spells.rs                    SpellTable loader. Pure parse + file load.           (Task 2)
│   ├── rules.rs                     + split_rank, parse_log_time, Event, classify        (Task 3)
│   ├── durations.rs                 seed_secs, DurationStore (JSON, atomic save)         (Task 4)
│   └── timers.rs                    Tracker: the state machine on log time              (Task 5)
└── wisp-hud/src/
    ├── text.rs                      + Line, render_lines (colour, stacked rows)         (Task 7)
    └── main.rs                      rows from snapshot, thresholds, window sizing       (Task 7)
```

`spells.rs`, `rules.rs`, `durations.rs` and `timers.rs` are pure over their inputs and carry the tests. `main.rs` in each crate is the only place that touches the clock, the filesystem paths, and the display.

---

## Task 1: Protocol v2

**Files:**
- Modify: `crates/wisp-proto/src/lib.rs`
- Modify: `crates/wispd/src/main.rs` (the `Snapshot { … }` literal, lines 68–74)
- Modify: `crates/wispd/src/server.rs` (the `snapshot()` test helper only)
- Modify: `crates/wisp-hud/src/client.rs` (the JSON literals in tests only)

**Interfaces:**
- Consumes: nothing.
- Produces: `wisp_proto::PROTOCOL_VERSION == 2`, `wisp_proto::TimerKind { Mez, Dot, Debuff }`, `wisp_proto::Confidence { Measured, Estimated }`, `wisp_proto::Timer { target: String, spell: String, rank: u8, kind: TimerKind, remaining_ms: i64, duration_ms: u64, confidence: Confidence }`, `Snapshot.timers: Vec<Timer>` (serde default, so a v1 line decodes far enough to be refused by version).

- [ ] **Step 1: Write the failing tests**

Add to the existing `mod tests` in `crates/wisp-proto/src/lib.rs` (keep the five existing tests; `sample()` gains `timers: Vec::new()`):

```rust
    fn mez() -> Timer {
        Timer {
            target: "a jeering gargoyle".to_string(),
            spell: "Mesmerization".to_string(),
            rank: 6,
            kind: TimerKind::Mez,
            remaining_ms: 11_800,
            duration_ms: 38_000,
            confidence: Confidence::Measured,
        }
    }

    #[test]
    fn timers_round_trip() {
        let mut s = sample();
        s.timers = vec![mez()];
        let decoded = decode(&encode(&s)).unwrap();
        assert_eq!(decoded, s);
        assert_eq!(decoded.timers[0].kind, TimerKind::Mez);
    }

    #[test]
    fn timer_kinds_and_confidence_serialise_lowercase() {
        let line = encode(&Snapshot { timers: vec![mez()], ..sample() });
        assert!(line.contains(r#""kind":"mez""#), "{line}");
        assert!(line.contains(r#""confidence":"measured""#), "{line}");
    }

    #[test]
    fn a_v1_line_is_refused_by_version_not_by_shape() {
        let v1 = r#"{"v":1,"seq":1,"ts":"x","lines_ingested":0,"session_kills":0}"#;
        match decode(v1) {
            Err(ProtoError::Version { found: 1, expected: 2 }) => {}
            other => panic!("expected a version error, got {other:?}"),
        }
    }
```

Also update `rejects_unknown_protocol_version`'s literal to include `"timers":[]` (it may stay without; the default handles it — leave it as is).

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wisp-proto`
Expected: FAIL to compile — `Timer`, `TimerKind`, `Confidence` do not exist; `sample()` lacks `timers`.

- [ ] **Step 3: Implement**

In `crates/wisp-proto/src/lib.rs`, change the version and add the types above `Snapshot`:

```rust
/// Bumped whenever the snapshot shape changes incompatibly.
/// 1: Spec 1 counters. 2: Spec 2 adds `timers`.
pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerKind {
    Mez,
    Dot,
    Debuff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Duration is the median of measured cast-to-fade intervals.
    Measured,
    /// Duration is seeded from the client's cap and the rank; not yet observed.
    Estimated,
}

/// One active countdown on a mob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timer {
    /// The target as the landing line printed it, e.g. `a jeering gargoyle`.
    pub target: String,
    /// Base spell name without rank, as the client data names it.
    pub spell: String,
    /// 0 when the cast line carried no numeral.
    pub rank: u8,
    pub kind: TimerKind,
    /// May be negative during the post-expiry hold.
    pub remaining_ms: i64,
    pub duration_ms: u64,
    pub confidence: Confidence,
}
```

and add to `Snapshot`, after `session_kills`:

```rust
    /// Active timers, soonest expiry first, at most 16. Absent on v1 lines.
    #[serde(default)]
    pub timers: Vec<Timer>,
```

Then fix the three other crates so the workspace compiles:

- `crates/wispd/src/main.rs`: add `timers: Vec::new(),` to the `Snapshot { … }` literal.
- `crates/wispd/src/server.rs` tests: add `timers: Vec::new(),` to the `snapshot()` helper's literal.
- `crates/wisp-hud/src/client.rs` tests: every JSON literal `{"v":1,…}` becomes `{"v":2,…}` (the `"v":99` version-mismatch literal stays 99). No `timers` key is needed thanks to the default.

- [ ] **Step 4: Run the whole workspace and confirm it passes**

Run: `cargo test --workspace`
Expected: PASS — wisp-proto 8 tests, wispd 20, wisp-hud 18. No warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/wisp-proto crates/wispd/src/main.rs crates/wispd/src/server.rs crates/wisp-hud/src/client.rs
git commit -m "$(cat <<'EOF'
Protocol v2: snapshots carry active timers

One list, soonest expiry first, at most sixteen entries. A timer names its
target as the log printed it, its base spell and rank, its kind, remaining
and total milliseconds, and whether the duration was measured from this
player's own logs or merely seeded from the client's cap.

The field defaults to empty on decode so a v1 line is refused for its
version, which is the honest error, rather than for its shape.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: The spell table

**Files:**
- Create: `crates/wispd/src/spells.rs`
- Modify: `crates/wispd/src/main.rs` (add `#[allow(dead_code)] // wired in Task 6` + `mod spells;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `spells::LULL_PROSE: &str`, `spells::MEZ_PROSE: &str`, `spells::SPELLS_FILE`, `spells::STRINGS_FILE`, `spells::SpellInfo { id: u32, name: String, cast_ms: u32, cap_ticks: f64, detrimental: bool, lands_as: Option<String> }`, `spells::SpellsError`, `spells::SpellTable::{parse(spells: &str, strings: &str) -> Result<SpellTable, SpellsError>, load(dir: &Path) -> Result<SpellTable, SpellsError>, get(&self, name: &str) -> Option<&SpellInfo>, len(&self) -> usize, is_empty(&self) -> bool, rows_parsed(&self) -> usize}`, `spells::spells_dir_from_log(log: &Path) -> Option<PathBuf>`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/wispd/src/spells.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // Hand-written rows in the client's format. Five fields are real
    // (id, name, cast_ms at 8, cap at 12, good_effect at 28); the rest are
    // filler so every row has exactly 173 fields. No game data is copied.
    fn row(id: u32, name: &str, cast_ms: u32, cap: &str, good: u32) -> String {
        let mut f: Vec<String> = vec!["0".to_string(); 173];
        f[0] = id.to_string();
        f[1] = name.to_string();
        f[8] = cast_ms.to_string();
        f[12] = cap.to_string();
        f[28] = good.to_string();
        f.join("^")
    }

    fn spells_text() -> String {
        [
            row(1, "Sleep", 3000, "4", 0),        // detrimental mez-like
            row(2, "Calm", 3000, "7", 1),         // beneficial lull
            row(3, "Ward", 2000, "10", 1),        // beneficial buff: not eligible
            row(4, "Jab", 1000, "0", 0),          // no duration: not eligible
            row(5, "Sleep", 3000, "9", 0),        // duplicate name, higher id: loses
            row(6, "Chill", 2000, "20.5", 0),     // fractional cap
        ]
        .join("\n")
            + "\n"
    }

    fn strings_text() -> String {
        "#SPELLINDEX^CASTERMETXT^CASTEROTHERTXT^CASTEDMETXT^CASTEDOTHERTXT^SPELLGONE^\n\
         1^^^You are mesmerized.^ has been mesmerized.^You are no longer mesmerized.^\n\
         2^^^You feel your aggression subside.^ looks less aggressive.^^\n\
         3^^^You feel protected.^ looks protected.^^\n\
         6^^^^ shivers.^^\n"
            .to_string()
    }

    #[test]
    fn eligible_rows_are_loaded_with_their_prose() {
        let t = SpellTable::parse(&spells_text(), &strings_text()).unwrap();
        let sleep = t.get("Sleep").unwrap();
        assert_eq!(sleep.id, 1, "lower id wins a duplicate name");
        assert_eq!(sleep.cast_ms, 3000);
        assert_eq!(sleep.cap_ticks, 4.0);
        assert!(sleep.detrimental);
        assert_eq!(sleep.lands_as.as_deref(), Some(MEZ_PROSE));
    }

    #[test]
    fn a_beneficial_lull_is_eligible_by_its_prose() {
        let t = SpellTable::parse(&spells_text(), &strings_text()).unwrap();
        let calm = t.get("Calm").unwrap();
        assert!(!calm.detrimental);
        assert_eq!(calm.lands_as.as_deref(), Some(LULL_PROSE));
    }

    #[test]
    fn buffs_and_instant_spells_are_not_eligible() {
        let t = SpellTable::parse(&spells_text(), &strings_text()).unwrap();
        assert!(t.get("Ward").is_none(), "beneficial, not a lull");
        assert!(t.get("Jab").is_none(), "cap 0");
        assert_eq!(t.len(), 3);
        assert_eq!(t.rows_parsed(), 6);
    }

    #[test]
    fn a_fractional_cap_parses() {
        let t = SpellTable::parse(&spells_text(), &strings_text()).unwrap();
        assert_eq!(t.get("Chill").unwrap().cap_ticks, 20.5);
    }

    #[test]
    fn a_row_with_the_wrong_field_count_is_a_loud_error() {
        let bad = "7^Broken^0\n";
        match SpellTable::parse(bad, &strings_text()) {
            Err(SpellsError::Format { line, .. }) => assert_eq!(line, 1),
            other => panic!("expected a format error, got {other:?}"),
        }
    }

    #[test]
    fn a_spell_without_prose_still_loads() {
        // The strings file has no row for id 5's name-sake; parse a table
        // where the eligible spell simply has no landing text.
        let spells = row(9, "Silent", 1000, "3", 0) + "\n";
        let t = SpellTable::parse(&spells, &strings_text()).unwrap();
        assert_eq!(t.get("Silent").unwrap().lands_as, None);
    }

    #[test]
    fn the_install_dir_is_the_parent_of_the_logs_dir() {
        let log = Path::new("/games/EverQuest Legends/Logs/eqlog_Daggo_freeport.txt");
        assert_eq!(
            spells_dir_from_log(log),
            Some(PathBuf::from("/games/EverQuest Legends"))
        );
        assert_eq!(spells_dir_from_log(Path::new("/tmp/eqlog_x.txt")), None);
    }

    /// Against the real client. Skipped unless WISP_EQL_DIR is set; run with
    /// `WISP_EQL_DIR=<install> cargo test -p wispd -- --ignored`.
    #[test]
    #[ignore]
    fn real_client_files_load_as_the_spec_records() {
        let Some(dir) = std::env::var_os("WISP_EQL_DIR") else { return };
        let t = SpellTable::load(Path::new(&dir)).expect("real client files load");
        assert_eq!(t.rows_parsed(), 73975);
        assert_eq!(t.len(), 12245);
        let mez = t.get("Mesmerization").unwrap();
        assert_eq!((mez.id, mez.cast_ms, mez.cap_ticks, mez.detrimental), (307, 3000, 4.0, true));
        assert_eq!(mez.lands_as.as_deref(), Some(MEZ_PROSE));
        let pac = t.get("Pacify").unwrap();
        assert_eq!((pac.id, pac.cap_ticks, pac.detrimental), (45, 7.0, false));
        assert_eq!(pac.lands_as.as_deref(), Some(LULL_PROSE));
        let tog = t.get("Togor's Insects").unwrap();
        assert_eq!((tog.id, tog.cast_ms, tog.cap_ticks), (507, 5000, 35.0));
        assert_eq!(tog.lands_as.as_deref(), Some(" yawns."));
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wispd spells`
Expected: FAIL to compile — `SpellTable` does not exist.

- [ ] **Step 3: Implement the loader**

Add above the `mod tests` block:

```rust
// SPDX-License-Identifier: MIT
//! The client's own spell data, read from the install and never shipped.
//!
//! Two files sit beside the game's `Logs/` directory: `spells_us.txt`
//! (173 `^`-separated fields per row) and `spells_us_str.txt` (the message
//! text, including the prose printed on a target when a spell lands). Five
//! fields of the first and one of the second are all Wisp needs. The layout
//! is documented by `amerzel/eql-info` (MIT, see THIRD_PARTY.md) and was
//! confirmed against the local files.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::{fs, io};

pub const SPELLS_FILE: &str = "spells_us.txt";
pub const STRINGS_FILE: &str = "spells_us_str.txt";
/// Landing prose of the lull family, which is beneficial in the data but
/// cast on mobs in practice. The one exception to "detrimental only".
pub const LULL_PROSE: &str = " looks less aggressive.";
/// Landing prose that marks a row as a mez.
pub const MEZ_PROSE: &str = " has been mesmerized.";

const FIELD_COUNT: usize = 173;
const F_ID: usize = 0;
const F_NAME: usize = 1;
const F_CAST_MS: usize = 8;
const F_CAP_TICKS: usize = 12;
const F_GOOD_EFFECT: usize = 28;
const S_ID: usize = 0;
const S_CAST_ON_OTHER: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct SpellInfo {
    pub id: u32,
    pub name: String,
    pub cast_ms: u32,
    pub cap_ticks: f64,
    pub detrimental: bool,
    /// The text the log prints after the target's name when this lands,
    /// leading space included. `None` when the client has no such text.
    pub lands_as: Option<String>,
}

#[derive(Debug)]
pub enum SpellsError {
    Io { file: String, source: io::Error },
    Format { file: String, line: usize, reason: String },
}

impl fmt::Display for SpellsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpellsError::Io { file, source } => write!(f, "cannot read {file}: {source}"),
            SpellsError::Format { file, line, reason } => {
                write!(f, "{file} line {line}: {reason}")
            }
        }
    }
}

impl std::error::Error for SpellsError {}

pub struct SpellTable {
    by_name: HashMap<String, SpellInfo>,
    rows_parsed: usize,
}

impl SpellTable {
    /// Parse the two files' contents. Pure; the tests feed hand-written rows.
    pub fn parse(spells: &str, strings: &str) -> Result<SpellTable, SpellsError> {
        let mut prose: HashMap<u32, String> = HashMap::new();
        for (i, line) in strings.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = line.split('^').collect();
            if f.len() <= S_CAST_ON_OTHER {
                return Err(SpellsError::Format {
                    file: STRINGS_FILE.to_string(),
                    line: i + 1,
                    reason: format!("expected at least {} fields, found {}", S_CAST_ON_OTHER + 1, f.len()),
                });
            }
            let id = parse_int(f[S_ID], STRINGS_FILE, i + 1, "id")? as u32;
            if !f[S_CAST_ON_OTHER].is_empty() {
                prose.insert(id, f[S_CAST_ON_OTHER].to_string());
            }
        }

        let mut by_name: HashMap<String, SpellInfo> = HashMap::new();
        let mut rows_parsed = 0;
        for (i, line) in spells.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split('^').collect();
            if f.len() != FIELD_COUNT {
                return Err(SpellsError::Format {
                    file: SPELLS_FILE.to_string(),
                    line: i + 1,
                    reason: format!("expected {FIELD_COUNT} fields, found {}", f.len()),
                });
            }
            rows_parsed += 1;
            let id = parse_int(f[F_ID], SPELLS_FILE, i + 1, "id")? as u32;
            let cast_ms = parse_int(f[F_CAST_MS], SPELLS_FILE, i + 1, "cast time")? as u32;
            let cap_ticks = parse_float(f[F_CAP_TICKS], SPELLS_FILE, i + 1, "duration cap")?;
            let good = parse_int(f[F_GOOD_EFFECT], SPELLS_FILE, i + 1, "good_effect")?;
            let lands_as = prose.get(&id).cloned();

            if cap_ticks <= 0.0 {
                continue;
            }
            let detrimental = good == 0;
            if !(detrimental || lands_as.as_deref() == Some(LULL_PROSE)) {
                continue;
            }
            let name = f[F_NAME].to_string();
            let info = SpellInfo { id, name: name.clone(), cast_ms, cap_ticks, detrimental, lands_as };
            match by_name.get(&name) {
                Some(existing) if existing.id <= id => {}
                _ => {
                    by_name.insert(name, info);
                }
            }
        }
        Ok(SpellTable { by_name, rows_parsed })
    }

    /// Read `spells_us.txt` and `spells_us_str.txt` from `dir`.
    pub fn load(dir: &Path) -> Result<SpellTable, SpellsError> {
        let read = |name: &str| {
            fs::read_to_string(dir.join(name)).map_err(|source| SpellsError::Io {
                file: dir.join(name).display().to_string(),
                source,
            })
        };
        let spells = read(SPELLS_FILE)?;
        let strings = read(STRINGS_FILE)?;
        SpellTable::parse(&spells, &strings)
    }

    pub fn get(&self, name: &str) -> Option<&SpellInfo> {
        self.by_name.get(name)
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Rows seen in `spells_us.txt`, eligible or not. 73,975 on the current client.
    pub fn rows_parsed(&self) -> usize {
        self.rows_parsed
    }
}

/// The log lives in `<install>/Logs/`; the spell files live in `<install>/`.
pub fn spells_dir_from_log(log: &Path) -> Option<PathBuf> {
    let logs_dir = log.parent()?;
    let is_logs = logs_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("logs"))
        .unwrap_or(false);
    if !is_logs {
        return None;
    }
    logs_dir.parent().map(Path::to_path_buf)
}

fn parse_int(s: &str, file: &str, line: usize, what: &str) -> Result<i64, SpellsError> {
    let v = if s.is_empty() { "0" } else { s };
    // Integer fields are integers on the current client; accept "3000.0"
    // defensively since the file is third-party-documented, not specified.
    v.parse::<i64>()
        .or_else(|_| v.parse::<f64>().map(|x| x as i64))
        .map_err(|_| SpellsError::Format {
            file: file.to_string(),
            line,
            reason: format!("{what} is not a number: {s:?}"),
        })
}

fn parse_float(s: &str, file: &str, line: usize, what: &str) -> Result<f64, SpellsError> {
    let v = if s.is_empty() { "0" } else { s };
    v.parse::<f64>().map_err(|_| SpellsError::Format {
        file: file.to_string(),
        line,
        reason: format!("{what} is not a number: {s:?}"),
    })
}
```

Add to `crates/wispd/src/main.rs`, next to the other `mod` lines:

```rust
#[allow(dead_code)] // wired into the pipeline in Task 6
mod spells;
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wispd`
Expected: PASS — 27 tests (20 + 7 new; the ignored one is not counted). Then the real-client check:

Run: `WISP_EQL_DIR="/mnt/Data4TB/Games/everquest/prefix/drive_c/users/Public/Daybreak Game Company/Installed Games/EverQuest Legends" cargo test -p wispd --release -- --ignored real_client`
Expected: PASS, 1 test. Note how long the load took (`time`): the spec's risk table wants the number; record it in your report. If it exceeds one second in release, say so — do not add a cache in this task.

- [ ] **Step 5: Commit**

```bash
git add crates/wispd
git commit -m "$(cat <<'EOF'
Add the spell table, read from the client's own data files

Five fields of spells_us.txt and one of spells_us_str.txt, located beside
the log's own Logs/ directory. The files are Daybreak's: read at runtime
from the user's install, never copied into the repository. Tests use a
handful of hand-written rows in the same format.

A spell is eligible when it has a duration and is detrimental, or carries
the lull family's landing prose -- Pacify is beneficial in the data and
cast on mobs in practice. The client has one row per spell; ranks live on
the log line, so the table is indexed by base name.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Rules — rank, log time, and the timer events

**Files:**
- Modify: `crates/wispd/src/rules.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `rules::split_rank(text: &str, known: impl Fn(&str) -> bool) -> Option<(&str, u8)>`, `rules::parse_log_time(ts: &str) -> Option<i64>` (seconds; differences only), `rules::body(line: &str) -> &str` (made `pub`), `rules::Event<'a>` and `rules::classify(body: &'a str) -> Event<'a>`. Existing `timestamp_text`, `own_kill`, `session_boundary`, `Counters` unchanged.

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` in `crates/wispd/src/rules.rs`:

```rust
    fn known(name: &str) -> bool {
        matches!(name, "Mesmerization" | "Togor's Insects" | "Venom of the Snake")
    }

    #[test]
    fn a_trailing_numeral_is_a_rank_only_when_the_base_name_is_known() {
        assert_eq!(split_rank("Mesmerization VI", known), Some(("Mesmerization", 6)));
        assert_eq!(split_rank("Togor's Insects X", known), Some(("Togor's Insects", 10)));
        assert_eq!(split_rank("Venom of the Snake", known), Some(("Venom of the Snake", 0)));
        assert_eq!(split_rank("Illusion: Human", known), None, "unknown spell");
        assert_eq!(split_rank("Mesmerization XI", known), None, "XI is not a rank we parse");
    }

    #[test]
    fn log_time_differences_are_exact() {
        let a = parse_log_time("Mon Aug 10 20:39:54 2026").unwrap();
        let b = parse_log_time("Mon Aug 10 20:40:32 2026").unwrap();
        assert_eq!(b - a, 38);
        let c = parse_log_time("Tue Aug 11 00:00:00 2026").unwrap();
        assert_eq!(c - a, 3 * 3600 + 20 * 60 + 6);
        let y = parse_log_time("Fri Jan  1 00:00:00 2027").unwrap();
        let x = parse_log_time("Thu Dec 31 23:59:59 2026").unwrap();
        assert_eq!(y - x, 1);
        assert_eq!(parse_log_time("not a time"), None);
        assert_eq!(parse_log_time("Mon Aug 10 20:39 2026"), None);
    }

    #[test]
    fn timer_events_classify_from_fixture_lines() {
        assert_eq!(classify("You begin casting Mesmerization VI."), Event::CastBegin { spell_text: "Mesmerization VI" });
        assert_eq!(classify("Your Shiftless Deeds spell fizzles!"), Event::Fizzle { spell: "Shiftless Deeds" });
        assert_eq!(classify("Your Mesmerization spell is interrupted."), Event::Interrupted { spell: "Mesmerization" });
        assert_eq!(classify("A spite golem resisted your Earthquake!"), Event::Resisted { target: "A spite golem", spell: "Earthquake" });
        assert_eq!(classify("Xicotl has taken 88 damage from your Gasping Embrace."), Event::DotTick { target: "Xicotl", spell: "Gasping Embrace" });
        assert_eq!(classify("Your Mesmerization spell has worn off of a flouting gargoyle."), Event::WornOff { spell: "Mesmerization", target: "a flouting gargoyle" });
        assert_eq!(classify("A Pickclaw guard has been awakened by Downslap."), Event::Awakened { target: "A Pickclaw guard" });
        assert_eq!(classify("You have slain a spiderling!"), Event::Slain { target: "a spiderling" });
        assert_eq!(classify("Zantetsu has been slain by Guard Wytiffin!"), Event::Slain { target: "Zantetsu" });
        assert_eq!(classify("You have entered The Northern Desert of Ro."), Event::ZoneChange);
        assert_eq!(classify("LOADING, PLEASE WAIT..."), Event::ZoneChange);
        assert_eq!(classify("a jeering gargoyle has been mesmerized."), Event::Other("a jeering gargoyle has been mesmerized."));
        assert_eq!(classify("A large rat bites YOU for 4 points of damage."), Event::Other("A large rat bites YOU for 4 points of damage."));
    }

    #[test]
    fn body_strips_the_timestamp() {
        assert_eq!(body("[Mon Aug 10 20:39:54 2026] You begin casting Pacify V."), "You begin casting Pacify V.");
        assert_eq!(body("no bracket"), "no bracket");
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wispd rules`
Expected: FAIL to compile — `split_rank`, `parse_log_time`, `classify`, `Event` do not exist; `body` is private.

- [ ] **Step 3: Implement**

Make `body` public (`pub fn body(line: &str) -> &str`) and add below `session_boundary`:

```rust
/// Rank numerals the log appends to an upgraded spell: `Mesmerization VI`.
const ROMAN: [(&str, u8); 10] = [
    ("I", 1), ("II", 2), ("III", 3), ("IV", 4), ("V", 5),
    ("VI", 6), ("VII", 7), ("VIII", 8), ("IX", 9), ("X", 10),
];

/// `Mesmerization VI` -> (`Mesmerization`, 6); `Venom of the Snake` -> (…, 0).
///
/// A trailing numeral is a rank only if what precedes it is a known spell;
/// the client data has one row per spell and no row for `Mesmerization VI`.
/// Names that legitimately end in numeral letters are the charter's trap,
/// and the table -- not this function -- decides them.
pub fn split_rank(text: &str, known: impl Fn(&str) -> bool) -> Option<(&str, u8)> {
    if known(text) {
        return Some((text, 0));
    }
    let (base, tail) = text.rsplit_once(' ')?;
    let rank = ROMAN.iter().find(|(r, _)| *r == tail).map(|(_, n)| *n)?;
    if known(base) {
        Some((base, rank))
    } else {
        None
    }
}

/// `Mon Aug 10 20:39:54 2026` -> seconds on an arbitrary naive scale.
///
/// Only differences between two values from the same log are meaningful.
/// No zone is applied and none is implied: EverQuest writes local wall
/// clock, and Wisp subtracts rather than relabels (spec §3).
pub fn parse_log_time(ts: &str) -> Option<i64> {
    let mut parts = ts.split_whitespace();
    let _weekday = parts.next()?;
    let month = match parts.next()? {
        "Jan" => 1, "Feb" => 2, "Mar" => 3, "Apr" => 4, "May" => 5, "Jun" => 6,
        "Jul" => 7, "Aug" => 8, "Sep" => 9, "Oct" => 10, "Nov" => 11, "Dec" => 12,
        _ => return None,
    };
    let day: u32 = parts.next()?.parse().ok()?;
    let mut hms = parts.next()?.split(':');
    let h: i64 = hms.next()?.parse().ok()?;
    let m: i64 = hms.next()?.parse().ok()?;
    let s: i64 = hms.next()?.parse().ok()?;
    if hms.next().is_some() {
        return None;
    }
    let year: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s)
}

/// Days since 1970-01-01 for a proleptic Gregorian date. Howard Hinnant's
/// algorithm; pure integer arithmetic, no calendar library.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// What one log line means to the timer state machine. Borrowed from the
/// line's body; `Other` carries the body for prose-landing matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event<'a> {
    CastBegin { spell_text: &'a str },
    Fizzle { spell: &'a str },
    Interrupted { spell: &'a str },
    Resisted { target: &'a str, spell: &'a str },
    DotTick { target: &'a str, spell: &'a str },
    WornOff { spell: &'a str, target: &'a str },
    Awakened { target: &'a str },
    Slain { target: &'a str },
    ZoneChange,
    Other(&'a str),
}

/// Classify a line body (timestamp already stripped). Every shape here was
/// read from the reference fixture; none is inferred from another client.
pub fn classify(body: &str) -> Event<'_> {
    if let Some(rest) = body.strip_prefix("You begin casting ") {
        if let Some(spell_text) = rest.strip_suffix('.') {
            return Event::CastBegin { spell_text };
        }
    }
    if let Some(rest) = body.strip_prefix("Your ") {
        if let Some(spell) = rest.strip_suffix(" spell fizzles!") {
            return Event::Fizzle { spell };
        }
        if let Some(spell) = rest.strip_suffix(" spell is interrupted.") {
            return Event::Interrupted { spell };
        }
        if let Some((spell, target)) = rest
            .strip_suffix('.')
            .and_then(|r| r.split_once(" spell has worn off of "))
        {
            return Event::WornOff { spell, target };
        }
    }
    if let Some((target, spell)) = body
        .strip_suffix('!')
        .and_then(|r| r.split_once(" resisted your "))
    {
        return Event::Resisted { target, spell };
    }
    if let Some((target, rest)) = body
        .strip_suffix('.')
        .and_then(|r| r.split_once(" has taken "))
    {
        if let Some((n, spell)) = rest.split_once(" damage from your ") {
            if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                return Event::DotTick { target, spell };
            }
        }
    }
    if let Some((target, _who)) = body
        .strip_suffix('.')
        .and_then(|r| r.split_once(" has been awakened by "))
    {
        return Event::Awakened { target };
    }
    if let Some(rest) = body.strip_prefix("You have slain ") {
        if let Some(target) = rest.strip_suffix('!') {
            if !target.is_empty() {
                return Event::Slain { target };
            }
        }
    }
    if let Some((target, _who)) = body
        .strip_suffix('!')
        .and_then(|r| r.split_once(" has been slain by "))
    {
        return Event::Slain { target };
    }
    if body.starts_with("You have entered ") || body.starts_with("LOADING, PLEASE WAIT") {
        return Event::ZoneChange;
    }
    Event::Other(body)
}
```

Note the order matches the reference script: cast, fizzle/interrupt, resist, tick, worn-off, awakened, own kill, slain-by, zone, other. `Fizzle`/`Interrupted`/`WornOff` are all `Your …` lines and are mutually exclusive by suffix.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wispd`
Expected: PASS — 31 tests. Clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/wispd/src/rules.rs
git commit -m "$(cat <<'EOF'
Rules: rank numerals, log-time differences, and the timer events

A trailing roman numeral is a rank only when the base name is a spell the
client knows: the data has one row per spell, so "Mesmerization VI" is
Mesmerization at rank six and a name that merely ends in numeral letters
is not split. The table decides, not the parser.

Log timestamps are parsed to seconds on a naive scale for one purpose:
subtracting two lines of the same log. No zone is applied, none implied.

The ten line shapes the state machine consumes, each read from the
fixture. The order of tests matches the reference replay's.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Durations — seed and measured store

**Files:**
- Create: `crates/wispd/src/durations.rs`
- Modify: `crates/wispd/src/main.rs` (add `#[allow(dead_code)] // wired in Task 6` + `mod durations;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `durations::seed_secs(cap_ticks: f64, rank: u8) -> u32`, `durations::SAMPLE_WINDOW: usize = 9`, `durations::MIN_SAMPLES: usize = 3`, `durations::DurationStore::{empty() -> Self, parse(json: &str) -> Result<Self, serde_json::Error>, load(path: &Path) -> Self, to_json(&self) -> String, record(&mut self, spell: &str, rank: u8, secs: u32), measured(&self, spell: &str, rank: u8) -> Option<u32>, samples(&self, spell: &str, rank: u8) -> &[u32], is_dirty(&self) -> bool, save(&mut self) -> io::Result<()>, default_path() -> PathBuf}`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/wispd/src/durations.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_is_the_cap_scaled_by_rank_capped_at_six() {
        assert_eq!(seed_secs(4.0, 0), 24, "Mesmerization, no rank");
        assert_eq!(seed_secs(4.0, 6), 38, "Mesmerization VI: 38.4 rounds down");
        assert_eq!(seed_secs(4.0, 10), 38, "rank X uses the rank-VI factor");
        assert_eq!(seed_secs(7.0, 5), 63, "Pacify V");
        assert_eq!(seed_secs(35.0, 6), 336, "Togor's Insects VI");
        assert_eq!(seed_secs(6.0, 0), 36, "Venom of the Snake");
        assert_eq!(seed_secs(20.5, 0), 123, "fractional cap");
    }

    #[test]
    fn a_measured_value_needs_three_samples_and_is_the_low_median() {
        let mut s = DurationStore::empty();
        s.record("Mesmerization", 6, 38);
        s.record("Mesmerization", 6, 40);
        assert_eq!(s.measured("Mesmerization", 6), None, "two samples are not enough");
        s.record("Mesmerization", 6, 37);
        assert_eq!(s.measured("Mesmerization", 6), Some(38));
        s.record("Mesmerization", 6, 41);
        // sorted: 37 38 40 41 -> low median is the second of the middle pair
        assert_eq!(s.measured("Mesmerization", 6), Some(38));
        assert_eq!(s.measured("Mesmerization", 5), None, "rank is part of the key");
    }

    #[test]
    fn only_the_last_nine_samples_are_kept() {
        let mut s = DurationStore::empty();
        for v in 1..=12 {
            s.record("Odium", 10, v);
        }
        assert_eq!(s.samples("Odium", 10), &[4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(s.measured("Odium", 10), Some(8));
    }

    #[test]
    fn the_store_round_trips_through_json_with_a_version() {
        let mut s = DurationStore::empty();
        s.record("Venom of the Snake", 0, 38);
        s.record("Pacify", 5, 69);
        let json = s.to_json();
        assert!(json.contains(r#""v":1"#), "{json}");
        assert!(json.contains(r#""Pacify|5":[69]"#), "{json}");
        let back = DurationStore::parse(&json).unwrap();
        assert_eq!(back.samples("Venom of the Snake", 0), &[38]);
        assert_eq!(back.samples("Pacify", 5), &[69]);
        assert!(!back.is_dirty());
    }

    #[test]
    fn a_missing_or_garbled_file_is_an_empty_store() {
        let mut p = std::env::temp_dir();
        p.push(format!("wisp-durations-missing-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let s = DurationStore::load(&p);
        assert_eq!(s.samples("x", 0), &[] as &[u32]);
        std::fs::write(&p, b"{not json").unwrap();
        let s = DurationStore::load(&p);
        assert_eq!(s.samples("x", 0), &[] as &[u32]);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn save_writes_atomically_and_clears_dirty() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("wisp-durations-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("durations.json");
        let mut s = DurationStore::load(&path);
        s.record("Odium", 10, 50);
        assert!(s.is_dirty());
        s.save().unwrap();
        assert!(!s.is_dirty());
        assert!(path.is_file(), "created parent dirs and the file");
        assert!(!path.with_extension("json.tmp").exists(), "temp file renamed away");
        let again = DurationStore::load(&path);
        assert_eq!(again.measured("Odium", 10), None);
        assert_eq!(again.samples("Odium", 10), &[50]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wispd durations`
Expected: FAIL to compile — `seed_secs`, `DurationStore` do not exist.

- [ ] **Step 3: Implement**

Add above the `mod tests` block:

```rust
// SPDX-License-Identifier: MIT
//! Where a timer's duration comes from: the client's cap as a seed, and
//! measured cast-to-fade intervals from this player's own logs as the
//! override. Measured wins; the seed fills gaps.
//!
//! The fixture shows durations changing on one character between sessions
//! (Mesmerization VI: ~38 s, later ~24 s) for reasons the log cannot name.
//! A nine-sample recency window follows that without being told.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{fs, io};

pub const SAMPLE_WINDOW: usize = 9;
pub const MIN_SAMPLES: usize = 3;
const TICK_SECS: f64 = 6.0;
/// Ranks above VI measure at the rank-VI factor in the fixture.
const MAX_SCALED_RANK: u8 = 6;
const STORE_VERSION: u32 = 1;

/// `round(cap × 6 × (10 + min(rank, 6)) / 10)` seconds.
///
/// `cap × 6 × (10 + r)` is always even for integer caps, so no value ever
/// sits on .5 and Rust's round-half-away agrees with the reference's
/// round-half-even.
pub fn seed_secs(cap_ticks: f64, rank: u8) -> u32 {
    let factor = (10 + rank.min(MAX_SCALED_RANK) as u32) as f64 / 10.0;
    (cap_ticks * TICK_SECS * factor).round() as u32
}

#[derive(Serialize, Deserialize)]
struct StoreFile {
    v: u32,
    samples: BTreeMap<String, Vec<u32>>,
}

pub struct DurationStore {
    samples: BTreeMap<String, Vec<u32>>,
    path: Option<PathBuf>,
    dirty: bool,
}

fn key(spell: &str, rank: u8) -> String {
    format!("{spell}|{rank}")
}

impl DurationStore {
    pub fn empty() -> Self {
        DurationStore { samples: BTreeMap::new(), path: None, dirty: false }
    }

    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        let file: StoreFile = serde_json::from_str(json)?;
        Ok(DurationStore { samples: file.samples, path: None, dirty: false })
    }

    /// A missing or unreadable store is an empty one; the path is kept so
    /// `save` creates it. Unreadable is reported once, on stderr.
    pub fn load(path: &Path) -> Self {
        let mut store = match fs::read_to_string(path) {
            Ok(text) => match DurationStore::parse(&text) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("wispd: ignoring unreadable duration store {}: {e}", path.display());
                    DurationStore::empty()
                }
            },
            Err(_) => DurationStore::empty(),
        };
        store.path = Some(path.to_path_buf());
        store
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&StoreFile { v: STORE_VERSION, samples: self.samples.clone() })
            .expect("a map of strings to numbers is always serialisable")
    }

    pub fn record(&mut self, spell: &str, rank: u8, secs: u32) {
        let v = self.samples.entry(key(spell, rank)).or_default();
        v.push(secs);
        if v.len() > SAMPLE_WINDOW {
            let drop = v.len() - SAMPLE_WINDOW;
            v.drain(..drop);
        }
        self.dirty = true;
    }

    /// Low median of the window once `MIN_SAMPLES` exist. For an even count
    /// the lower of the two middle values, matching the reference replay.
    pub fn measured(&self, spell: &str, rank: u8) -> Option<u32> {
        let v = self.samples.get(&key(spell, rank))?;
        if v.len() < MIN_SAMPLES {
            return None;
        }
        let mut sorted = v.clone();
        sorted.sort_unstable();
        Some(sorted[(sorted.len() - 1) / 2])
    }

    pub fn samples(&self, spell: &str, rank: u8) -> &[u32] {
        self.samples.get(&key(spell, rank)).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Write to a temp file beside the store and rename over it, so a crash
    /// mid-write leaves the old store intact. No-op without a path.
    pub fn save(&mut self) -> io::Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, self.to_json())?;
        fs::rename(&tmp, &path)?;
        self.dirty = false;
        Ok(())
    }

    /// `$XDG_DATA_HOME/wisp/durations.json`, else `~/.local/share/wisp/durations.json`.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("wisp").join("durations.json")
    }
}
```

Add to `crates/wispd/src/main.rs`: `#[allow(dead_code)] // wired into the pipeline in Task 6` above `mod durations;`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wispd`
Expected: PASS — 37 tests. Clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/wispd
git commit -m "$(cat <<'EOF'
Add the duration store: client cap as seed, measured intervals as override

The seed is the client's duration cap scaled by rank, with the factor
capped at rank VI because that is what the fixture measures for rank X
spells. It is only ever a starting point: once three cast-to-fade
intervals have been seen for a spell and rank, their low median replaces
it, and a nine-sample window keeps the value current when the game changes
something the log cannot name.

Persisted as one small JSON file under the XDG data dir, written
atomically so a crash mid-write cannot lose what was learned.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: The state machine

**Files:**
- Create: `crates/wispd/src/timers.rs`
- Modify: `crates/wispd/src/main.rs` (add `#[allow(dead_code)] // wired in Task 6` + `mod timers;`)

**Interfaces:**
- Consumes: `spells::{SpellTable, SpellInfo, LULL_PROSE, MEZ_PROSE}`, `rules::{timestamp_text, body, parse_log_time, split_rank, classify, Event}`, `durations::{seed_secs, DurationStore}`, `wisp_proto::{Timer, TimerKind, Confidence}`.
- Produces: `timers::TrackerStats` (all `u64` pub fields: `pending_armed, pending_cancelled, pending_expired, armed, armed_mez, armed_dot, armed_debuff, promoted_to_dot, ticks_heartbeat, ended_worn_off, ended_awakened, ended_slain, ended_expired, cleared_by_zone, samples, samples_discarded_short`), `timers::Tracker::{new(table: SpellTable, store: DurationStore) -> Self, observe(&mut self, line: &str), timers(&self, now_secs: f64) -> Vec<Timer>, stats(&self) -> &TrackerStats, store(&self) -> &DurationStore, store_mut(&mut self) -> &mut DurationStore, last_time(&self) -> Option<i64>, active_count(&self) -> usize, pending_count(&self) -> usize}`.

**Semantics are the reference script's (Appendix A), transition for transition.** Read it before writing code. Where the spec prose and the script differ in detail, the script is the arbiter; report any place you believe the script violates the spec rather than silently choosing.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/wispd/src/timers.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;
    use crate::spells::SpellTable;
    use wisp_proto::{Confidence, TimerKind};

    fn row(id: u32, name: &str, cast_ms: u32, cap: &str, good: u32) -> String {
        let mut f: Vec<String> = vec!["0".to_string(); 173];
        f[0] = id.to_string();
        f[1] = name.to_string();
        f[8] = cast_ms.to_string();
        f[12] = cap.to_string();
        f[28] = good.to_string();
        f.join("^")
    }

    fn table() -> SpellTable {
        let spells = [
            row(1, "Sleep", 3000, "4", 0),
            row(2, "Calm", 3000, "7", 1),
            row(3, "Sting", 2000, "6", 0),
            row(4, "Drowse", 5000, "35", 0),
            row(5, "Slug", 4000, "35", 0),
        ]
        .join("\n");
        let strings = "#h^^^^^^\n1^^^^ has been mesmerized.^^\n2^^^^ looks less aggressive.^^\n3^^^^ has been poisoned.^^\n4^^^^ yawns.^^\n5^^^^ yawns.^^\n";
        SpellTable::parse(&spells, strings).unwrap()
    }

    fn tracker() -> Tracker {
        Tracker::new(table(), crate::durations::DurationStore::empty())
    }

    // Lines at a given second offset from a fixed base time.
    fn at(secs: i64, body: &str) -> String {
        let base = crate::rules::parse_log_time("Mon Aug 10 20:00:00 2026").unwrap();
        let t = base + secs;
        let (h, m, s) = ((t / 3600) % 24, (t / 60) % 60, t % 60);
        format!("[Mon Aug 10 {h:02}:{m:02}:{s:02} 2026] {body}")
    }

    fn names(t: &Tracker, now: i64) -> Vec<(String, String, u8, i64)> {
        t.timers(now as f64)
            .into_iter()
            .map(|x| (x.target, x.spell, x.rank, x.remaining_ms))
            .collect()
    }

    #[test]
    fn a_local_cast_plus_matching_prose_arms_a_timer_with_the_seed() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sleep VI."));
        assert_eq!(t.pending_count(), 1);
        t.observe(&at(3, "a jeering gargoyle has been mesmerized."));
        assert_eq!(t.pending_count(), 0);
        let rows = t.timers(3.0 + 0.0);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!((r.target.as_str(), r.spell.as_str(), r.rank, r.kind), ("a jeering gargoyle", "Sleep", 6, TimerKind::Mez));
        assert_eq!(r.duration_ms, 38_000, "seed: 4 ticks x 6 s x 1.6");
        assert_eq!(r.confidence, Confidence::Estimated);
        assert_eq!(t.stats().armed_mez, 1);
    }

    #[test]
    fn prose_without_a_pending_cast_arms_nothing() {
        let mut t = tracker();
        t.observe(&at(0, "a jeering gargoyle has been mesmerized."));
        assert_eq!(t.active_count(), 0);
        assert_eq!(t.stats().armed, 0);
    }

    #[test]
    fn the_pending_window_is_cast_time_plus_one_tick() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sleep."));
        // cast 3000 ms -> ceil 3 + 6 = 9 s window; a landing at 9 s counts, at 10 s does not.
        t.observe(&at(10, "a rat has been mesmerized."));
        assert_eq!(t.active_count(), 0);
        assert_eq!(t.stats().pending_expired, 1);
    }

    #[test]
    fn fizzle_interrupt_and_resist_cancel_the_pending_cast() {
        for cancel in ["Your Sleep spell fizzles!", "Your Sleep spell is interrupted.", "a rat resisted your Sleep!"] {
            let mut t = tracker();
            t.observe(&at(0, "You begin casting Sleep."));
            t.observe(&at(1, cancel));
            t.observe(&at(2, "a rat has been mesmerized."));
            assert_eq!(t.active_count(), 0, "{cancel}");
            assert_eq!(t.stats().pending_cancelled, 1);
        }
    }

    #[test]
    fn shared_prose_names_the_spell_that_was_pending() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Slug."));
        t.observe(&at(4, "an ice giant yawns."));
        let rows = t.timers(4.0);
        assert_eq!(rows[0].spell, "Slug");
        assert_eq!(rows[0].kind, TimerKind::Debuff);
    }

    #[test]
    fn a_dot_tick_arms_from_a_pending_cast_and_promotes_a_prose_row() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sting."));
        t.observe(&at(2, "a rat has been poisoned."));
        assert_eq!(t.timers(2.0)[0].kind, TimerKind::Debuff);
        t.observe(&at(8, "A rat has taken 20 damage from your Sting."));
        assert_eq!(t.timers(8.0)[0].kind, TimerKind::Dot, "promoted on first tick, case-insensitive");
        assert_eq!(t.stats().promoted_to_dot, 1);
        assert_eq!(t.stats().ticks_heartbeat, 1);

        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sting."));
        t.observe(&at(2, "A rat has taken 20 damage from your Sting."));
        assert_eq!(t.timers(2.0)[0].kind, TimerKind::Dot, "armed directly by a tick");
        assert_eq!(t.stats().armed_dot, 1);
    }

    #[test]
    fn a_dot_row_is_held_open_while_ticks_keep_arriving() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sting."));
        t.observe(&at(2, "A rat has taken 20 damage from your Sting.")); // seed 36 s -> expiry 38
        t.observe(&at(40, "A rat has taken 20 damage from your Sting.")); // still ticking past expiry
        t.observe(&at(45, "Something unrelated happened."));
        assert_eq!(t.active_count(), 1, "held: now 45 <= last_tick 40 + 6");
        t.observe(&at(47, "Something unrelated happened."));
        assert_eq!(t.active_count(), 0, "47 > 46: retired");
        assert_eq!(t.stats().ended_expired, 1);
    }

    #[test]
    fn wear_off_ends_the_row_and_records_a_sample() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sleep VI."));
        t.observe(&at(3, "a rat has been mesmerized."));
        t.observe(&at(41, "Your Sleep spell has worn off of a rat."));
        assert_eq!(t.active_count(), 0);
        assert_eq!(t.stats().ended_worn_off, 1);
        assert_eq!(t.store().samples("Sleep", 6), &[38]);
        assert_eq!(t.stats().samples, 1);
    }

    #[test]
    fn a_short_wear_off_is_not_a_sample() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sleep VI."));
        t.observe(&at(3, "a rat has been mesmerized."));
        t.observe(&at(10, "Your Sleep spell has worn off of a rat.")); // 7 s < 38/2
        assert_eq!(t.store().samples("Sleep", 6), &[] as &[u32]);
        assert_eq!(t.stats().samples_discarded_short, 1);
    }

    #[test]
    fn three_samples_switch_the_seed_to_measured() {
        let mut t = tracker();
        for i in 0..3 {
            let b = i * 100;
            t.observe(&at(b, "You begin casting Sleep VI."));
            t.observe(&at(b + 3, "a rat has been mesmerized."));
            t.observe(&at(b + 27, "Your Sleep spell has worn off of a rat."));
        }
        t.observe(&at(400, "You begin casting Sleep VI."));
        t.observe(&at(403, "a rat has been mesmerized."));
        let r = &t.timers(403.0)[0];
        assert_eq!(r.duration_ms, 24_000);
        assert_eq!(r.confidence, Confidence::Measured);
    }

    #[test]
    fn awaken_retires_the_earliest_mez_for_that_name_case_insensitively() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sleep VI."));
        t.observe(&at(3, "a flouting gargoyle has been mesmerized."));
        t.observe(&at(10, "You begin casting Sleep VI."));
        t.observe(&at(13, "a flouting gargoyle has been mesmerized."));
        assert_eq!(t.active_count(), 2, "twins: one row per landing");
        t.observe(&at(20, "A flouting gargoyle has been awakened by Daggo."));
        let rows = names(&t, 20);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].3, (13 + 38 - 20) * 1000, "the later landing survives");
        assert_eq!(t.stats().ended_awakened, 1);
    }

    #[test]
    fn death_retires_one_row_of_any_spell_for_that_name() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Drowse."));
        t.observe(&at(5, "a rat yawns."));
        t.observe(&at(6, "You begin casting Sleep."));
        t.observe(&at(9, "a rat has been mesmerized."));
        t.observe(&at(12, "You have slain a rat!"));
        assert_eq!(t.active_count(), 1);
        assert_eq!(t.timers(12.0)[0].spell, "Drowse", "the mez expired sooner and was retired");
        t.observe(&at(13, "A rat has been slain by Someone!"));
        assert_eq!(t.active_count(), 0);
        assert_eq!(t.stats().ended_slain, 2);
    }

    #[test]
    fn zoning_clears_everything() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sleep."));
        t.observe(&at(3, "a rat has been mesmerized."));
        t.observe(&at(4, "You begin casting Calm."));
        t.observe(&at(5, "You have entered The Northern Desert of Ro."));
        assert_eq!((t.active_count(), t.pending_count()), (0, 0));
        assert_eq!(t.stats().cleared_by_zone, 1);
        t.observe(&at(6, "LOADING, PLEASE WAIT..."));
        assert_eq!(t.stats().cleared_by_zone, 1, "nothing left to clear");
    }

    #[test]
    fn a_row_is_held_one_tick_past_expiry_then_retired() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sleep."));
        t.observe(&at(3, "a rat has been mesmerized.")); // seed 24 -> expiry 27
        t.observe(&at(33, "Unrelated line."));
        assert_eq!(t.active_count(), 1, "33 <= 27 + 6");
        assert_eq!(t.timers(33.0)[0].remaining_ms, -6000);
        t.observe(&at(34, "Unrelated line."));
        assert_eq!(t.active_count(), 0);
    }

    #[test]
    fn a_lull_arms_by_its_prose_and_the_output_is_sorted_soonest_first() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Calm."));
        t.observe(&at(3, "Guard Drazden looks less aggressive.")); // 42 s -> expiry 45
        t.observe(&at(4, "You begin casting Sleep."));
        t.observe(&at(7, "a rat has been mesmerized.")); // 24 s -> expiry 31
        let rows = names(&t, 10);
        assert_eq!(rows[0].0, "a rat");
        assert_eq!(rows[0].3, 21_000);
        assert_eq!(rows[1].0, "Guard Drazden");
        assert_eq!(rows[1].3, 35_000);
    }

    #[test]
    fn remaining_uses_fractional_now() {
        let mut t = tracker();
        t.observe(&at(0, "You begin casting Sleep."));
        t.observe(&at(3, "a rat has been mesmerized."));
        assert_eq!(t.timers(3.25)[0].remaining_ms, 23_750);
        assert_eq!(t.last_time(), Some(crate::rules::parse_log_time("Mon Aug 10 20:00:03 2026").unwrap()));
    }

    #[test]
    fn lines_without_a_timestamp_are_ignored() {
        let mut t = tracker();
        t.observe("You begin casting Sleep.");
        assert_eq!(t.pending_count(), 0);
        assert_eq!(t.last_time(), None);
    }

    /// The acceptance replay. Skipped unless both variables are set:
    /// `WISP_EQL_DIR=<install> WISP_FIXTURE=<frozen fixture> cargo test -p wispd --release -- --ignored replay`
    #[test]
    #[ignore]
    fn fixture_replay_matches_the_reference_exactly() {
        let (Some(dir), Some(fixture)) = (std::env::var_os("WISP_EQL_DIR"), std::env::var_os("WISP_FIXTURE")) else {
            return;
        };
        let table = SpellTable::load(std::path::Path::new(&dir)).unwrap();
        assert_eq!(table.len(), 12245);
        let mut t = Tracker::new(table, crate::durations::DurationStore::empty());
        let text = std::fs::read_to_string(&fixture).unwrap();
        for line in text.lines() {
            t.observe(line.trim_end_matches('\r'));
        }
        let s = t.stats().clone();
        assert_eq!(
            s,
            TrackerStats {
                pending_armed: 8042,
                pending_cancelled: 730,
                pending_expired: 399,
                armed: 5972,
                armed_mez: 822,
                armed_dot: 214,
                armed_debuff: 4936,
                promoted_to_dot: 2097,
                ticks_heartbeat: 12814,
                ended_worn_off: 1660,
                ended_awakened: 19,
                ended_slain: 2042,
                ended_expired: 2107,
                cleared_by_zone: 144,
                samples: 1060,
                samples_discarded_short: 600,
            }
        );
        assert_eq!((t.active_count(), t.pending_count()), (0, 0));
        assert_eq!(t.store().samples("Mesmerization", 6), &[22, 27, 21, 27, 19, 28, 24, 22, 28]);
        assert_eq!(t.store().measured("Mesmerization", 6), Some(24));
        assert_eq!(t.store().measured("Pacify", 5), Some(69));
        assert_eq!(t.store().measured("Venom of the Snake", 0), Some(38));
        assert_eq!(t.store().measured("Envenomed Bolt", 10), Some(57));
        assert_eq!(t.store().measured("Odium", 10), Some(50));
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wispd timers`
Expected: FAIL to compile — `Tracker` does not exist.

- [ ] **Step 3: Implement the tracker**

Add above the `mod tests` block:

```rust
// SPDX-License-Identifier: MIT
//! The timer state machine. Pure over classified log lines; runs on log
//! time so a replay of an old log behaves exactly as the live session did.
//!
//! Only a local cast can arm a row: the log prints the same landing prose
//! for everyone's spells, and four slows share " yawns.", so the pending
//! cast is what names the spell. Names are keyed case-insensitively because
//! the log capitalises a mob's article at the start of some sentences and
//! not others.

use crate::durations::{seed_secs, DurationStore};
use crate::rules::{body, classify, parse_log_time, split_rank, timestamp_text, Event};
use crate::spells::{SpellTable, MEZ_PROSE};
use wisp_proto::{Confidence, Timer, TimerKind};

/// One server tick, the slack allowed after a cast and after an expiry.
const TICK: i64 = 6;
/// The snapshot carries at most this many rows, soonest first.
const MAX_TIMERS: usize = 16;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TrackerStats {
    pub pending_armed: u64,
    pub pending_cancelled: u64,
    pub pending_expired: u64,
    pub armed: u64,
    pub armed_mez: u64,
    pub armed_dot: u64,
    pub armed_debuff: u64,
    pub promoted_to_dot: u64,
    pub ticks_heartbeat: u64,
    pub ended_worn_off: u64,
    pub ended_awakened: u64,
    pub ended_slain: u64,
    pub ended_expired: u64,
    pub cleared_by_zone: u64,
    pub samples: u64,
    pub samples_discarded_short: u64,
}

struct Pending {
    spell: String,
    rank: u8,
    cast_at: i64,
    deadline: i64,
    /// First-insertion order; a replacement keeps its slot, as the reference does.
    seq: u64,
}

struct Active {
    key: String,
    target: String,
    spell: String,
    rank: u8,
    kind: TimerKind,
    landed_at: i64,
    duration_s: u32,
    measured: bool,
    last_tick: i64,
}

impl Active {
    fn expiry(&self) -> i64 {
        self.landed_at + self.duration_s as i64
    }
}

pub struct Tracker {
    table: SpellTable,
    store: DurationStore,
    pending: Vec<Pending>,
    active: Vec<Active>,
    stats: TrackerStats,
    last_time: Option<i64>,
    next_seq: u64,
}

fn key_of(name: &str) -> String {
    name.to_lowercase()
}

impl Tracker {
    pub fn new(table: SpellTable, store: DurationStore) -> Self {
        Tracker {
            table,
            store,
            pending: Vec::new(),
            active: Vec::new(),
            stats: TrackerStats::default(),
            last_time: None,
            next_seq: 0,
        }
    }

    pub fn stats(&self) -> &TrackerStats {
        &self.stats
    }

    pub fn store(&self) -> &DurationStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut DurationStore {
        &mut self.store
    }

    /// Log time of the most recent timestamped line, in seconds.
    pub fn last_time(&self) -> Option<i64> {
        self.last_time
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Feed one raw log line. Lines without a parseable timestamp are ignored.
    pub fn observe(&mut self, line: &str) {
        let Some(now) = timestamp_text(line).and_then(parse_log_time) else {
            return;
        };
        self.last_time = Some(now);
        self.expire(now);

        match classify(body(line)) {
            Event::CastBegin { spell_text } => self.cast_begin(spell_text, now),
            Event::Fizzle { spell } | Event::Interrupted { spell } => self.cancel_pending(spell),
            Event::Resisted { spell, .. } => self.cancel_pending(spell),
            Event::DotTick { target, spell } => self.dot_tick(target, spell, now),
            Event::WornOff { spell, target } => self.worn_off(spell, target, now),
            Event::Awakened { target } => {
                let k = key_of(target);
                if self.retire_earliest(|a| a.key == k && a.kind == TimerKind::Mez).is_some() {
                    self.stats.ended_awakened += 1;
                }
            }
            Event::Slain { target } => {
                let k = key_of(target);
                if self.retire_earliest(|a| a.key == k).is_some() {
                    self.stats.ended_slain += 1;
                }
            }
            Event::ZoneChange => {
                self.stats.cleared_by_zone += self.active.len() as u64;
                self.active.clear();
                self.pending.clear();
            }
            Event::Other(text) => self.prose_landing(text, now),
        }
    }

    /// Active rows, soonest expiry first, at most `MAX_TIMERS`.
    pub fn timers(&self, now_secs: f64) -> Vec<Timer> {
        let mut rows: Vec<Timer> = self
            .active
            .iter()
            .map(|a| Timer {
                target: a.target.clone(),
                spell: a.spell.clone(),
                rank: a.rank,
                kind: a.kind,
                remaining_ms: ((a.expiry() as f64 - now_secs) * 1000.0).round() as i64,
                duration_ms: a.duration_s as u64 * 1000,
                confidence: if a.measured { Confidence::Measured } else { Confidence::Estimated },
            })
            .collect();
        rows.sort_by_key(|t| t.remaining_ms);
        rows.truncate(MAX_TIMERS);
        rows
    }

    fn expire(&mut self, now: i64) {
        let before = self.pending.len();
        self.pending.retain(|p| now <= p.deadline);
        self.stats.pending_expired += (before - self.pending.len()) as u64;

        let before = self.active.len();
        self.active.retain(|a| {
            let held_until = if a.kind == TimerKind::Dot { a.expiry().max(a.last_tick) } else { a.expiry() };
            now <= held_until + TICK
        });
        self.stats.ended_expired += (before - self.active.len()) as u64;
    }

    fn cast_begin(&mut self, spell_text: &str, now: i64) {
        let Some((spell, rank)) = split_rank(spell_text, |n| self.table.get(n).is_some()) else {
            return;
        };
        let info = self.table.get(spell).expect("known");
        let deadline = now + (info.cast_ms as i64 + 999) / 1000 + TICK;
        self.stats.pending_armed += 1;
        if let Some(p) = self.pending.iter_mut().find(|p| p.spell == spell) {
            p.rank = rank;
            p.cast_at = now;
            p.deadline = deadline;
            return;
        }
        self.pending.push(Pending { spell: spell.to_string(), rank, cast_at: now, deadline, seq: self.next_seq });
        self.next_seq += 1;
    }

    fn cancel_pending(&mut self, spell: &str) {
        let before = self.pending.len();
        self.pending.retain(|p| p.spell != spell);
        if self.pending.len() < before {
            self.stats.pending_cancelled += 1;
        }
    }

    fn take_pending(&mut self, spell: &str, now: i64) -> Option<Pending> {
        let i = self.pending.iter().position(|p| p.spell == spell && now <= p.deadline)?;
        Some(self.pending.remove(i))
    }

    fn arm(&mut self, target: &str, spell: &str, rank: u8, kind: TimerKind, now: i64) {
        let cap = self.table.get(spell).expect("known").cap_ticks;
        let measured = self.store.measured(spell, rank);
        let duration_s = measured.unwrap_or_else(|| seed_secs(cap, rank));
        self.active.push(Active {
            key: key_of(target),
            target: target.to_string(),
            spell: spell.to_string(),
            rank,
            kind,
            landed_at: now,
            duration_s,
            measured: measured.is_some(),
            last_tick: now,
        });
        self.stats.armed += 1;
        match kind {
            TimerKind::Mez => self.stats.armed_mez += 1,
            TimerKind::Dot => self.stats.armed_dot += 1,
            TimerKind::Debuff => self.stats.armed_debuff += 1,
        }
    }

    fn dot_tick(&mut self, target: &str, spell: &str, now: i64) {
        if let Some(p) = self.take_pending(spell, now) {
            self.arm(target, spell, p.rank, TimerKind::Dot, now);
            return;
        }
        let k = key_of(target);
        for a in self.active.iter_mut().filter(|a| a.key == k && a.spell == spell) {
            a.last_tick = now;
            self.stats.ticks_heartbeat += 1;
            if a.kind != TimerKind::Dot {
                a.kind = TimerKind::Dot;
                self.stats.promoted_to_dot += 1;
            }
        }
    }

    fn worn_off(&mut self, spell: &str, target: &str, now: i64) {
        let k = key_of(target);
        let Some(row) = self.retire_earliest(|a| a.key == k && a.spell == spell) else {
            return;
        };
        self.stats.ended_worn_off += 1;
        let secs = (now - row.landed_at).max(0) as u32;
        let cap = self.table.get(spell).map(|s| s.cap_ticks).unwrap_or(0.0);
        let seed = seed_secs(cap, row.rank);
        if secs * 2 >= seed {
            self.store.record(spell, row.rank, secs);
            self.stats.samples += 1;
        } else {
            self.stats.samples_discarded_short += 1;
        }
    }

    /// A landing line is `<name><lands_as>` for some pending spell. Pending
    /// casts are tried earliest-cast first (ties by first insertion), so two
    /// spells sharing prose resolve to the one cast first.
    fn prose_landing(&mut self, text: &str, now: i64) {
        let mut order: Vec<usize> = (0..self.pending.len()).collect();
        order.sort_by_key(|&i| (self.pending[i].cast_at, self.pending[i].seq));
        for i in order {
            let p = &self.pending[i];
            let Some(lands) = self.table.get(&p.spell).and_then(|s| s.lands_as.as_deref()) else {
                continue;
            };
            if now <= p.deadline && text.len() > lands.len() && text.ends_with(lands) {
                let target = &text[..text.len() - lands.len()];
                let kind = if lands == MEZ_PROSE { TimerKind::Mez } else { TimerKind::Debuff };
                let p = self.pending.remove(i);
                self.arm(target, &p.spell, p.rank, kind, now);
                return;
            }
        }
    }

    /// Remove and return the row with the earliest expiry among those
    /// matching; ties go to the earlier-armed row.
    fn retire_earliest(&mut self, pred: impl Fn(&Active) -> bool) -> Option<Active> {
        let mut best: Option<usize> = None;
        for (i, a) in self.active.iter().enumerate() {
            if !pred(a) {
                continue;
            }
            match best {
                Some(b) if self.active[b].expiry() <= a.expiry() => {}
                _ => best = Some(i),
            }
        }
        best.map(|i| self.active.remove(i))
    }
}
```

Two details that decide the replay numbers, both from the reference: `expire` runs on **every** timestamped line before classification, and `worn_off` compares `secs * 2 >= seed` (the reference's `secs >= seed / 2` on integers). `arm` for a prose landing uses the pending cast's rank; `dot_tick` with a pending cast arms as `Dot` without touching any existing row.

Add to `crates/wispd/src/main.rs`: `#[allow(dead_code)] // wired into the pipeline in Task 6` above `mod timers;`.

- [ ] **Step 4: Run the unit tests, then the replay**

Run: `cargo test -p wispd`
Expected: PASS — 54 tests. Clippy clean.

Run:
```bash
WISP_EQL_DIR="/mnt/Data4TB/Games/everquest/prefix/drive_c/users/Public/Daybreak Game Company/Installed Games/EverQuest Legends" \
WISP_FIXTURE=/mnt/Data4TB/Games/everquest/fixtures/eqlog_Daggo_freeport.1440036.txt \
cargo test -p wispd --release -- --ignored replay
```
Expected: PASS. If a counter differs, the reference in Appendix A is the arbiter: run it (`python3 docs/plans/…` — copy the appendix to a scratch file), add a debug print of the first divergent event on both sides, and fix the Rust. Do **not** edit the expected numbers. If you believe the reference violates spec §4, stop and report the two lines.

- [ ] **Step 5: Commit**

```bash
git add crates/wispd
git commit -m "$(cat <<'EOF'
Add the timer state machine, on log time

Only a local cast can arm a row. The pending cast lives one tick past its
cast time; a landing line whose prose matches it, or a DoT tick naming it,
becomes a row with a duration from the store or the seed. Wear-off,
awaken, death and zoning retire rows; a row past its expiry is held one
tick for its wear-off line, longer if it is a DoT still ticking.

Two mobs can share a name. Each landing is its own row and an end signal
retires the earliest expiry -- the log's information limit, stated.

Running on log time rather than arrival time makes a --from-start replay
identical to the live session, and the acceptance replay reproduces the
reference implementation's counters exactly.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Wire the daemon

**Files:**
- Modify: `crates/wispd/src/main.rs` (whole file replaced below)

**Interfaces:**
- Consumes: everything from Tasks 1–5.
- Produces: `wispd --log <path> [--from-start] [--spells <dir>]` and `wispd --stub`, publishing v2 snapshots with `timers`.

- [ ] **Step 1: Replace `main.rs`**

```rust
// SPDX-License-Identifier: MIT
mod durations;
mod rules;
mod server;
mod spells;
mod tail;
mod timers;

use std::ffi::OsStr;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};
use wisp_proto::{Confidence, Snapshot, Timer, TimerKind, PROTOCOL_VERSION};

const TICK: Duration = Duration::from_millis(250);

fn main() -> std::io::Result<()> {
    // args_os, not args: a log path is arbitrary bytes on Linux and must not
    // panic on non-UTF-8. No String round-trip -- PathBuf is built directly
    // from the OsString.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let stub = args.iter().any(|a| a == OsStr::new("--stub"));
    let from_start = args.iter().any(|a| a == OsStr::new("--from-start"));
    let value_of = |flag: &str| {
        args.iter()
            .position(|a| a == OsStr::new(flag))
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let log: Option<PathBuf> = value_of("--log");
    let spells_override: Option<PathBuf> = value_of("--spells");

    if !stub && log.is_none() {
        eprintln!("usage: wispd --log <path> [--from-start] [--spells <dir>]");
        eprintln!("       wispd --stub");
        std::process::exit(2);
    }

    let path = server::socket_path();
    let mut srv = server::Server::bind(&path)?;
    eprintln!("wispd: listening on {}", path.display());

    let mut counters = rules::Counters::default();
    let mut tailer = match &log {
        Some(p) => Some(tail::Tailer::open(p, from_start)?),
        None => None,
    };

    // Timers need the client's spell data, found beside the log's Logs/ dir
    // unless --spells says otherwise. Without it the daemon still counts
    // kills; it just says so once and publishes no timers.
    let mut tracker = match (&log, spells_override) {
        (None, _) => None,
        (Some(log), override_dir) => {
            let dir = override_dir.or_else(|| spells::spells_dir_from_log(log));
            match dir {
                None => {
                    eprintln!("wispd: timers disabled: the log is not under a Logs/ directory; pass --spells <dir>");
                    None
                }
                Some(dir) => match spells::SpellTable::load(&dir) {
                    Ok(table) => {
                        let store_path = durations::DurationStore::default_path();
                        let store = durations::DurationStore::load(&store_path);
                        eprintln!(
                            "wispd: {} eligible spells from {}; durations in {}",
                            table.len(),
                            dir.display(),
                            store_path.display()
                        );
                        Some(timers::Tracker::new(table, store))
                    }
                    Err(e) => {
                        eprintln!("wispd: timers disabled: {e}");
                        None
                    }
                },
            }
        }
    };

    let mut seq = 0u64;
    let mut last_line_arrival = Instant::now();

    loop {
        if let Some(t) = tailer.as_mut() {
            // A poll error is reported and the loop continues rather than
            // exiting the daemon: spec §6 requires surviving log truncation
            // and file replacement without restarting.
            match t.poll() {
                Ok(lines) => {
                    if !lines.is_empty() {
                        last_line_arrival = Instant::now();
                    }
                    for line in lines {
                        counters.apply(&line);
                        if let Some(tr) = tracker.as_mut() {
                            tr.observe(&line);
                        }
                    }
                }
                Err(e) => eprintln!("wispd: tail read error: {e}"),
            }
        } else {
            // stub feed
            counters.lines_ingested += 17;
            if seq.is_multiple_of(5) {
                counters.session_kills += 1;
            }
            counters.last_ts = "Mon Aug 10 20:39:54 2026".to_string();
        }

        // Learned samples are persisted as they arrive, at most once per tick.
        if let Some(tr) = tracker.as_mut() {
            if tr.store().is_dirty() {
                if let Err(e) = tr.store_mut().save() {
                    eprintln!("wispd: cannot save duration store: {e}");
                }
            }
        }

        // Current log time is the last line's timestamp plus the wall-clock
        // time since it arrived. Live, that advances smoothly between lines;
        // under --from-start it is simply the last line's time.
        let timers_now: Vec<Timer> = match tracker.as_ref().and_then(|tr| tr.last_time().map(|t| (tr, t))) {
            Some((tr, last)) => tr.timers(last as f64 + last_line_arrival.elapsed().as_secs_f64()),
            None if stub => stub_timers(seq),
            None => Vec::new(),
        };

        seq += 1;
        let snapshot = Snapshot {
            v: PROTOCOL_VERSION,
            seq,
            ts: counters.last_ts.clone(),
            lines_ingested: counters.lines_ingested,
            session_kills: counters.session_kills,
            timers: timers_now,
        };
        srv.accept_pending(&snapshot);
        srv.broadcast(&snapshot);
        sleep(TICK);
    }
}

/// Two synthetic rows that count down and restart, so the HUD can be built
/// and eyeballed without a log or the client data.
fn stub_timers(seq: u64) -> Vec<Timer> {
    let phase_ms = (seq * 250 % 40_000) as i64;
    vec![
        Timer {
            target: "a jeering gargoyle".to_string(),
            spell: "Mesmerization".to_string(),
            rank: 6,
            kind: TimerKind::Mez,
            remaining_ms: 38_000 - phase_ms,
            duration_ms: 38_000,
            confidence: Confidence::Measured,
        },
        Timer {
            target: "Guard Drazden".to_string(),
            spell: "Pacify".to_string(),
            rank: 5,
            kind: TimerKind::Debuff,
            remaining_ms: 63_000 - phase_ms,
            duration_ms: 63_000,
            confidence: Confidence::Estimated,
        },
    ]
}
```

Remove the three `#[allow(dead_code)]` attributes added by Tasks 2, 4 and 5; nothing is dead now. If clippy or rustc reports any item as unused after this, that is a wiring mistake to fix, not something to allow.

- [ ] **Step 2: Build and test**

Run: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all clean.

- [ ] **Step 3: Verify the stub and the replay by socket**

Terminal one: `cargo run -p wispd -- --stub`
Terminal two: `timeout 3 socat - "$XDG_RUNTIME_DIR/wisp/wispd.sock"`
Expected: v2 lines with two timers whose `remaining_ms` fall by 250 each line. Kill the daemon.

Then the replay end to end (release; the fixture is read in one poll):

```bash
FX=/mnt/Data4TB/Games/everquest/fixtures/eqlog_Daggo_freeport.1440036.txt
XDG_DATA_HOME=$(mktemp -d) cargo run --release -p wispd -- --log "$FX" --from-start &
sleep 20
socat -T2 - "$XDG_RUNTIME_DIR/wisp/wispd.sock" | head -1
kill %1
```

`spells_dir_from_log` will **not** find the client files beside the frozen fixture, so this run must print `timers disabled: the log is not under a Logs/ directory`. Re-run with `--spells "<install dir>"`; expected: the startup line reports `12245 eligible spells`, the first snapshot shows `lines_ingested:1440036`, `session_kills:880`, and `"timers":[]` (the replay ends with no active rows). Then inspect the store the run wrote: `cat "$XDG_DATA_HOME"/wisp/durations.json` must contain `"Mesmerization|6":[22,27,21,27,19,28,24,22,28]`. Record the outputs. Remove the temp dir.

- [ ] **Step 4: Commit**

```bash
git add crates/wispd
git commit -m "$(cat <<'EOF'
wispd: publish timers

The spell files are found beside the log's own Logs/ directory, or wherever
--spells points. Without them the daemon still counts kills and says once
why it has no timers. Learned durations are saved as they arrive.

The snapshot's remaining times are computed against the last line's
timestamp advanced by the wall-clock time since it arrived, which is the
only place log time and the monotonic clock meet.

--stub now carries two synthetic rows so the HUD can be built against it.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: HUD rows

**Files:**
- Modify: `crates/wisp-hud/src/text.rs` (add `Line`, `render_lines`)
- Modify: `crates/wisp-hud/src/main.rs` (rows, thresholds, window sizing)

**Interfaces:**
- Consumes: `wisp_proto::{Snapshot, Timer, Confidence}`, `text::Renderer::render` (unchanged).
- Produces: `text::Line { text: String, rgb: [u8; 3] }`, `text::Renderer::render_lines(&self, lines: &[Line]) -> Frame`, `text::LINE_GAP_PX: u32`.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/wisp-hud/src/text.rs`:

```rust
    #[test]
    fn lines_stack_vertically_and_take_the_widest_width() {
        let r = Renderer::new(32.0);
        let long = r.render("1234");
        let short = r.render("12");
        let frame = r.render_lines(&[
            Line { text: "12".to_string(), rgb: [255, 255, 255] },
            Line { text: "1234".to_string(), rgb: [255, 255, 255] },
        ]);
        assert_eq!(frame.width, long.width, "as wide as the widest line");
        // Heights are per line: '3' has a one-pixel descender that '1' and '2' lack.
        assert_eq!(frame.height, short.height + long.height + LINE_GAP_PX);
        assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    }

    #[test]
    fn a_coloured_line_is_premultiplied_by_coverage() {
        let r = Renderer::new(32.0);
        let frame = r.render_lines(&[Line { text: "8".to_string(), rgb: [255, 0, 0] }]);
        let max_px = frame
            .rgba
            .chunks_exact(4)
            .max_by_key(|px| px[3])
            .unwrap();
        assert!(max_px[3] > 200, "a fully covered pixel exists");
        assert_eq!(max_px[0], max_px[3], "red channel equals alpha (premultiplied)");
        assert_eq!(max_px[1], 0);
        assert_eq!(max_px[2], 0);
        assert!(frame.rgba.chunks_exact(4).all(|px| px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]));
    }

    #[test]
    fn an_empty_line_list_is_an_empty_frame() {
        let frame = Renderer::new(32.0).render_lines(&[]);
        assert_eq!((frame.width, frame.height), (0, 0));
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wisp-hud text`
Expected: FAIL to compile — `Line`, `render_lines`, `LINE_GAP_PX` do not exist.

- [ ] **Step 3: Implement `render_lines`**

In `crates/wisp-hud/src/text.rs`, add after `Renderer`'s struct:

```rust
/// Vertical gap between stacked lines, in pixels.
pub const LINE_GAP_PX: u32 = 4;

/// One row of text and the colour to draw it in (straight, not premultiplied).
pub struct Line {
    pub text: String,
    pub rgb: [u8; 3],
}
```

Refactor `render` so the per-line rasterisation is reusable: rename the existing body of `render` to a private `fn render_one(&self, text: &str, rgb: [u8; 3]) -> Frame` whose pixel write becomes

```rust
                    // Premultiplied colour. X11 and Wayland both want premultiplied alpha.
                    rgba[o] = (rgb[0] as u32 * coverage as u32 / 255) as u8;
                    rgba[o + 1] = (rgb[1] as u32 * coverage as u32 / 255) as u8;
                    rgba[o + 2] = (rgb[2] as u32 * coverage as u32 / 255) as u8;
                    rgba[o + 3] = coverage;
```

and make `pub fn render(&self, text: &str) -> Frame { self.render_one(text, [255, 255, 255]) }` (all existing tests keep passing: white premultiplied is `coverage` in every channel). Then add:

```rust
    /// Stack lines top to bottom, `LINE_GAP_PX` apart, left-aligned, the
    /// canvas as wide as the widest line.
    pub fn render_lines(&self, lines: &[Line]) -> Frame {
        let frames: Vec<Frame> = lines.iter().map(|l| self.render_one(&l.text, l.rgb)).collect();
        if frames.is_empty() {
            return Frame { width: 0, height: 0, rgba: Vec::new() };
        }
        let width = frames.iter().map(|f| f.width).max().unwrap_or(0);
        let height: u32 = frames.iter().map(|f| f.height).sum::<u32>() + LINE_GAP_PX * (frames.len() as u32 - 1);
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        let mut y0 = 0u32;
        for f in &frames {
            for y in 0..f.height {
                let src = ((y * f.width) * 4) as usize;
                let dst = (((y0 + y) * width) * 4) as usize;
                rgba[dst..dst + (f.width * 4) as usize].copy_from_slice(&f.rgba[src..src + (f.width * 4) as usize]);
            }
            y0 += f.height + LINE_GAP_PX;
        }
        Frame { width, height, rgba }
    }
```

An empty `text` inside `render_one` still returns a 0×0 frame; `render_lines` copies zero rows for it and the gap keeps the layout stable.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wisp-hud`
Expected: PASS — 21 tests.

- [ ] **Step 5: Wire the rows into `main.rs`**

Replace the section of `crates/wisp-hud/src/main.rs` from `let renderer = …` to the end of the `while let` loop with:

```rust
    let renderer = text::Renderer::new(scale);

    // Size the window from the renderer: the kill line plus MAX_ROWS timer
    // rows at their widest, padded, so the HUD never clips at this scale.
    const PAD: u32 = 8;
    let widest = std::iter::once(text::Line { text: "999999 kills".to_string(), rgb: WHITE })
        .chain((0..MAX_ROWS).map(|_| text::Line { text: format_row("W".repeat(TARGET_COLS).as_str(), &"W".repeat(SPELL_COLS), 9999), rgb: WHITE }))
        .collect::<Vec<_>>();
    let probe = renderer.render_lines(&widest);
    let (w, h) = (probe.width + 2 * PAD, probe.height + 2 * PAD);

    let mut surface: Box<dyn OverlayBackend> = match kind {
        BackendKind::GamescopeX11 => {
            Box::new(backend::gamescope_x11::GamescopeX11Backend::new(w, h))
        }
        BackendKind::WlrLayerShell => Box::new(backend::layer_shell::LayerShellBackend::new(w, h)),
        BackendKind::PlainWindow => Box::new(backend::plain_window::PlainWindowBackend::new(w, h)),
    };
    surface.attach()?;

    let path = client::socket_path();
    let mut stream = client::connect(&path)?;
    eprintln!("wisp-hud: connected to {}", path.display());

    while let Some(item) = stream.next_snapshot() {
        match item {
            Ok(snap) => {
                let frame = renderer.render_lines(&hud_lines(&snap));
                if let Err(e) = surface.present(&frame) {
                    eprintln!("wisp-hud: {e}");
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("wisp-hud: {e}");
                std::process::exit(1);
            }
        }
    }
```

The probe must measure uniform line heights (see the final-review fix F1):
each rendered row is padded to `Renderer::line_height()`, not left at its
own ink height, so a probe row built from `"W"`/`9999` — neither of which
has a descender — is not shorter than a real row containing `g`/`j`/`y`.
Without that, the probe under-measures the frame and the bottom row clips.

This task's own commit message should not claim "so nothing clips at any
scale" — clipping was still possible on this task's code alone, until F1's
uniform-height fix landed — and should say instead what this task alone
provides: "sized for the kill line plus eight rows of uniform height".

and add these items at module level (below `main`):

```rust
use wisp_proto::{Confidence, Snapshot, Timer};

/// Wisp's own presentation thresholds. Not derived from anything.
const WARNING_SECS: i64 = 10;
const CRITICAL_SECS: i64 = 5;
const MAX_ROWS: usize = 8;
const TARGET_COLS: usize = 20;
const SPELL_COLS: usize = 18;

const WHITE: [u8; 3] = [255, 255, 255];
const DIM: [u8; 3] = [170, 170, 170];
const WARNING: [u8; 3] = [255, 200, 0];
const CRITICAL: [u8; 3] = [255, 70, 70];

fn roman(rank: u8) -> &'static str {
    ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"]
        .get(rank as usize)
        .copied()
        .unwrap_or("")
}

/// Truncate to `cols` characters, padding on the right so columns line up
/// in the monospace face.
fn fit(s: &str, cols: usize) -> String {
    let mut out: String = s.chars().take(cols).collect();
    while out.chars().count() < cols {
        out.push(' ');
    }
    out
}

fn format_row(target: &str, spell: &str, secs: i64) -> String {
    format!("{} {} {:>4}", fit(target, TARGET_COLS), fit(spell, SPELL_COLS), secs)
}

fn row_colour(t: &Timer) -> [u8; 3] {
    let secs = t.remaining_ms.div_euclid(1000);
    if secs <= CRITICAL_SECS {
        CRITICAL
    } else if secs <= WARNING_SECS {
        WARNING
    } else if t.confidence == Confidence::Estimated {
        DIM
    } else {
        WHITE
    }
}

/// The kill count, then at most MAX_ROWS timers as the daemon ordered them.
fn hud_lines(snap: &Snapshot) -> Vec<text::Line> {
    let mut lines = vec![text::Line { text: format!("{} kills", snap.session_kills), rgb: WHITE }];
    for t in snap.timers.iter().take(MAX_ROWS) {
        let spell = if t.rank == 0 { t.spell.clone() } else { format!("{} {}", t.spell, roman(t.rank)) };
        let secs = t.remaining_ms.div_euclid(1000).max(0);
        lines.push(text::Line { text: format_row(&t.target, &spell, secs), rgb: row_colour(t) });
    }
    lines
}
```

Add unit tests for the pure pieces at the bottom of `main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wisp_proto::TimerKind;

    fn timer(remaining_ms: i64, confidence: Confidence) -> Timer {
        Timer {
            target: "a jeering gargoyle".to_string(),
            spell: "Mesmerization".to_string(),
            rank: 6,
            kind: TimerKind::Mez,
            remaining_ms,
            duration_ms: 38_000,
            confidence,
        }
    }

    #[test]
    fn colour_follows_the_thresholds_and_confidence() {
        assert_eq!(row_colour(&timer(30_000, Confidence::Measured)), WHITE);
        assert_eq!(row_colour(&timer(30_000, Confidence::Estimated)), DIM);
        assert_eq!(row_colour(&timer(10_000, Confidence::Measured)), WARNING);
        assert_eq!(row_colour(&timer(5_999, Confidence::Measured)), CRITICAL);
        assert_eq!(row_colour(&timer(-2_000, Confidence::Measured)), CRITICAL);
    }

    #[test]
    fn rows_are_fixed_width_and_the_rank_is_roman() {
        let snap = Snapshot {
            v: 2, seq: 1, ts: String::new(), lines_ingested: 0, session_kills: 7,
            timers: vec![timer(11_800, Confidence::Measured)],
        };
        let lines = hud_lines(&snap);
        assert_eq!(lines[0].text, "7 kills");
        assert_eq!(lines[1].text, format!("{} {} {:>4}", fit("a jeering gargoyle", 20), fit("Mesmerization VI", 18), 11));
        assert_eq!(fit("a very long mob name indeed", 20).chars().count(), 20);
    }

    #[test]
    fn at_most_eight_rows_are_drawn() {
        let snap = Snapshot {
            v: 2, seq: 1, ts: String::new(), lines_ingested: 0, session_kills: 0,
            timers: (0..12).map(|i| timer(1000 * i, Confidence::Measured)).collect(),
        };
        assert_eq!(hud_lines(&snap).len(), 1 + MAX_ROWS);
    }
}
```

- [ ] **Step 6: Build, test, and look at it**

Run: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: clean; wisp-hud 24 tests.

Then with `cargo run -p wispd -- --stub` in the background: `timeout 10 cargo run -p wisp-hud -- --backend layer-shell` (this desktop is KDE). Expected on stderr: the backend line and no errors; on screen (if you can see it) three lines: `N kills`, then the gargoyle and Guard Drazden rows counting down, the mez row turning yellow at 10 and red at 5. You cannot verify colours yourself; record that the run was clean and leave the visual check to JDS300. Kill everything and remove the socket.

- [ ] **Step 7: Commit**

```bash
git add crates/wisp-hud
git commit -m "$(cat <<'EOF'
wisp-hud: draw timer rows under the kill count

One line per timer as the daemon ordered them, soonest first, at most
eight: target, spell with its rank, seconds left. Columns are fixed width
in the monospace face so rows never reflow. The seconds turn to the warning
colour at ten and the critical colour at five -- Wisp's own thresholds --
and an estimated duration draws dimmer than a measured one so the player
can see which countdowns the daemon has verified.

The window is sized at attach for the kill line plus eight rows at their
widest, so nothing clips at any scale.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Milestone 4 — live over EverQuest (JDS300)

**Files:**
- Modify: `PROVENANCE.md`, `README.md`

No new code. Only JDS300 can do this; the controller records the outcome.

- [ ] **Step 1: Run live**

With EverQuest Legends up: `cargo run --release -p wispd -- --log "<live log>"` and `wisp-hud` as in Spec 1. Cast a mez.

**Acceptance (spec §6, live):** the row appears on the landing line; it reaches zero within 1 s of the wear-off line and disappears; breaking a mez by damage removes the row on the awaken line; killing a mob removes its rows; zoning clears the list; restarting `wisp-hud` mid-fight shows the same rows within one second; the HUD still never takes focus or input.

- [ ] **Step 2: Record the result**

A dated entry in `PROVENANCE.md` stating exactly what was observed, and the README's Spec 2 status row. If something failed, say how; do not soften the claim.

- [ ] **Step 3: Commit**

```bash
git add PROVENANCE.md README.md
git commit -m "$(cat <<'EOF'
Record Spec 2's live run: timers over EverQuest

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Self-review

**Spec coverage.**

| Spec section | Task |
|---|---|
| §4 spell data (loader, eligibility, dir from log, fractional cap) | 2 |
| §4 rules (ten line shapes, rank, case-insensitive names, log time) | 3, 5 |
| §4 state machine (pending, landing, same-name, ending, re-landing, DoT promotion) | 5 |
| §4 durations (seed, measured, window, store, XDG path, atomic save) | 4 |
| §4 protocol v2 | 1 |
| §4 timing model (log time, wall-clock advance) | 5, 6 |
| §4 `wisp-hud` rows, thresholds, dim estimated, window sizing | 7 |
| §5 milestones 1–3 | 2, 5, 6–7 |
| §5 milestone 4 / §6 live | 8 |
| §6 loader and replay acceptance (exact numbers) | 2, 5, 6 |
| §3 game data never shipped | tests use hand-written rows; real files only under `#[ignore]` + env |
| §3 timestamps subtracted only | `parse_log_time` returns a naive scale; only differences are used |

**Known gaps, stated:**

1. The spec's risk row on parse time is measured in Task 2 and reported, not solved; a cache is out of scope until the number says otherwise.
2. `Event::Resisted` ignores the target (the reference cancels by spell only). Deliberate: a resist ends the cast whoever resisted it.
3. The HUD cannot verify its own colours; Task 7's visual check and Task 8's live acceptance are JDS300's.

**Type consistency:** `Timer`, `TimerKind`, `Confidence` (proto) are the only cross-crate types; `SpellTable::get` returns `Option<&SpellInfo>` in Tasks 2 and 5; `Tracker::timers(now_secs: f64)` in Tasks 5 and 6; `render_lines(&[Line])` in Task 7 both files; `TrackerStats` fields are identical in Task 5's struct and its replay assertion.

---

## Appendix A — the reference replay

Run on 2026-09-08 against the frozen fixture and the real client files; deterministic across runs. Throwaway Python; it is the arbiter of the spec's rules for Task 5 and is recorded here, not shipped.

```python
#!/usr/bin/env python3
"""Reference implementation of Spec 2 §4 over the fixture."""
import re, datetime, collections, statistics, math

INSTALL = "/mnt/Data4TB/Games/everquest/prefix/drive_c/users/Public/Daybreak Game Company/Installed Games/EverQuest Legends"
FIXTURE = "/mnt/Data4TB/Games/everquest/fixtures/eqlog_Daggo_freeport.1440036.txt"
LULL_PROSE = " looks less aggressive."
MEZ_PROSE = " has been mesmerized."
ROMAN = {"I":1,"II":2,"III":3,"IV":4,"V":5,"VI":6,"VII":7,"VIII":8,"IX":9,"X":10}

def load_table():
    strs = {}
    with open(INSTALL + "/spells_us_str.txt", encoding="utf-8", errors="replace") as f:
        for line in f:
            if line.startswith("#"): continue
            p = line.rstrip("\r\n").split("^")
            if len(p) > 4 and p[4]: strs[int(p[0])] = p[4]
    table = {}; rows = 0
    with open(INSTALL + "/spells_us.txt", encoding="utf-8", errors="replace") as f:
        for line in f:
            p = line.rstrip("\r\n").split("^")
            if len(p) < 173: continue
            rows += 1
            sid, name = int(p[0]), p[1]
            cast_ms, cap, good = int(float(p[8] or 0)), float(p[12] or 0), int(float(p[28] or 0))
            lands = strs.get(sid)
            if cap <= 0: continue
            if not (good == 0 or lands == LULL_PROSE): continue
            if name in table and table[name]["id"] < sid: continue
            table[name] = {"id": sid, "cast_ms": cast_ms, "cap": cap, "detrimental": good == 0, "lands_as": lands}
    return table, rows

def split_rank(text, table):
    if text in table: return text, 0
    base, _, tail = text.rpartition(" ")
    if tail in ROMAN and base in table: return base, ROMAN[tail]
    return None, 0

def seed_secs(cap, rank):
    return int(round(cap * 6 * (10 + min(rank, 6)) / 10))

def logtime(ts):
    return int(datetime.datetime.strptime(ts, "%a %b %d %H:%M:%S %Y").timestamp())  # differences only

class Store:
    def __init__(self): self.samples = collections.defaultdict(list)
    def record(self, spell, rank, secs):
        v = self.samples[(spell, rank)]; v.append(secs); del v[:-9]
    def measured(self, spell, rank):
        v = self.samples.get((spell, rank), [])
        return int(statistics.median_low(v)) if len(v) >= 3 else None

def main():
    table, rows = load_table()
    print("spells_us rows", rows, "eligible", len(table))
    store = Store(); pending = {}; active = []
    c = collections.Counter()
    ts_re = re.compile(r"^\[(.*?)\] (.*)$")
    cast_re = re.compile(r"^You begin casting (.+)\.$")
    tick_re = re.compile(r"^(.+) has taken (\d+) damage from your (.+)\.$")
    worn_re = re.compile(r"^Your (.+) spell has worn off of (.+)\.$")
    awak_re = re.compile(r"^(.+) has been awakened by (.+)\.$")
    slain_by_re = re.compile(r"^(.+) has been slain by (.+)!$")
    resist_re = re.compile(r"^(.+) resisted your (.+)!$")
    fizz_re = re.compile(r"^Your (.+) spell fizzles!$")
    intr_re = re.compile(r"^Your (.+) spell is interrupted\.$")

    def norm(n): return n.lower()
    def expiry(row): return row["landed"] + row["dur"]
    def retire(pred, key):
        cands = [r for r in active if pred(r)]
        if not cands: return None
        r = min(cands, key=expiry); active.remove(r); c[key] += 1; return r
    def arm(target, spell, rank, kind, now):
        m = store.measured(spell, rank); dur = m if m is not None else seed_secs(table[spell]["cap"], rank)
        active.append({"target": norm(target), "shown": target, "spell": spell, "rank": rank, "kind": kind,
                       "landed": now, "dur": dur, "measured": m is not None, "last_tick": now})
        c["armed"] += 1; c["armed_" + kind] += 1

    with open(FIXTURE, encoding="utf-8", errors="replace") as f:
        for raw in f:
            m = ts_re.match(raw.rstrip("\r\n"))
            if not m: continue
            now, body = logtime(m.group(1)), m.group(2)
            for sp in [s for s, p in pending.items() if now > p["deadline"]]: del pending[sp]; c["pending_expired"] += 1
            for r in [r for r in active if now > max(expiry(r), r["last_tick"] if r["kind"] == "dot" else 0) + 6]:
                active.remove(r); c["ended_expired"] += 1
            mc = cast_re.match(body)
            if mc:
                spell, rank = split_rank(mc.group(1), table)
                if spell: pending[spell] = {"rank": rank, "t": now, "deadline": now + math.ceil(table[spell]["cast_ms"] / 1000) + 6}; c["pending_armed"] += 1
                continue
            mm = fizz_re.match(body) or intr_re.match(body)
            if mm:
                if pending.pop(mm.group(1), None): c["pending_cancelled"] += 1
                continue
            mr = resist_re.match(body)
            if mr:
                if pending.pop(mr.group(2), None): c["pending_cancelled"] += 1
                continue
            mt = tick_re.match(body)
            if mt:
                target, spell = mt.group(1), mt.group(3)
                p = pending.get(spell)
                if p and now <= p["deadline"]:
                    del pending[spell]; arm(target, spell, p["rank"], "dot", now)
                else:
                    for r in active:
                        if r["target"] == norm(target) and r["spell"] == spell:
                            r["last_tick"] = now; c["ticks_heartbeat"] += 1
                            if r["kind"] != "dot": r["kind"] = "dot"; c["promoted_to_dot"] += 1
                continue
            mw = worn_re.match(body)
            if mw:
                spell, target = mw.group(1), mw.group(2)
                r = retire(lambda r: r["target"] == norm(target) and r["spell"] == spell, "ended_worn_off")
                if r:
                    secs = now - r["landed"]
                    seed = seed_secs(table[spell]["cap"], r["rank"])
                    if secs >= seed / 2: store.record(spell, r["rank"], secs); c["samples"] += 1
                    else: c["samples_discarded_short"] += 1
                continue
            ma = awak_re.match(body)
            if ma:
                retire(lambda r: r["target"] == norm(ma.group(1)) and r["kind"] == "mez", "ended_awakened"); continue
            if body.startswith("You have slain ") and body.endswith("!"):
                target = body[len("You have slain "):-1]
                retire(lambda r: r["target"] == norm(target), "ended_slain"); continue
            ms = slain_by_re.match(body)
            if ms:
                retire(lambda r: r["target"] == norm(ms.group(1)), "ended_slain"); continue
            if body.startswith("You have entered ") or body.startswith("LOADING, PLEASE WAIT"):
                c["cleared_by_zone"] += len(active); active.clear(); pending.clear(); continue
            for spell, p in sorted(pending.items(), key=lambda kv: kv[1]["t"]):
                lands = table[spell]["lands_as"]
                if lands and body.endswith(lands) and len(body) > len(lands) and now <= p["deadline"]:
                    target = body[:-len(lands)]
                    del pending[spell]; arm(target, spell, p["rank"], "mez" if lands == MEZ_PROSE else "debuff", now); break
    print(dict(sorted(c.items())))
    print("active at end", len(active), "pending at end", len(pending))
    for k in [("Mesmerization",6),("Pacify",5),("Venom of the Snake",0),("Envenomed Bolt",10),("Odium",10)]:
        print(k, store.samples.get(k), store.measured(*k))

main()
```

Its output on 2026-09-08 (counters only):

```
spells_us rows 73975 eligible 12245
{'armed': 5972, 'armed_debuff': 4936, 'armed_dot': 214, 'armed_mez': 822, 'cleared_by_zone': 144,
 'ended_awakened': 19, 'ended_expired': 2107, 'ended_slain': 2042, 'ended_worn_off': 1660,
 'pending_armed': 8042, 'pending_cancelled': 730, 'pending_expired': 399, 'promoted_to_dot': 2097,
 'samples': 1060, 'samples_discarded_short': 600, 'ticks_heartbeat': 12814}
active at end 0 pending at end 0
('Mesmerization', 6) [22, 27, 21, 27, 19, 28, 24, 22, 28] 24
('Pacify', 5) [37, 70, 73, 67, 72, 70, 68, 69, 51] 69
('Venom of the Snake', 0) [37, 37, 40, 39, 30, 38, 40, 40, 38] 38
('Envenomed Bolt', 10) [59, 59, 60, 56, 57, 58, 57, 57, 56] 57
('Odium', 10) [50, 52, 48, 49, 50, 50, 51, 54, 52] 50
```

A note on Python's `datetime.strptime(...).timestamp()`: it applies the machine's local zone, which is the same for both ends of every difference, so differences are exact except across a DST transition inside one timer's life (spec §3 accepts that). The Rust `parse_log_time` uses no zone at all; differences agree.

---

## Execution handoff

Plan complete. Execute with `superpowers:subagent-driven-development`: one worktree per implementer, review between tasks, the final whole-branch review on the most capable model. Tasks 2, 3 and 4 are independent of each other once Task 1 has landed and may run in parallel; Task 5 needs all three; Task 6 needs 5; Task 7 needs only Task 1 and may run alongside 2–6; Task 8 is JDS300's.
