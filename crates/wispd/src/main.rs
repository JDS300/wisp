// SPDX-License-Identifier: MIT
mod rules;
mod server;
mod tail;

use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;
use wisp_proto::{Snapshot, PROTOCOL_VERSION};

const TICK: Duration = Duration::from_millis(250);

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let stub = args.iter().any(|a| a == "--stub");
    let from_start = args.iter().any(|a| a == "--from-start");
    let log: Option<PathBuf> = args
        .iter()
        .position(|a| a == "--log")
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
            for line in t.poll()? {
                counters.apply(&line);
            }
        } else {
            // stub feed
            counters.lines_ingested += 17;
            if seq % 5 == 0 {
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
        };
        srv.accept_pending(&snapshot);
        srv.broadcast(&snapshot);
        sleep(TICK);
    }
}
