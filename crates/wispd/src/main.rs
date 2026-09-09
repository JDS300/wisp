// SPDX-License-Identifier: MIT
mod combat;
mod durations;
mod encounter;
mod rules;
mod server;
mod spells;
mod tail;
mod timers;

use std::ffi::OsStr;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};
use wisp_proto::{Confidence, Encounter, MeterRow, Personal, Snapshot, Timer, TimerKind, PROTOCOL_VERSION};

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
                            "wispd: {} eligible spells from {} ({} names shared by more than one row; lowest id wins); durations in {}",
                            table.len(),
                            dir.display(),
                            table.collisions(),
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
                        if let Some(fx) = fights.as_mut() {
                            fx.observe(&line);
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

        let encounter_now: Option<Encounter> = match fights.as_ref().and_then(|fx| fx.last_time().map(|t| (fx, t))) {
            Some((fx, last)) => fx.encounter(last as f64 + last_line_arrival.elapsed().as_secs_f64()),
            None if stub => Some(stub_encounter(seq)),
            None => None,
        };

        seq += 1;
        let snapshot = Snapshot {
            v: PROTOCOL_VERSION,
            seq,
            ts: counters.last_ts.clone(),
            lines_ingested: counters.lines_ingested,
            session_kills: counters.session_kills,
            timers: timers_now,
            encounter: encounter_now,
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
