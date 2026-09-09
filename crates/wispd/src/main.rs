// SPDX-License-Identifier: MIT
#[allow(dead_code)] // wired into the pipeline in Task 4
mod combat;
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
            encounter: None,
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
