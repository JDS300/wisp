// SPDX-License-Identifier: MIT
mod server;

use std::thread::sleep;
use std::time::Duration;
use wisp_proto::{Snapshot, PROTOCOL_VERSION};

const TICK: Duration = Duration::from_millis(200); // 5 Hz

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|a| a == "--stub") {
        eprintln!("wispd: only --stub is implemented so far (Spec 1, Task 2)");
        std::process::exit(2);
    }

    let path = server::socket_path();
    let mut srv = server::Server::bind(&path)?;
    eprintln!("wispd: listening on {}", path.display());
    eprintln!("wispd: try  socat - {}", path.display());

    let mut snapshot = Snapshot {
        v: PROTOCOL_VERSION,
        seq: 0,
        ts: "Mon Aug 10 20:39:54 2026".to_string(),
        lines_ingested: 0,
        session_kills: 0,
    };

    loop {
        snapshot.seq += 1;
        snapshot.lines_ingested += 17;
        if snapshot.seq % 5 == 0 {
            snapshot.session_kills += 1;
        }
        srv.accept_pending(&snapshot);
        srv.broadcast(&snapshot);
        sleep(TICK);
    }
}
