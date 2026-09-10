// SPDX-License-Identifier: MIT
//! `wisp status` against a socket that accepts and never writes.
//!
//! A separate file, not an addition to `tests/cli.rs`: this exercises exactly
//! one thing (the 5 s giveup added in `crates/wisp/src/status.rs`), needs none
//! of that file's `Scratch`/`Running` machinery for a real daemon, and does
//! not touch a file another worker owns. The scratch environment and binary
//! lookup below follow the same pattern as `tests/cli.rs` — never writing to
//! the developer's own `~/.config`, `~/.local/share` or `/run/user`, and never
//! at risk of seeing a real daemon — but are written fresh here rather than
//! imported, since nothing in that file is `pub`.

use std::fs;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Generous headroom over the 5 s the daemon-side timeout should take: enough
/// that a loaded CI box does not flake, not so much that a regression back to
/// "hangs forever" fails slowly instead of failing.
const WAIT: Duration = Duration::from_secs(20);

/// `wisp` itself, found the way `cargo test` promises: `CARGO_BIN_EXE_<name>`
/// is set for the package's own binaries.
fn wisp() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_wisp"))
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[test]
fn status_gives_up_after_5s_when_the_daemon_never_writes() {
    // A scratch tree of its own, so `wisp status` cannot reach a real
    // `XDG_RUNTIME_DIR/wisp/wispd.sock` and see an actual daemon.
    let root = std::env::temp_dir()
        .join(format!("wisp-status-timeout-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let runtime = root.join("runtime");
    let wisp_dir = runtime.join("wisp");
    fs::create_dir_all(&wisp_dir).unwrap();
    let socket_path: &Path = &wisp_dir.join("wispd.sock");

    // A listener that accepts the connection and then holds it open,
    // deliberately never writing anything — the "daemon accepted the
    // connection but is stuck" case this timeout exists for.
    let listener = UnixListener::bind(socket_path).unwrap();
    let accepting = std::thread::spawn(move || {
        // Held for the life of the thread so the socket stays open and
        // `wisp status` sees an accepted-but-silent connection rather than an
        // immediate EOF.
        let (sock, _) = listener.accept().unwrap();
        sleep_ms(WAIT.as_millis() as u64 + 1000);
        drop(sock);
    });

    let mut child = Command::new(wisp())
        .arg("status")
        .env("XDG_RUNTIME_DIR", &runtime)
        .env_remove("FLATPAK_ID")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wisp spawns");

    let mut stderr_pipe = child.stderr.take().expect("stderr is piped");

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("can wait on the child") {
            break status;
        }
        if started.elapsed() > WAIT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("wisp status did not exit within {WAIT:?}; it should give up after 5 s");
        }
        sleep_ms(20);
    };
    let elapsed = started.elapsed();

    let mut stderr = String::new();
    stderr_pipe.read_to_string(&mut stderr).unwrap();

    assert_eq!(status.code(), Some(1), "stderr was:\n{stderr}");
    assert!(
        stderr.contains("the daemon accepted the connection but sent no snapshot in 5 s"),
        "stderr was:\n{stderr}"
    );
    assert!(stderr.contains(&socket_path.display().to_string()), "stderr was:\n{stderr}");
    // Comfortably below WAIT, and comfortably above the 5 s timeout itself —
    // proof this exited because of the timeout, not some other path.
    assert!(
        elapsed >= Duration::from_secs(5) && elapsed < Duration::from_secs(15),
        "took {elapsed:?}, expected to give up close to 5 s"
    );

    let _ = accepting; // outlives the assertions; dropped (and its thread ended) at process exit.
    let _ = fs::remove_dir_all(&root);
}
