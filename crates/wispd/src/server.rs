// SPDX-License-Identifier: MIT
//! Unix-socket fan-out. One writer, many readers, no back-pressure:
//! a client that cannot keep up is dropped rather than allowed to stall
//! the daemon. Snapshots are cheap and idempotent, so a dropped client
//! simply reconnects and gets the current state.

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::{fs, io};
use wisp_proto::{encode, Snapshot};

/// `$XDG_RUNTIME_DIR/wisp/wispd.sock`, falling back to `/run/user/<uid>`.
pub fn socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // SAFETY: getuid() takes no arguments and cannot fail.
            let uid = unsafe { getuid() };
            PathBuf::from(format!("/run/user/{}", uid))
        });
    base.join("wisp").join("wispd.sock")
}

extern "C" {
    fn getuid() -> u32;
}

pub struct Server {
    listener: UnixListener,
    clients: Vec<UnixStream>,
}

impl Server {
    pub fn bind(path: &Path) -> io::Result<Server> {
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
                    if stream.write_all(encode(current).as_bytes()).is_ok() {
                        let _ = stream.flush();
                        self.clients.push(stream);
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
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
            lines_ingested: seq * 10,
            session_kills: kills,
        }
    }

    fn temp_socket(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("wisp-test-{}-{}.sock", name, std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn new_client_receives_the_current_snapshot_immediately() {
        let path = temp_socket("hello");
        let mut server = Server::bind(&path).unwrap();

        let client = UnixStream::connect(&path).unwrap();
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
        let path = temp_socket("broadcast");
        let mut server = Server::bind(&path).unwrap();
        let client = UnixStream::connect(&path).unwrap();
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
        let path = temp_socket("drop");
        let mut server = Server::bind(&path).unwrap();
        let client = UnixStream::connect(&path).unwrap();
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
        let path = temp_socket("stale");
        std::fs::write(&path, b"not a socket").unwrap();
        // Must not fail with EADDRINUSE.
        let _server = Server::bind(&path).unwrap();
    }
}
