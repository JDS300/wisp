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

    fn temp_socket(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "wisp-proto-test-{}-{}.sock",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn reads_successive_snapshots() {
        let path = temp_socket("read");
        let listener = UnixListener::bind(&path).unwrap();
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

        let mut stream = connect(&path).unwrap();
        for expected in 1..=3u64 {
            let snap = stream.next_snapshot().unwrap().unwrap();
            assert_eq!(snap.seq, expected);
            assert_eq!(snap.session_kills, expected);
        }
        writer.join().unwrap();
    }

    #[test]
    fn surfaces_a_version_mismatch_rather_than_guessing() {
        let path = temp_socket("version");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let _ = sock.write_all(
                b"{\"v\":99,\"seq\":1,\"ts\":\"t\",\"lines_ingested\":0,\"session_kills\":0}\n",
            );
        });

        let mut stream = connect(&path).unwrap();
        assert!(stream.next_snapshot().unwrap().is_err());
    }

    #[test]
    fn ends_cleanly_when_the_daemon_goes_away() {
        let path = temp_socket("eof");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let (_sock, _) = listener.accept().unwrap();
            // drop immediately -> EOF
        });

        let mut stream = connect(&path).unwrap();
        // Either an immediate None, or None after whatever arrived first.
        while let Some(item) = stream.next_snapshot() {
            let _ = item;
        }
    }
}
