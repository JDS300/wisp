// SPDX-License-Identifier: MIT
//! `wisp stop`: one word down the socket the daemon already has.
//!
//! The third client of the protocol, and the only one that writes. Spec 6
//! §3.2: the daemon answers one request kind and this is it. No signals, no
//! `libc`, no pid file — stopping is a request over the socket, and the
//! acknowledgement is the daemon closing the connection.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use wisp_proto::STOP_LINE;

/// How long the daemon is given to close the connection after it has been
/// asked to stop. The same five seconds `wisp run` gives it to *start*
/// listening (`READY` in `run.rs`) and `wisp status` gives it to produce a
/// snapshot: one figure for "the daemon has had long enough".
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

pub fn stop() -> i32 {
    let path = wisp_config::paths::socket_path();
    let nothing_to_stop = || {
        eprintln!("wisp: nothing to stop: no daemon is listening on {}", path.display());
        0
    };

    let Ok(mut stream) = UnixStream::connect(&path) else {
        // A socket file with nothing behind it gives ECONNREFUSED and is
        // reported the same way as no file at all. It is not removed here:
        // clearing a stale socket is the next `wispd`'s job (`Server::bind`),
        // and a `stop` that unlinked paths could unlink a live one it merely
        // failed to reach.
        return nothing_to_stop();
    };
    if stream.write_all(format!("{STOP_LINE}\n").as_bytes()).is_err() || stream.flush().is_err() {
        return nothing_to_stop();
    }

    let deadline = Instant::now() + STOP_TIMEOUT;
    let mut sink = [0u8; 4096];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            eprintln!("wisp: the daemon did not stop within {} s", STOP_TIMEOUT.as_secs());
            return 1;
        }
        // Set every time round: what is left of the deadline shrinks, and a
        // daemon still publishing snapshots would otherwise reset the clock
        // with every line it sent.
        if stream.set_read_timeout(Some(left)).is_err() {
            return 0;
        }
        match stream.read(&mut sink) {
            // The acknowledgement: the daemon closed the connection.
            Ok(0) => return 0,
            // A last snapshot on its way out. Read past it.
            Ok(_) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                eprintln!("wisp: the daemon did not stop within {} s", STOP_TIMEOUT.as_secs());
                return 1;
            }
            // The connection broke rather than closed — the daemon died
            // instead of exiting. The user asked for it to stop; it stopped.
            Err(_) => return 0,
        }
    }
}
