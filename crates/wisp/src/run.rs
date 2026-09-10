// SPDX-License-Identifier: MIT
//! Starting the daemon, the HUD and, under `--`, the game.
//!
//! The launcher, and the only part of Wisp that starts anything. Both children
//! are looked for beside this executable first and on `PATH` second, which is
//! what lets an AppImage work with a two-line `AppRun`: the three binaries are
//! unpacked into one directory that is nobody's `PATH`.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};
use wisp_config::paths::socket_path;

use crate::args::RunArgs;

/// How long the daemon gets to start listening before it is given up on.
const READY: Duration = Duration::from_secs(5);
/// How often the loops below look at their children.
const POLL: Duration = Duration::from_millis(100);

/// Start Wisp, and return the status `main` exits with.
pub fn run(args: RunArgs) -> i32 {
    let socket = socket_path();
    // Before starting anything. A daemon already listening here is not ours to
    // replace, and `wispd` would refuse it anyway — this is the same check with
    // the better message, and it costs one connect with nothing spawned and
    // nothing to clean up.
    if is_listening(&socket) {
        eprintln!("wisp: a daemon is already listening on {}", socket.display());
        return 1;
    }

    let mut daemon = match start("wispd", &args.wispd) {
        Ok(child) => child,
        Err(code) => return code,
    };
    // Readiness is a successful connect, never the socket file's appearance: a
    // daemon that died on the way leaves its file behind, so a file test can
    // pass with nothing listening, and the HUD would then be started against a
    // socket that answers nobody. The child is watched in the same loop, so a
    // daemon that dies at once is reported at once.
    match wait_for_daemon(&mut daemon, &socket, READY) {
        Readiness::Listening => {}
        // Nothing to stop: the child is gone and `try_wait` has reaped it, and
        // whatever it said about itself is already on the inherited stderr.
        Readiness::Exited(status) => {
            eprintln!("wisp: wispd exited before listening: {status}");
            return 1;
        }
        Readiness::TimedOut => {
            stop(&mut daemon);
            eprintln!(
                "wisp: wispd did not listen on {} within {}s",
                socket.display(),
                READY.as_secs()
            );
            return 1;
        }
    }
    let mut hud = match start("wisp-hud", &args.hud) {
        Ok(child) => child,
        Err(code) => {
            stop(&mut daemon);
            return code;
        }
    };

    match args.command {
        Some(command) => follow_command(&mut daemon, &mut hud, &command),
        None => follow_wisp(&mut daemon, &mut hud),
    }
}

/// Is something at `path` accepting connections.
pub(crate) fn is_listening(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

/// One of the two Wisp children, found and started.
///
/// Stdout and stderr are inherited: what the daemon and the HUD say about
/// themselves belongs on the terminal the user launched from, and a startup
/// message this launcher swallowed would be a message nobody read.
fn start(name: &str, flags: &[OsString]) -> Result<Child, i32> {
    let Some(program) = find_child(name) else {
        eprintln!("wisp: cannot find {name} beside {} or on PATH", beside());
        return Err(1);
    };
    match Command::new(&program).args(flags).spawn() {
        Ok(child) => Ok(child),
        Err(e) => {
            eprintln!("wisp: cannot start {}: {e}", program.display());
            Err(1)
        }
    }
}

/// The directory this executable is in, for a message that has to say where it
/// looked.
fn beside() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .map(|dir| dir.display().to_string())
        .unwrap_or_else(|| "itself".to_string())
}

/// `name` beside this executable, and failing that on `PATH`.
pub fn find_child(name: &str) -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    find_child_in(name, beside.as_deref(), &std::env::var_os("PATH").unwrap_or_default())
}

/// [`find_child`] against a caller's own two places to look.
///
/// Split out so both rules are testable without touching the process
/// environment, which every other test in the binary is using: `PATH` cannot be
/// set for one test and unset for the next inside one process.
fn find_child_in(name: &str, beside: Option<&Path>, path: &OsStr) -> Option<PathBuf> {
    if let Some(dir) = beside {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    path.as_bytes()
        .split(|byte| *byte == b':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(OsStr::from_bytes(dir)).join(name))
        .find(|candidate| is_executable(candidate))
}

/// A candidate that can actually be started: a file, with one of the three
/// execute bits set.
///
/// Anything else is passed over rather than spawned into an `EACCES`, so a
/// stray file beside the binaries or a half-copied install falls through to the
/// next place to look. `PermissionsExt` for the mode and no `libc`: the mode is
/// the only thing that decides it.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// What [`wait_for_daemon`] found: the socket answering, the timeout running
/// out, or the child stopping first. The three need three different reports, so
/// they are three answers rather than a `bool` and a guess.
#[derive(Debug)]
pub enum Readiness {
    Listening,
    TimedOut,
    Exited(ExitStatus),
}

/// True once something at `path` accepts a connection.
///
/// A successful connect, never a file test — see [`run`]. `run` itself calls
/// [`wait_for_daemon`], which watches the child as well; this is the
/// socket-only form, and the two share one loop so neither can drift on the
/// interval or the deadline.
// Only exercised by tests today; `wisp` is a binary crate, so an otherwise
// unused public function on it trips `dead_code` on a plain `cargo build`.
#[allow(dead_code)]
pub fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    matches!(wait_until(path, timeout, &mut || None), Readiness::Listening)
}

/// Wait for the daemon to answer on `path`, watching the child at the same time.
///
/// Watching the child is not a refinement. With nothing configured anywhere
/// `wispd` prints what to do about it and exits 2 at once, and a wait that
/// watched only the socket would sit out the whole timeout and then report the
/// wrong thing — that the daemon "did not listen" — when the real answer is that
/// it died and said why.
pub fn wait_for_daemon(child: &mut Child, path: &Path, timeout: Duration) -> Readiness {
    wait_until(path, timeout, &mut || exited(child))
}

/// One loop for both waits: a connect attempt, then the caller's own check for a
/// child that stopped, then a sleep, until the deadline has passed.
fn wait_until(
    path: &Path,
    timeout: Duration,
    child: &mut dyn FnMut() -> Option<ExitStatus>,
) -> Readiness {
    let deadline = Instant::now() + timeout;
    loop {
        if is_listening(path) {
            return Readiness::Listening;
        }
        if let Some(status) = child() {
            return Readiness::Exited(status);
        }
        if Instant::now() >= deadline {
            return Readiness::TimedOut;
        }
        std::thread::sleep(POLL);
    }
}

/// A command was given after `--`, so this is a launch and not a session: the
/// `mangohud %command%` convention, which is what makes a Steam or Lutris launch
/// option of `wisp run -- %command%` work.
///
/// The command's own status is what comes back, whatever Wisp does meanwhile. If
/// the HUD or the daemon dies, that is said and the other Wisp child is stopped,
/// but the command is left alone: the game is what the user launched, and losing
/// an overlay is no reason to close it.
fn follow_command(daemon: &mut Child, hud: &mut Child, command: &[OsString]) -> i32 {
    let mut command = match Command::new(&command[0]).args(&command[1..]).spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("wisp: cannot start {}: {e}", command[0].to_string_lossy());
            stop(daemon);
            stop(hud);
            return 1;
        }
    };
    loop {
        // The command first: when it ends, so does the launch, and the two Wisp
        // children go with it whichever of them is still alive.
        if let Some(status) = exited(&mut command) {
            stop(daemon);
            stop(hud);
            return command_status(status);
        }
        if let Some(status) = exited(daemon) {
            eprintln!("wisp: wispd exited: {status}; leaving the command running");
            stop(hud);
            return wait_for(&mut command);
        }
        if let Some(status) = exited(hud) {
            eprintln!("wisp: wisp-hud exited: {status}; leaving the command running");
            stop(daemon);
            return wait_for(&mut command);
        }
        std::thread::sleep(POLL);
    }
}

/// No command after `--`: run until the user stops it or one of the children
/// does, then stop the other.
///
/// **Signal handling is deliberately minimal.** Both children are in this
/// process's group and share its terminal, so SIGINT from Ctrl-C reaches all
/// three at once and what follows is a poll that notices. No signal crate, no
/// self-pipe, no `libc`. The limitation, and it is accepted for Spec 4: a
/// launcher signalled on its own rather than as a group — `kill <wisp's pid>`,
/// not a terminal — leaves this loop running with both children alive, and a
/// daemon can therefore outlive the `wisp` that started it. Stopping the group,
/// or closing the terminal, ends all three.
fn follow_wisp(daemon: &mut Child, hud: &mut Child) -> i32 {
    loop {
        if let Some(status) = exited(daemon) {
            eprintln!("wisp: wispd exited: {status}");
            stop(hud);
            return wisp_status(status);
        }
        if let Some(status) = exited(hud) {
            eprintln!("wisp: wisp-hud exited: {status}");
            stop(daemon);
            return wisp_status(status);
        }
        std::thread::sleep(POLL);
    }
}

/// The command's status, waited for once the other two children are dealt with.
fn wait_for(command: &mut Child) -> i32 {
    match command.wait() {
        Ok(status) => command_status(status),
        Err(e) => {
            eprintln!("wisp: cannot wait on the command: {e}");
            1
        }
    }
}

/// A command's own exit status, the way a shell reports it: its code, or 128
/// plus the signal that killed it. A game that was terminated must not look like
/// a game that finished.
fn command_status(status: ExitStatus) -> i32 {
    status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

/// A Wisp child that stopped, as a launcher reports it.
///
/// A signal is the interrupt path — the terminal delivers SIGINT to the whole
/// group, which is how `wisp run` is meant to be stopped — so it is a clean stop
/// and returns 0. A non-zero exit code is a failure. The trade this accepts: a
/// child that died of a signal for another reason, a crash rather than an
/// interrupt, is reported as a clean stop too, because telling the two apart
/// needs a signal handler this launcher deliberately does not have.
fn wisp_status(status: ExitStatus) -> i32 {
    match status.code() {
        Some(0) | None => 0,
        Some(_) => 1,
    }
}

/// `Some` once the child has stopped, which also reaps it.
///
/// An `Err` from `try_wait` means a child this process does not have, which
/// cannot happen while `run` owns it; it is read as "still running" rather than
/// handled.
fn exited(child: &mut Child) -> Option<ExitStatus> {
    child.try_wait().ok().flatten()
}

/// Stop a child and reap it, so nothing is left running and nothing is left
/// unwaited-for.
///
/// Killed rather than asked: an overlay that outlives its launcher stays on
/// screen with nothing to close it, and a daemon that ignored SIGTERM once is
/// not going to be argued with. Its own state is already on disk — the duration
/// store is saved as samples arrive, not at exit.
fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    /// A scratch directory of its own per test — the harness runs them in
    /// parallel inside one process — removed when the test ends, whether it
    /// passed or panicked. Nothing this suite creates outlives it, socket files
    /// included: they live inside the tree.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let path = std::env::temp_dir().join(format!("wisp-run-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A file that can be started. `fs::write` leaves mode 0644, and
    /// `find_child` passes a candidate over unless one of its execute bits is
    /// set, so a test that means "this is the child" has to say so.
    fn executable(path: &Path) {
        fs::write(path, b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    /// One file the test put somewhere it does not own, removed on the way out
    /// even if an assertion panicked first.
    struct Remove(PathBuf);

    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn find_child_prefers_the_directory_of_the_current_exe() {
        // Both directories hold one, so the answer says which wins. The sibling
        // of the real `wisp` is what makes an AppImage work with a two-line
        // AppRun, so it has to come first.
        let scratch = Scratch::new("find-child");
        let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let name = format!("wisp-cli-test-child-{}", std::process::id());
        let beside_exe = exe_dir.join(&name);
        let on_path = scratch.join(&name);
        executable(&beside_exe);
        executable(&on_path);
        let _remove = Remove(beside_exe.clone());

        assert_eq!(
            find_child_in(&name, Some(&exe_dir), scratch.path().as_os_str()),
            Some(beside_exe.clone())
        );
        // And the exported function really consults the directory this binary
        // is in, rather than only the PATH the test process happens to have.
        assert_eq!(find_child(&name), Some(beside_exe));
    }

    #[test]
    fn find_child_falls_back_to_path() {
        let scratch = Scratch::new("find-child-path");
        let empty = Scratch::new("find-child-empty");
        let name = format!("wisp-cli-test-on-path-{}", std::process::id());
        let on_path = scratch.join(&name);
        executable(&on_path);

        assert_eq!(
            find_child_in(&name, Some(empty.path()), scratch.path().as_os_str()),
            Some(on_path)
        );
        // Neither place holds it: no guess, and no current directory.
        let missing = format!("wisp-cli-test-missing-{}", std::process::id());
        assert_eq!(find_child_in(&missing, Some(empty.path()), scratch.path().as_os_str()), None);
        assert_eq!(find_child(&missing), None);
    }

    #[test]
    fn find_child_skips_a_non_executable_candidate() {
        // Two PATH entries: the first holds a file that cannot be started, the
        // second one that can. Passing the first over is what keeps a stray file
        // from becoming a spawn that fails with EACCES.
        let blocked_dir = Scratch::new("find-child-0644");
        let runnable_dir = Scratch::new("find-child-0755");
        let name = format!("wisp-cli-test-mode-{}", std::process::id());
        let blocked = blocked_dir.join(&name);
        let runnable = runnable_dir.join(&name);
        fs::write(&blocked, b"#!/bin/sh\n").unwrap();
        executable(&runnable);
        assert_eq!(fs::metadata(&blocked).unwrap().permissions().mode() & 0o111, 0);

        let path = OsString::from(format!(
            "{}:{}",
            blocked_dir.path().display(),
            runnable_dir.path().display()
        ));
        assert_eq!(find_child_in(&name, None, &path), Some(runnable.clone()));
        // Beside the executable is skipped the same way, and with nothing
        // executable anywhere the answer is none rather than a guess.
        assert_eq!(find_child_in(&name, Some(blocked_dir.path()), &path), Some(runnable));
        assert_eq!(find_child_in(&name, Some(blocked_dir.path()), &OsString::new()), None);
    }

    #[test]
    fn wait_for_socket_is_false_for_a_path_nothing_listens_on() {
        let scratch = Scratch::new("wait-socket-none");
        let path = scratch.join("wispd.sock");
        let started = Instant::now();
        assert!(!wait_for_socket(&path, Duration::from_millis(200)));
        assert!(
            started.elapsed() >= Duration::from_millis(150),
            "it waits the timeout out rather than giving up on the first refusal"
        );
        // A path that exists as a file and nobody is listening on is the same
        // answer: readiness is a successful connect, never a file test.
        fs::write(&path, b"not a socket").unwrap();
        assert!(!wait_for_socket(&path, Duration::from_millis(200)));
    }

    #[test]
    fn wait_for_socket_is_true_once_a_listener_exists() {
        let scratch = Scratch::new("wait-socket-some");
        let path = scratch.join("wispd.sock");
        // Bound after the wait has begun, so what is proven is the waiting and
        // not the ordering of two statements in the test.
        let bind_at = path.clone();
        let listener = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            UnixListener::bind(&bind_at).unwrap()
        });
        assert!(wait_for_socket(&path, Duration::from_secs(5)));
        let _listener = listener.join().unwrap();
        // Immediately, too.
        assert!(wait_for_socket(&path, Duration::from_millis(200)));
    }
}
