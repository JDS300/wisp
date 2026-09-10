// SPDX-License-Identifier: MIT
//! Reads NDJSON snapshots from wispd over its Unix socket.
//!
//! Moved here from `wisp-hud/src/client.rs` so the `wisp` CLI can be a second
//! client: a binary crate cannot export these, and this reader already called
//! `decode` from the crate it now lives in. No new dependency — `UnixStream`
//! and `BufReader` are stdlib.

use crate::{decode, ProtoError, Snapshot};
use std::io;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::Path;

pub struct SnapshotStream {
    reader: BufReader<UnixStream>,
    had_error: bool,
}

pub fn connect(path: &Path) -> io::Result<SnapshotStream> {
    Ok(SnapshotStream {
        reader: BufReader::new(UnixStream::connect(path)?),
        had_error: false,
    })
}

impl SnapshotStream {
    /// `None` means the daemon closed the connection, or a read error
    /// occurred (logged to stderr before returning; see `had_error`).
    pub fn next_snapshot(&mut self) -> Option<Result<Snapshot, ProtoError>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(decode(&line)),
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
                    r#"{{"v":3,"seq":{seq},"ts":"t","lines_ingested":{},"session_kills":{seq}}}"#,
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
}
