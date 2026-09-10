// SPDX-License-Identifier: MIT
//! The client's own spell data, read from the install and never shipped.
//!
//! Two files sit beside the game's `Logs/` directory: `spells_us.txt`
//! (173 `^`-separated fields per row) and `spells_us_str.txt` (the message
//! text, including the prose printed on a target when a spell lands). Five
//! fields of the first and one of the second are all Wisp needs. The layout
//! is documented by `amerzel/eql-info` (MIT, see THIRD_PARTY.md) and was
//! confirmed against the local files.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;
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

#[derive(Debug)]
pub struct SpellTable {
    by_name: HashMap<String, SpellInfo>,
    /// Eligible names seen on more than one row, counted once per name (not
    /// once per extra row). Names are not unique in the client data; the
    /// lowest id wins each collision.
    collisions: usize,
    #[cfg(test)]
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
        let mut collided_names: HashSet<String> = HashSet::new();
        let mut collisions = 0usize;
        #[cfg(test)]
        let mut rows_parsed = 0usize;
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
            #[cfg(test)]
            {
                rows_parsed += 1;
            }
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
                Some(existing) => {
                    // Names are not unique across eligible rows. Counted
                    // once per name, however many extra rows share it.
                    if collided_names.insert(name.clone()) {
                        collisions += 1;
                    }
                    if existing.id > id {
                        by_name.insert(name, info);
                    }
                }
                None => {
                    by_name.insert(name, info);
                }
            }
        }
        Ok(SpellTable {
            by_name,
            collisions,
            #[cfg(test)]
            rows_parsed,
        })
    }

    /// Read `spells_us.txt` and `spells_us_str.txt` from `dir`. Decoded
    /// lossily, never assumed to be valid UTF-8: it is Daybreak's file, not
    /// ours.
    pub fn load(dir: &Path) -> Result<SpellTable, SpellsError> {
        let read = |name: &str| {
            fs::read(dir.join(name))
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|source| SpellsError::Io {
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

    /// Eligible names that appeared on more than one row, counted once per
    /// name. Names are not unique in the client data; the lowest id wins.
    pub fn collisions(&self) -> usize {
        self.collisions
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Rows seen in `spells_us.txt`, eligible or not. 73,975 on the current client.
    #[cfg(test)]
    pub fn rows_parsed(&self) -> usize {
        self.rows_parsed
    }
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

    // The parser only ever reads field 4 (CASTEDOTHERTXT, the landing
    // prose); CASTEDMETXT and SPELLGONE are left empty rather than
    // populated with unused prose the fixture doesn't need.
    fn str_row(id: u32, cast_on_other: &str) -> String {
        let mut f: Vec<String> = vec![String::new(); 6];
        f[S_ID] = id.to_string();
        f[S_CAST_ON_OTHER] = cast_on_other.to_string();
        f.join("^") + "^"
    }

    fn strings_text() -> String {
        let header = "#SPELLINDEX^CASTERMETXT^CASTEROTHERTXT^CASTEDMETXT^CASTEDOTHERTXT^SPELLGONE^\n";
        let rows = [
            str_row(1, MEZ_PROSE),
            str_row(2, LULL_PROSE),
            str_row(3, " looks protected."),
            str_row(6, " shivers."),
        ]
        .join("\n");
        format!("{header}{rows}\n")
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
        assert!(!t.is_empty());
    }

    #[test]
    fn a_table_with_no_eligible_rows_is_empty() {
        let spells = [
            row(3, "Ward", 2000, "10", 1), // beneficial buff: not eligible
            row(4, "Jab", 1000, "0", 0),   // no duration: not eligible
        ]
        .join("\n")
            + "\n";
        let t = SpellTable::parse(&spells, &strings_text()).unwrap();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
    }

    #[test]
    fn a_beneficial_lull_is_eligible_by_its_prose() {
        let t = SpellTable::parse(&spells_text(), &strings_text()).unwrap();
        let calm = t.get("Calm").unwrap();
        assert!(!calm.detrimental);
        assert_eq!(calm.lands_as.as_deref(), Some(LULL_PROSE));
    }

    #[test]
    fn a_duplicate_name_among_eligible_rows_is_one_collision() {
        // spells_text() has two "Sleep" rows (ids 1 and 5): one collision,
        // counted once regardless of how many extra rows share the name.
        let t = SpellTable::parse(&spells_text(), &strings_text()).unwrap();
        assert_eq!(t.collisions(), 1);
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

    /// Against the real client. Skipped unless WISP_EQL_DIR is set; run with
    /// `WISP_EQL_DIR=<install> cargo test -p wispd -- --ignored`.
    #[test]
    #[ignore]
    fn real_client_files_load_as_the_spec_records() {
        let Some(dir) = std::env::var_os("WISP_EQL_DIR") else { return };
        let t = SpellTable::load(Path::new(&dir)).expect("real client files load");
        assert_eq!(t.rows_parsed(), 73975);
        assert_eq!(t.len(), 12245);
        assert_eq!(t.collisions(), 706, "names shared by more than one eligible row");
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
