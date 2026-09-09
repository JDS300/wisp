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
