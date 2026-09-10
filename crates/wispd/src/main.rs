// SPDX-License-Identifier: MIT
mod combat;
mod durations;
mod encounter;
mod rules;
mod server;
mod source;
mod spells;
mod tail;
mod timers;

use source::Action;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};
use wisp_config::config::{Config, Key};
use wisp_config::discover::scan_logs_dir;
use wisp_config::paths::{config_path, socket_path};
use wisp_config::source::{resolve_log_source, LogSource};
use wisp_config::spells::{spells_dir_from_log, spells_dir_from_logs_dir};
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
    let log_flag = value_of("--log");
    let logs_dir_flag = value_of("--logs-dir");
    let spells_override = value_of("--spells");

    let config = load_config();
    for name in config.unknown() {
        eprintln!("wispd: ignoring unknown config key: {name}");
    }

    // The precedence chain — `--log`, then `--logs-dir`, then the config's
    // `log`, then its `logs_dir` — lives in wisp-config, so `wisp doctor`
    // resolves the same answer about the same machine instead of restating it.
    // `--stub` reads neither config key: it exists to run with no log at all, so
    // it is handed an empty config rather than growing a flag of its own there.
    let no_config = Config::default();
    let source = resolve_log_source(
        log_flag,
        logs_dir_flag,
        if stub { &no_config } else { &config },
    );
    if source.is_none() && !stub {
        no_log_to_read();
    }

    let path = socket_path();
    let mut srv = server::Server::bind(&path)?;
    eprintln!("wispd: listening on {}", path.display());

    // The stub feed is the only case with no source at all. A directory source
    // with nothing in it yet publishes zero counters instead, and says so.
    let stub_feed = stub && source.is_none();

    let mut counters = rules::Counters::default();
    // `--from-start` is spent on the first file this process opens, so clearing
    // it at that open is what makes a second use impossible: a login already in
    // progress is read from where the daemon found it, not from the character's
    // whole history.
    let mut from_start = from_start;
    let mut current: Option<PathBuf> = None;
    let mut tailer = match &source {
        Some(LogSource::File(p)) => {
            let opened = tail::Tailer::open(p, from_start)?;
            from_start = false;
            current = Some(p.clone());
            Some(opened)
        }
        // A directory is scanned at the top of the first tick like every other
        // tick: the game may not have created it yet, and that is not an error.
        _ => None,
    };

    // Timers need the client's spell data. `--spells` beats the config's
    // `spells_dir` and either beats the location derived from the source -- the
    // install being the parent of a directory named `Logs`. Derived from the
    // *source*, not from the file currently open, because the table is loaded
    // once per process and has to outlive every switch: it is 73,975 rows and
    // 38,211,219 bytes, and reparsing it on a 250 ms tick is not acceptable.
    // Without it the daemon still counts kills; it just says so once and
    // publishes no timers.
    let spells_dir = spells_override
        .or_else(|| config.path_value(Key::SpellsDir))
        .or_else(|| match &source {
            Some(LogSource::Dir(dir)) => spells_dir_from_logs_dir(dir),
            Some(LogSource::File(log)) => spells_dir_from_log(log),
            None => None,
        });

    let mut tracker = match spells_dir {
        None => {
            match &source {
                Some(LogSource::File(_)) => eprintln!("wispd: timers disabled: the log is not under a Logs/ directory; pass --spells <dir>"),
                Some(LogSource::Dir(_)) => eprintln!("wispd: timers disabled: the logs directory is not named Logs; pass --spells <dir>"),
                None => {}
            }
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
    };

    // The encounter tracker needs only the player's name, read from the
    // log's filename; without one, "you" is still recognised by pronoun and
    // only a `Daggo`-style self-reference would be missed. Rebuilt on every
    // switch, since the name belongs to the file being tailed.
    let mut fights = current.as_deref().map(encounter_tracker);

    let mut seq = 0u64;
    let mut last_line_arrival = Instant::now();
    // Each of these is said once and never again. A line every 250 ms would be
    // a denial of service on the terminal the daemon was started from.
    let mut said_waiting = false;
    let mut said_scan_error = false;
    let mut reported_unopenable: Option<PathBuf> = None;

    loop {
        // Discovery: one readdir a tick, and the answer is a pure function of
        // what is open and what is newest. The game creating its log on login
        // therefore needs no restart, and another character or server becoming
        // the newest file is followed automatically.
        if let Some(LogSource::Dir(dir)) = &source {
            let action = match scan_logs_dir(dir) {
                Ok(newest) => {
                    said_scan_error = false;
                    source::next_action(current.as_deref(), newest)
                }
                // A directory that exists but cannot be read is reported once
                // and treated exactly like an empty listing: the daemon keeps
                // running and keeps publishing. Once means once per failure,
                // not once per tick -- the latch clears when a scan succeeds.
                Err(e) => {
                    if !said_scan_error {
                        eprintln!("wispd: cannot read {}: {e}", dir.display());
                        said_scan_error = true;
                    }
                    Action::Keep
                }
            };

            let opening = match action {
                Action::Keep => None,
                Action::Open(log) => Some((log, false)),
                Action::Switch(log) => Some((log, true)),
            };
            if let Some((log, switched)) = opening {
                if switched {
                    eprintln!("wispd: newest log is now {}; resetting the session", log.display());
                }
                // The old tailer goes first: a switch must not leave two logs
                // open, and the session that log was measuring is over.
                tailer = None;
                counters = rules::Counters::default();
                // Cleared here but only rebuilt once the open succeeds: the
                // tracker is named from the filename, and building it before the
                // attempt would print "could not read the player's name" once
                // per tick for as long as an unopenable file stayed the newest.
                fights = None;
                if let Some(tr) = tracker.as_mut() {
                    tr.reset();
                }
                last_line_arrival = Instant::now();
                match tail::Tailer::open(&log, from_start) {
                    Ok(opened) => {
                        tailer = Some(opened);
                        fights = Some(encounter_tracker(&log));
                        current = Some(log);
                        // Both latches are spent with the file they belonged to:
                        // the next file this process opens is not read from its
                        // beginning, and a later failure to open one is a new
                        // fact worth reporting again.
                        from_start = false;
                        reported_unopenable = None;
                    }
                    // The file was there a moment ago and is not readable now.
                    // Reported once per path, so the next tick can try again
                    // without filling the terminal.
                    Err(e) => {
                        if reported_unopenable.as_deref() != Some(log.as_path()) {
                            eprintln!("wispd: cannot open {}: {e}", log.display());
                            reported_unopenable = Some(log);
                        }
                        current = None;
                    }
                }
            }

            if tailer.is_none() && !said_waiting {
                eprintln!("wispd: waiting for a log in {}", dir.display());
                said_waiting = true;
            }
        }

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
        } else if stub_feed {
            // stub feed
            counters.lines_ingested += 17;
            if seq.is_multiple_of(5) {
                counters.session_kills += 1;
            }
            counters.last_ts = "Mon Aug 10 20:39:54 2026".to_string();
        }
        // Otherwise a directory with nothing to tail: the counters keep the
        // values they were reset to, so the snapshot says zero and an empty ts,
        // and the daemon keeps publishing until the game gives it a log.

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
            None if stub_feed => stub_timers(seq),
            None => Vec::new(),
        };

        let encounter_now: Option<Encounter> = match fights.as_ref().and_then(|fx| fx.last_time().map(|t| (fx, t))) {
            Some((fx, last)) => fx.encounter(last as f64 + last_line_arrival.elapsed().as_secs_f64()),
            None if stub_feed => Some(stub_encounter(seq)),
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

/// The config file, or an empty config when there is none or this process
/// cannot read it.
///
/// Reported once, here, rather than at each use: an unreadable config stops the
/// daemon no more than an absent one does. A file that is not valid UTF-8, or
/// one another user owns, is the user's to fix — refusing to count kills over it
/// would leave the HUD blank with nothing on screen to say why.
fn load_config() -> Config {
    // No path means nowhere to look, which is an empty config rather than an
    // error; `no_log_to_read` is where the missing variable gets named.
    let Ok(path) = config_path() else {
        return Config::default();
    };
    match Config::load(&path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("wispd: ignoring unreadable config {}: {e}", path.display());
            Config::default()
        }
    }
}

/// The fight tracker for one log, named from its filename.
fn encounter_tracker(log: &Path) -> encounter::Tracker {
    let name = encounter::player_name_from_log(log).unwrap_or_default();
    if name.is_empty() {
        eprintln!("wispd: could not read the player's name from the log filename; self-heals by name will not count as yours");
    }
    encounter::Tracker::new(&name)
}

/// Exit 2, naming the config file and the command that fixes it. Nothing has
/// been bound or opened yet, so there is nothing to undo.
fn no_log_to_read() -> ! {
    eprintln!("wispd: no log to read");
    match config_path() {
        Ok(path) => {
            eprintln!("  set one in {}:  wisp config set logs_dir <dir>", path.display());
            eprintln!("  or pass --log <path> | --logs-dir <dir>");
        }
        // Name the variable that is missing rather than a path that could not
        // be resolved, and offer no `wisp config set` that could not write one.
        Err(e) => {
            eprintln!("  cannot resolve the config path: {e}");
            eprintln!("  pass --log <path> | --logs-dir <dir>");
        }
    }
    eprintln!("usage: wispd [--log <path> | --logs-dir <dir>] [--from-start] [--spells <dir>]");
    eprintln!("       wispd --stub");
    std::process::exit(2);
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
            MeterRow { name: "Serenitee".to_string(), amount: ser, per_s: 290 },
            MeterRow { name: "Misery".to_string(), amount: mis, per_s: 75 },
        ],
        healing: vec![MeterRow { name: "Misery".to_string(), amount: 74 * d, per_s: 74 }],
    }
}
