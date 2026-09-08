// SPDX-License-Identifier: MIT
//! Reads NDJSON snapshots from wispd.

use std::io;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use wisp_proto::{decode, ProtoError, Snapshot};

extern "C" {
    fn getuid() -> u32;
}

pub fn socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // SAFETY: getuid takes no arguments and cannot fail.
            PathBuf::from(format!("/run/user/{}", unsafe { getuid() }))
        });
    base.join("wisp").join("wispd.sock")
}

pub struct SnapshotStream {
    reader: BufReader<UnixStream>,
}

pub fn connect(path: &Path) -> io::Result<SnapshotStream> {
    Ok(SnapshotStream {
        reader: BufReader::new(UnixStream::connect(path)?),
    })
}

impl SnapshotStream {
    /// `None` means the daemon closed the connection.
    pub fn next_snapshot(&mut self) -> Option<Result<Snapshot, ProtoError>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(decode(&line)),
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;

    fn temp_socket(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("wisp-hud-test-{}-{}.sock", name, std::process::id()));
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
                    r#"{{"v":1,"seq":{seq},"ts":"t","lines_ingested":{},"session_kills":{seq}}}"#,
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
