// SPDX-License-Identifier: MIT
//! Unix-socket fan-out. One writer, many readers, no back-pressure:
//! a client that cannot keep up is dropped rather than allowed to stall
//! the daemon. Snapshots are cheap and idempotent, so a dropped client
//! simply reconnects and gets the current state.

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::{fs, io};
use wisp_proto::{encode, Snapshot};

pub struct Server {
    listener: UnixListener,
    clients: Vec<UnixStream>,
}

impl Server {
    /// Bind `path`, refusing it if a daemon is already listening there.
    ///
    /// The refusal is a successful `connect`: a socket file proves nothing,
    /// because a daemon that died mid-session leaves its file behind. Anything
    /// that does *not* answer — no file at all, or a stale one nobody is
    /// listening on — is removed and bound over, exactly as before, so a crash
    /// still needs no clean-up by hand.
    ///
    /// Without this, a second `wispd` unlinked a running one's socket and took
    /// its place: the first daemon kept reading the log and publishing to nobody,
    /// and nothing said that it had been orphaned. `wisp run` checks the same
    /// thing before it spawns anything and produces the better message; this is
    /// the daemon's own half of that guard, and the two must agree.
    pub fn bind(path: &Path) -> io::Result<Server> {
        if UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                path.display().to_string(),
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // A leftover file from a crashed daemon would cause EADDRINUSE.
        let _ = fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;
        Ok(Server {
            listener,
            clients: Vec::new(),
        })
    }

    // Only exercised by tests today; `wispd` is a binary crate, so an
    // otherwise-unused public method on it trips `dead_code` on a plain
    // `cargo build`. Kept public: it is the natural way to observe reaping.
    #[allow(dead_code)]
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Accept every pending connection, sending each the current snapshot at once
    /// so a fresh client is never blank while waiting for the next change.
    pub fn accept_pending(&mut self, current: &Snapshot) {
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    // `set_nonblocking` on the listener does not propagate to
                    // sockets it accepts -- each one starts out blocking and
                    // must be switched over here. Without this, a client that
                    // stops reading would make a later `write_all` in
                    // `broadcast` block forever, stalling every other client.
                    if stream.set_nonblocking(true).is_err() {
                        // Could not prepare the stream; treat like a failed handshake.
                        continue;
                    }
                    if stream.write_all(encode(current).as_bytes()).is_ok() {
                        let _ = stream.flush();
                        self.clients.push(stream);
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("wispd: accept error: {e}");
                    break;
                }
            }
        }
    }

    /// Write to every client, reaping any that have gone away.
    pub fn broadcast(&mut self, snapshot: &Snapshot) {
        let line = encode(snapshot);
        self.clients.retain_mut(|stream| {
            stream.write_all(line.as_bytes()).is_ok() && stream.flush().is_ok()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    use wisp_proto::{Snapshot, PROTOCOL_VERSION};

    fn snapshot(seq: u64, kills: u64) -> Snapshot {
        Snapshot {
            v: PROTOCOL_VERSION,
            seq,
            ts: "Mon Aug 10 20:39:54 2026".to_string(),
            log: None,
            lines_ingested: seq * 10,
            session_kills: kills,
            timers: Vec::new(),
            encounter: None,
        }
    }

    /// A socket path of its own per test, removed when the test ends whether it
    /// passed or panicked.
    ///
    /// The helper this replaced removed the path *before* binding and nothing
    /// removed it after, so every run left one file behind per test: 668
    /// `wisp-test-*.sock` files were counted under `/tmp` on the development box
    /// on 2026-09-09. The pre-bind removal stays — a leftover from a killed run
    /// still has to be cleared — and the guard is what stops the next one.
    struct TempSocket {
        path: std::path::PathBuf,
    }

    impl TempSocket {
        fn new(name: &str) -> TempSocket {
            let mut path = std::env::temp_dir();
            path.push(format!("wisp-test-{}-{}.sock", name, std::process::id()));
            let _ = fs::remove_file(&path);
            TempSocket { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempSocket {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    #[test]
    fn bind_refuses_a_path_something_is_listening_on() {
        let socket = TempSocket::new("live");
        let listener = UnixListener::bind(socket.path()).unwrap();

        // A second daemon must not take the socket from a running one, which is
        // what unlinking whatever is at the path did.
        let error = match Server::bind(socket.path()) {
            Ok(_) => panic!("bind replaced a daemon that was listening"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert!(error.to_string().contains(&socket.path().display().to_string()), "{error}");

        // Nothing was unlinked: the daemon that was there is still there and
        // still answering. That is the whole of the guarantee.
        assert!(UnixStream::connect(socket.path()).is_ok());
        assert!(listener.accept().is_ok());
        drop(listener);
    }

    #[test]
    fn new_client_receives_the_current_snapshot_immediately() {
        let socket = TempSocket::new("hello");
        let mut server = Server::bind(socket.path()).unwrap();

        let client = UnixStream::connect(socket.path()).unwrap();
        server.accept_pending(&snapshot(1, 7));

        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();

        let got = wisp_proto::decode(&line).unwrap();
        assert_eq!(got.seq, 1);
        assert_eq!(got.session_kills, 7);
    }

    #[test]
    fn broadcast_reaches_a_connected_client() {
        let socket = TempSocket::new("broadcast");
        let mut server = Server::bind(socket.path()).unwrap();
        let client = UnixStream::connect(socket.path()).unwrap();
        server.accept_pending(&snapshot(1, 0));

        let mut reader = BufReader::new(client);
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();

        server.broadcast(&snapshot(2, 3));
        let mut second = String::new();
        reader.read_line(&mut second).unwrap();

        assert_eq!(wisp_proto::decode(&second).unwrap().session_kills, 3);
    }

    #[test]
    fn a_disconnected_client_does_not_kill_the_server() {
        let socket = TempSocket::new("drop");
        let mut server = Server::bind(socket.path()).unwrap();
        let client = UnixStream::connect(socket.path()).unwrap();
        server.accept_pending(&snapshot(1, 0));
        drop(client);

        // Several broadcasts to a dead peer must not panic; the client is reaped.
        for seq in 2..6 {
            server.broadcast(&snapshot(seq, 0));
        }
        assert_eq!(server.client_count(), 0);
    }

    #[test]
    fn bind_replaces_a_stale_socket_file() {
        let socket = TempSocket::new("stale");
        std::fs::write(socket.path(), b"not a socket").unwrap();
        // Must not fail with EADDRINUSE: a file nobody is listening on is a
        // leftover, not a live daemon, and refusing it would need a restart
        // after every crash.
        let _server = Server::bind(socket.path()).unwrap();
    }

    #[test]
    fn a_client_that_never_reads_is_dropped_rather_than_stalling_the_server() {
        let socket = TempSocket::new("slow-reader");
        let mut server = Server::bind(socket.path()).unwrap();
        let client = UnixStream::connect(socket.path()).unwrap();
        server.accept_pending(&snapshot(1, 0));

        // Never read from `client`, so its kernel receive buffer (and this
        // process's send buffer for it) eventually fills. A correct server
        // must notice the resulting write failure and drop the client
        // rather than blocking forever on `write_all`. Bounded so a
        // regression to a blocking write hangs `cargo test` (and gets
        // killed by an external `timeout`) instead of spinning here
        // forever.
        let full = snapshot(2, 0);
        let mut dropped = false;
        for _ in 0..20_000 {
            server.broadcast(&full);
            if server.client_count() == 0 {
                dropped = true;
                break;
            }
        }

        assert!(
            dropped,
            "a client that never reads its socket must eventually be reaped"
        );
        assert_eq!(server.client_count(), 0);
        drop(client);
    }
}
