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
                let opened = self.touch(now, true).is_some();
                if !opened {
                    self.stats.heal_outside_fight += actual;
                    return;
                }
                let f = self.current.as_mut().expect("just touched");
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
                encounters: 2544,
                dmg_out_melee: 15199089,
                dmg_out_spell: 9082968,
                dmg_out_dot: 4433865,
                dmg_out_shield: 447208,
                own_self_dmg: 18085260,
                own_pet_dmg: 5204134,
                taken_melee: 1917807,
                taken_spell: 709376,
                taken_dot: 273054,
                heal_actual: 2356833,
                heal_over: 837342,
                own_heal_actual: 1875591,
                own_heal_over: 590911,
                own_hot_actual: 355478,
                heal_outside_fight: 266814,
                pet_announcements: 4890,
                boundaries: 971,
            }
        );
        assert_eq!(t.pet_count(), 91);
        let h = t.history();
        assert_eq!(h.len(), 2544);
        let mut durs: Vec<u64> = h.iter().map(|f| f.duration_s).collect();
        durs.sort_unstable();
        assert_eq!((durs[0], durs[durs.len() / 2], *durs.last().unwrap(), durs.iter().sum::<u64>()), (1, 35, 683, 146220));
        let big = h.iter().max_by_key(|f| f.own_damage).unwrap();
        assert_eq!((big.own_damage, big.duration_s, (big.own_damage as f64 / big.duration_s as f64).round() as u64, big.taken), (211477, 482, 439, 21020));
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
            ("you".to_string(), 23289394), ("Yder".to_string(), 1447129),
            ("Serenitee".to_string(), 1321322), ("Misery".to_string(), 989452)]);
        assert_eq!(top(&heal, 3), vec![
            ("you".to_string(), 1875591), ("Serenitee".to_string(), 196052), ("Misery".to_string(), 116859)]);
    }
}
