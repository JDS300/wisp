// SPDX-License-Identifier: MIT
//! Unix-socket fan-out. One writer, many readers, no back-pressure:
//! a client that cannot keep up is dropped rather than allowed to stall
//! the daemon. Snapshots are cheap and idempotent, so a dropped client
//! simply reconnects and gets the current state.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::{fs, io};
use wisp_proto::{encode, Snapshot, STOP_LINE};

/// The only thing a client may ask of the daemon. One variant, deliberately:
/// Spec 6 §3.2 opens no request vocabulary, and an enum with one arm is the
/// shape that says so while still being matched exhaustively.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Stop,
}

/// Bytes kept per client between newlines. 256 is far more than the one
/// four-byte word the protocol has, and small enough that a thousand
/// clients cost a quarter of a megabyte.
const REQUEST_BUFFER: usize = 256;

struct Client {
    stream: UnixStream,
    /// Bytes of a line not yet terminated by `\n`.
    pending: Vec<u8>,
    /// True once `pending` overflowed: every byte up to and including the
    /// next newline is discarded, so half a giant line can never be read as
    /// a request.
    overflowed: bool,
}

pub struct Server {
    listener: UnixListener,
    clients: Vec<Client>,
    path: PathBuf,
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
            path: path.to_path_buf(),
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
                    // The same switch is what makes the read in `poll_requests`
                    // non-blocking too.
                    if stream.set_nonblocking(true).is_err() {
                        // Could not prepare the stream; treat like a failed handshake.
                        continue;
                    }
                    // The snapshot write can fail even for a client that has
                    // something to say: `wisp stop` and the tray's Stop entry
                    // both connect, write `stop\n`, flush and drop at once,
                    // well before this write reaches the socket, so the peer
                    // can already be fully closed by the time it runs. A
                    // failed write does not mean the peer sent nothing --
                    // bytes it wrote before closing are still sitting in this
                    // socket's receive queue -- so the client is kept
                    // regardless, and `poll_requests` reads it exactly like
                    // any other client. One with nothing to say is reaped the
                    // normal way, by that same read finding end-of-stream or
                    // by a later `broadcast`'s write failing.
                    let _ = stream.write_all(encode(current).as_bytes());
                    let _ = stream.flush();
                    self.clients.push(Client {
                        stream,
                        pending: Vec::new(),
                        overflowed: false,
                    });
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
        self.clients.retain_mut(|client| {
            client.stream.write_all(line.as_bytes()).is_ok() && client.stream.flush().is_ok()
        });
    }

    /// One non-blocking read per client, and the first complete `stop` line
    /// found. Everything else a client writes is dropped on the floor: the
    /// client stays connected and keeps receiving snapshots.
    ///
    /// At most `REQUEST_BUFFER` bytes are kept per client between newlines.
    /// A line longer than that is abandoned -- the excess is discarded and
    /// the rest of that line with it -- and reading resumes cleanly at the
    /// next newline, so a client that floods costs one fixed buffer and no
    /// more. A client at end-of-stream, or one whose read fails, is reaped
    /// here exactly as `broadcast` reaps a client whose write fails.
    pub fn poll_requests(&mut self) -> Option<Request> {
        let mut buf = [0u8; REQUEST_BUFFER];
        let mut stop = false;

        self.clients.retain_mut(|client| {
            if stop {
                // A stop line was already found this call; the remaining
                // clients are not read this tick -- the daemon is about to
                // exit, so there is no next tick for them to wait for.
                return true;
            }

            match client.stream.read(&mut buf) {
                Ok(0) => false, // end of stream; reap the client
                Ok(n) => {
                    if client.overflowed {
                        // Discard up to and including the next newline.
                        match buf[..n].iter().position(|&b| b == b'\n') {
                            Some(nl) => {
                                client.overflowed = false;
                                client.pending.clear();
                                client.pending.extend_from_slice(&buf[nl + 1..n]);
                            }
                            None => return true, // still no newline; stay overflowed
                        }
                    } else {
                        client.pending.extend_from_slice(&buf[..n]);
                    }

                    // Drain every complete line this read produced. A line
                    // that arrives already terminated is never held in
                    // `pending` between newlines, however long it was, so
                    // only a *partial* line can trigger the overflow check
                    // below.
                    while let Some(nl) = client.pending.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = client.pending.drain(..=nl).collect();
                        let line = &line[..line.len() - 1]; // drop the '\n'
                        let line = line.strip_suffix(b"\r").unwrap_or(line);
                        if line == STOP_LINE.as_bytes() {
                            stop = true;
                            break;
                        }
                    }

                    if client.pending.len() > REQUEST_BUFFER {
                        client.pending.clear();
                        client.overflowed = true;
                    }

                    true
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => true,
                Err(_) => false, // reap a client whose read failed
            }
        });

        if stop {
            Some(Request::Stop)
        } else {
            None
        }
    }

    /// Stop listening and remove the socket file.
    ///
    /// `UnixListener` does not unlink on drop, so a daemon that simply
    /// returned would leave its socket file behind for `Server::bind` to
    /// clear next time -- which works, but leaves `wisp status` connecting to
    /// a dead path and `ls` showing a daemon that is not there. A stop is
    /// the one exit this daemon has, so it is the one exit that tidies up.
    pub fn shutdown(self) {
        let _ = fs::remove_file(&self.path);
        // `self.listener` and every `Client`'s stream drop here, which
        // closes every client socket -- `wisp stop`'s acknowledgement is
        // exactly that close.
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
    fn a_complete_stop_line_is_reported_once() {
        let socket = TempSocket::new("stop-line");
        let mut server = Server::bind(socket.path()).unwrap();
        let mut client = UnixStream::connect(socket.path()).unwrap();
        server.accept_pending(&snapshot(1, 0));

        assert_eq!(server.poll_requests(), None, "nothing written yet");
        client.write_all(b"stop\n").unwrap();
        client.flush().unwrap();
        // One read per tick; the write has landed in the socket by the time the
        // next call happens, because both ends are in this process.
        assert_eq!(server.poll_requests(), Some(Request::Stop));
        drop(client);
    }

    #[test]
    fn garbage_a_partial_line_and_an_over_long_line_leave_the_server_running() {
        let socket = TempSocket::new("stop-garbage");
        let mut server = Server::bind(socket.path()).unwrap();
        let mut noisy = UnixStream::connect(socket.path()).unwrap();
        let quiet = UnixStream::connect(socket.path()).unwrap();
        server.accept_pending(&snapshot(1, 0));
        assert_eq!(server.client_count(), 2);

        for junk in [&b"stopp\n"[..], br#"{"stop":true}"#, b"\n", b"sto", &[b'x'; 300][..]] {
            noisy.write_all(junk).unwrap();
            noisy.flush().unwrap();
            assert_eq!(server.poll_requests(), None, "junk is never a request");
        }
        assert_eq!(server.client_count(), 2, "and the noisy client is not disconnected");

        // Both clients still receive snapshots, which is the whole of "ignored".
        server.broadcast(&snapshot(2, 3));
        for stream in [noisy, quiet] {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(wisp_proto::decode(&line).unwrap().seq, 1, "the snapshot sent on accept");
        }
    }

    #[test]
    fn a_partial_stop_line_completes_on_a_later_tick() {
        let socket = TempSocket::new("stop-partial");
        let mut server = Server::bind(socket.path()).unwrap();
        let mut client = UnixStream::connect(socket.path()).unwrap();
        server.accept_pending(&snapshot(1, 0));

        client.write_all(b"st").unwrap();
        client.flush().unwrap();
        assert_eq!(server.poll_requests(), None, "half a word is not a word");
        client.write_all(b"op\n").unwrap();
        client.flush().unwrap();
        assert_eq!(server.poll_requests(), Some(Request::Stop));
        drop(client);
    }

    #[test]
    fn an_over_long_line_is_abandoned_and_does_not_swallow_the_next_one() {
        let socket = TempSocket::new("stop-overflow");
        let mut server = Server::bind(socket.path()).unwrap();
        let mut client = UnixStream::connect(socket.path()).unwrap();
        server.accept_pending(&snapshot(1, 0));

        client.write_all(&[b'x'; 400]).unwrap();
        client.flush().unwrap();
        assert_eq!(server.poll_requests(), None);
        // The newline ends the abandoned line; the word after it is read normally.
        client.write_all(b"\nstop\n").unwrap();
        client.flush().unwrap();
        assert_eq!(server.poll_requests(), Some(Request::Stop));
        drop(client);
    }

    #[test]
    fn a_client_that_went_away_is_reaped_by_the_read_as_well_as_by_the_write() {
        let socket = TempSocket::new("stop-eof");
        let mut server = Server::bind(socket.path()).unwrap();
        let client = UnixStream::connect(socket.path()).unwrap();
        server.accept_pending(&snapshot(1, 0));
        assert_eq!(server.client_count(), 1);
        drop(client);

        assert_eq!(server.poll_requests(), None);
        assert_eq!(server.client_count(), 0, "end of stream reaps the client");
    }

    #[test]
    fn shutdown_removes_the_socket_file() {
        let socket = TempSocket::new("stop-shutdown");
        let server = Server::bind(socket.path()).unwrap();
        assert!(socket.path().exists());
        server.shutdown();
        assert!(!socket.path().exists(), "a stop leaves nothing behind at {}", socket.path().display());
        // And nothing answers there any more.
        assert!(UnixStream::connect(socket.path()).is_err());
    }

    #[test]
    fn a_client_that_writes_stop_and_closes_before_the_snapshot_write_is_still_honoured() {
        let socket = TempSocket::new("stop-then-close");
        let mut server = Server::bind(socket.path()).unwrap();
        {
            let mut client = UnixStream::connect(socket.path()).unwrap();
            client.write_all(b"stop\n").unwrap();
            client.flush().unwrap();
            // Dropped here: the peer closes immediately, exactly as `wisp
            // stop` and the tray's Stop entry do -- write, flush, drop --
            // well before `accept_pending`'s own snapshot write reaches
            // this socket. That write can fail (the peer already closed
            // its read side too), but the "stop\n" bytes it sent are
            // still sitting in this socket's receive queue regardless.
        }
        server.accept_pending(&snapshot(1, 0));
        assert_eq!(server.poll_requests(), Some(Request::Stop));
    }

    #[test]
    fn a_client_that_writes_garbage_and_closes_is_reaped_and_the_server_keeps_running() {
        let socket = TempSocket::new("garbage-then-close");
        let mut server = Server::bind(socket.path()).unwrap();
        {
            let mut client = UnixStream::connect(socket.path()).unwrap();
            client.write_all(b"nonsense\n").unwrap();
            client.flush().unwrap();
        }
        server.accept_pending(&snapshot(1, 0));
        assert_eq!(server.poll_requests(), None, "garbage is never a request");
        // Reaped either by the read finding end-of-stream or by a later
        // broadcast's write failing -- either way the server itself is
        // unharmed and keeps running.
        server.broadcast(&snapshot(2, 0));
        assert_eq!(server.client_count(), 0);
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
