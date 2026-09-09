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
    /// The first timestamp ever observed. Everything internal runs on the
    /// difference from this: the log's absolute wall clock is never itself
    /// meaningful (rules::parse_log_time), only differences between two of
    /// its lines are, so the tracker picks an arbitrary zero on first line
    /// and works entirely in seconds since it.
    epoch: Option<i64>,
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
            epoch: None,
            next_seq: 0,
        }
    }

    #[cfg(test)]
    pub fn stats(&self) -> &TrackerStats {
        &self.stats
    }

    pub fn store(&self) -> &DurationStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut DurationStore {
        &mut self.store
    }

    /// Seconds since the tracker's first observed line.
    pub fn last_time(&self) -> Option<i64> {
        self.last_time
    }

    #[cfg(test)]
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Feed one raw log line. Lines without a parseable timestamp are ignored.
    pub fn observe(&mut self, line: &str) {
        let Some(raw_now) = timestamp_text(line).and_then(parse_log_time) else {
            return;
        };
        // Seconds since the first line ever observed -- the log's own
        // wall-clock value is never meaningful on its own, only differences
        // are, so every exposed clock (`last_time`, `timers()`'s `now_secs`)
        // speaks this same zero.
        let now = raw_now - *self.epoch.get_or_insert(raw_now);
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
        assert_eq!(t.last_time(), Some(3));
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
