// SPDX-License-Identifier: MIT
//! Reads NDJSON snapshots from wispd over its Unix socket.
//!
//! Moved here from `wisp-hud/src/client.rs` so the `wisp` CLI can be a second
//! client: a binary crate cannot export these, and this reader already called
//! `decode` from the crate it now lives in. No new dependency — `UnixStream`
//! and `BufReader` are stdlib.

use crate::{decode, ProtoError, Snapshot};
use std::fmt;
use std::io;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

pub struct SnapshotStream {
    reader: BufReader<UnixStream>,
    had_error: bool,
    timed_out: bool,
    /// Bytes of a line `next_snapshot_within` has read but not yet completed
    /// with a `\n`, so a timeout mid-line does not lose them: the next call
    /// picks up where this one left off.
    pending: Vec<u8>,
}

pub fn connect(path: &Path) -> io::Result<SnapshotStream> {
    Ok(SnapshotStream {
        reader: BufReader::new(UnixStream::connect(path)?),
        had_error: false,
        timed_out: false,
        pending: Vec::new(),
    })
}

/// How the stream ended for [`SnapshotStream::next_snapshot_within`]: unlike
/// `next_snapshot`, which logs a read error itself and returns `None` for
/// every kind of ending, this reports which one so `wisp-hud` can print the
/// right line and choose its exit code.
#[derive(Debug)]
pub enum StreamEnd {
    /// A clean EOF: the daemon closed the connection.
    Closed,
    /// A line arrived but did not decode.
    Error(ProtoError),
    /// The read itself failed (not a timeout — see
    /// [`SnapshotStream::next_snapshot_within`]).
    Io(io::Error),
}

impl fmt::Display for StreamEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamEnd::Closed => write!(f, "daemon closed the connection"),
            StreamEnd::Error(e) => write!(f, "{e}"),
            StreamEnd::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StreamEnd {}

impl SnapshotStream {
    /// Bounds how long `next_snapshot` will block waiting for the next line.
    ///
    /// The socket is unbounded by default (`None`, the OS default), which is
    /// what `wisp-hud` needs: its read loop is meant to sit there for as long
    /// as the daemon is quiet, and this method exists so that default never
    /// changes under it — `wisp-hud` does not call it. Only `wisp status`
    /// does, a one-shot read that must not hang forever if a daemon accepts
    /// the connection but never writes a snapshot.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.reader.get_ref().set_read_timeout(timeout)
    }

    /// `None` means the daemon closed the connection, a read error occurred
    /// (logged to stderr before returning; see `had_error`), or a read
    /// timeout set with `set_read_timeout` elapsed (see `timed_out`).
    pub fn next_snapshot(&mut self) -> Option<Result<Snapshot, ProtoError>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(decode(&line)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                // Only reachable when the caller has set a read timeout: a
                // socket left at the default (`wisp-hud`'s only mode) never
                // produces this. Not printed here — the caller is the one
                // that knows why it set a timeout and what to tell the user
                // about it (`wisp status` has its own message).
                self.timed_out = true;
                None
            }
            Err(e) => {
                // No process name: every binary prefixes its own stderr, and
                // this crate now has more than one consumer.
                eprintln!("snapshot read error: {e}");
                self.had_error = true;
                None
            }
        }
    }

    /// True once `next_snapshot` has returned `None` because of a read
    /// error, rather than a clean EOF. Lets the caller tell "the daemon
    /// closed the connection" apart from "reading from it failed", instead
    /// of printing the former unconditionally after every `None`.
    pub fn had_error(&self) -> bool {
        self.had_error
    }

    /// True once `next_snapshot` has returned `None` because a read timeout
    /// set with `set_read_timeout` elapsed, rather than a clean EOF or
    /// another read error.
    pub fn timed_out(&self) -> bool {
        self.timed_out
    }

    /// Like [`next_snapshot`](Self::next_snapshot), but returns `Ok(None)`
    /// when `timeout` passes with no complete line, leaving the stream
    /// usable: a line read only partway through is kept in an internal
    /// buffer rather than dropped, so the next call resumes it instead of
    /// re-reading from the middle. Used by `wisp-hud`, which polls keys
    /// between snapshots and cannot afford `next_snapshot`'s unbounded block.
    ///
    /// Sets the socket's read timeout on every call (cheap, and lets the
    /// caller vary it) rather than requiring `set_read_timeout` up front;
    /// unlike `next_snapshot`, a timeout here is not `had_error` or
    /// `timed_out` — those two fields describe `next_snapshot`'s own
    /// outcomes, and this method reports its ending through its return value
    /// instead.
    pub fn next_snapshot_within(&mut self, timeout: Duration) -> Result<Option<Snapshot>, StreamEnd> {
        self.reader.get_ref().set_read_timeout(Some(timeout)).map_err(StreamEnd::Io)?;
        loop {
            let available = match self.reader.fill_buf() {
                Ok(buf) => buf,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                    return Ok(None);
                }
                Err(e) => return Err(StreamEnd::Io(e)),
            };
            if available.is_empty() {
                // A clean EOF. A partial line sitting in `pending` with no
                // trailing newline is still decoded, the same way
                // `next_snapshot`'s `read_line` returns it rather than
                // discarding it.
                return if self.pending.is_empty() {
                    Err(StreamEnd::Closed)
                } else {
                    self.decode_pending()
                };
            }
            match available.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    self.pending.extend_from_slice(&available[..=i]);
                    self.reader.consume(i + 1);
                    return self.decode_pending();
                }
                None => {
                    let n = available.len();
                    self.pending.extend_from_slice(available);
                    self.reader.consume(n);
                    // No newline yet and the buffer is drained: loop back to
                    // `fill_buf`, which blocks (bounded by `timeout`) for more.
                }
            }
        }
    }

    /// Decodes and clears `self.pending`, which holds one line (its trailing
    /// `\n`, if any, is harmless -- `decode` trims it).
    fn decode_pending(&mut self) -> Result<Option<Snapshot>, StreamEnd> {
        let line = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        match decode(&line) {
            Ok(snap) => Ok(Some(snap)),
            Err(e) => Err(StreamEnd::Error(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;

    /// A socket path of its own per test, removed when the test ends whether it
    /// passed or panicked.
    ///
    /// The helper this replaced removed the path *before* binding and nothing
    /// removed it after, so every run left one file behind per test — the same
    /// leak as the copy in `wispd/src/server.rs`, whose 668 files under `/tmp`
    /// are what made it worth fixing in both. The pre-bind removal stays: a
    /// leftover from a killed run still has to be cleared, and the guard is what
    /// stops the next one.
    struct TempSocket {
        path: std::path::PathBuf,
    }

    impl TempSocket {
        fn new(name: &str) -> TempSocket {
            let mut path = std::env::temp_dir();
            path.push(format!("wisp-proto-test-{}-{}.sock", name, std::process::id()));
            let _ = std::fs::remove_file(&path);
            TempSocket { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempSocket {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn reads_successive_snapshots() {
        let socket = TempSocket::new("read");
        let listener = UnixListener::bind(socket.path()).unwrap();
        let writer = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            for seq in 1..=3 {
                let line = format!(
                    r#"{{"v":5,"seq":{seq},"ts":"t","lines_ingested":{},"session_kills":{seq}}}"#,
                    seq * 10
                );
                sock.write_all(line.as_bytes()).unwrap();
                sock.write_all(b"\n").unwrap();
            }
        });

        let mut stream = connect(socket.path()).unwrap();
        for expected in 1..=3u64 {
            let snap = stream.next_snapshot().unwrap().unwrap();
            assert_eq!(snap.seq, expected);
            assert_eq!(snap.session_kills, expected);
        }
        writer.join().unwrap();
    }

    #[test]
    fn surfaces_a_version_mismatch_rather_than_guessing() {
        let socket = TempSocket::new("version");
        let listener = UnixListener::bind(socket.path()).unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let _ = sock.write_all(
                b"{\"v\":99,\"seq\":1,\"ts\":\"t\",\"lines_ingested\":0,\"session_kills\":0}\n",
            );
        });

        let mut stream = connect(socket.path()).unwrap();
        assert!(stream.next_snapshot().unwrap().is_err());
    }

    #[test]
    fn ends_cleanly_when_the_daemon_goes_away() {
        let socket = TempSocket::new("eof");
        let listener = UnixListener::bind(socket.path()).unwrap();
        std::thread::spawn(move || {
            let (_sock, _) = listener.accept().unwrap();
            // drop immediately -> EOF
        });

        let mut stream = connect(socket.path()).unwrap();
        // Either an immediate None, or None after whatever arrived first.
        while let Some(item) = stream.next_snapshot() {
            let _ = item;
        }
    }

    #[test]
    fn a_read_timeout_gives_up_instead_of_hanging() {
        let socket = TempSocket::new("timeout");
        let listener = UnixListener::bind(socket.path()).unwrap();
        // Accept and never write. The thread outlives the assertions below —
        // this test does not join it, and does not need to: dropping the
        // listener's accepted socket at process exit is fine.
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(5));
            drop(sock);
        });

        let mut stream = connect(socket.path()).unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_millis(200))).unwrap();

        let started = std::time::Instant::now();
        let result = stream.next_snapshot();
        let elapsed = started.elapsed();

        assert!(result.is_none());
        assert!(stream.timed_out(), "expected the timeout outcome, not EOF or a read error");
        assert!(!stream.had_error());
        assert!(
            elapsed < std::time::Duration::from_millis(800),
            "took {elapsed:?}, should give up near the 200 ms timeout"
        );
    }

    #[test]
    fn next_snapshot_within_times_out_on_a_half_line_then_completes_it() {
        let (mut writer, sock) = UnixStream::pair().unwrap();
        let mut stream = SnapshotStream {
            reader: BufReader::new(sock),
            had_error: false,
            timed_out: false,
            pending: Vec::new(),
        };

        writer.write_all(br#"{"v":5,"seq":1,"ts":"t","#).unwrap();
        let started = std::time::Instant::now();
        let result = stream.next_snapshot_within(Duration::from_millis(150)).unwrap();
        let elapsed = started.elapsed();
        assert!(result.is_none(), "a half line is not a complete snapshot yet");
        assert!(elapsed < Duration::from_millis(800), "took {elapsed:?}, should give up near the timeout");

        writer.write_all(br#""lines_ingested":0,"session_kills":0}"#).unwrap();
        writer.write_all(b"\n").unwrap();
        let snap = stream
            .next_snapshot_within(Duration::from_millis(150))
            .unwrap()
            .expect("the completed line decodes");
        assert_eq!(snap.seq, 1);

        drop(writer);
        // The stream is still usable after both calls: a clean EOF now, not
        // an error and not the half-line's bytes resurfacing.
        assert!(matches!(stream.next_snapshot_within(Duration::from_millis(150)), Err(StreamEnd::Closed)));
    }

    #[test]
    fn no_timeout_set_still_reads_a_line() {
        let socket = TempSocket::new("no-timeout");
        let listener = UnixListener::bind(socket.path()).unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            // A short delay before writing: proof that a stream with no
            // timeout set keeps waiting rather than giving up early, the
            // behaviour `wisp-hud` depends on.
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = sock.write_all(
                b"{\"v\":5,\"seq\":1,\"ts\":\"t\",\"lines_ingested\":0,\"session_kills\":0}\n",
            );
        });

        let mut stream = connect(socket.path()).unwrap();
        let snap = stream.next_snapshot().unwrap().unwrap();
        assert_eq!(snap.seq, 1);
        assert!(!stream.timed_out());
        assert!(!stream.had_error());
    }
}
