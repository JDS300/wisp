// SPDX-License-Identifier: MIT
#[allow(dead_code)] // wired into the pipeline in Task 6
mod durations;
mod rules;
mod server;
#[allow(dead_code)] // wired into the pipeline in Task 6
mod spells;
mod tail;

use std::ffi::OsStr;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;
use wisp_proto::{Snapshot, PROTOCOL_VERSION};

const TICK: Duration = Duration::from_millis(250);

fn main() -> std::io::Result<()> {
    // args_os, not args: a log path is arbitrary bytes on Linux and must not
    // panic on non-UTF-8. No String round-trip -- PathBuf is built directly
    // from the OsString.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let stub = args.iter().any(|a| a == OsStr::new("--stub"));
    let from_start = args.iter().any(|a| a == OsStr::new("--from-start"));
    let log: Option<PathBuf> = args
        .iter()
        .position(|a| a == OsStr::new("--log"))
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);

    if !stub && log.is_none() {
        eprintln!("usage: wispd --log <path> [--from-start]");
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
    let mut seq = 0u64;

    loop {
        if let Some(t) = tailer.as_mut() {
            // A poll error is reported and the loop continues rather than
            // exiting the daemon: spec §6 requires surviving log truncation
            // and file replacement without restarting, and an unhandled `?`
            // here would lose every counter on a transient I/O hiccup.
            match t.poll() {
                Ok(lines) => {
                    for line in lines {
                        counters.apply(&line);
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

        seq += 1;
        let snapshot = Snapshot {
            v: PROTOCOL_VERSION,
            seq,
            ts: counters.last_ts.clone(),
            lines_ingested: counters.lines_ingested,
            session_kills: counters.session_kills,
            timers: Vec::new(),
        };
        srv.accept_pending(&snapshot);
        srv.broadcast(&snapshot);
        sleep(TICK);
    }
}
