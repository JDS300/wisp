// SPDX-License-Identifier: MIT
//! The CLI end to end: every command spawned as a process, against a scratch
//! environment that cannot reach the developer's own `~/.config`,
//! `~/.local/share` or `/run/user`.
//!
//! Three binaries are involved — `wisp`, and the `wispd` and `wisp-hud` it
//! starts — so every child here is owned by a guard that kills it and every
//! scratch tree by one that removes it, whether the test passed or panicked. A
//! leaked daemon would break the test after it.

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Long enough for a daemon to start and a HUD to attach on a loaded box, short
/// enough that a hung child fails one test instead of the suite.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The two launch lines the `run` tests are specified with, each as one command
/// line rather than a list of words.
///
/// A backend name has to appear in them: a launch names one, and the plain
/// window is the only backend that works without gamescope or a layer-shell
/// compositor, which is why both lines ask for it. It is spelled inside a
/// command line this test hands to a child process, and never as a value this
/// crate reads — the three names are `wisp_probe::BackendKind::parse`'s to match
/// against text, and `crates/wisp` states no backend vocabulary of its own.
const RUN_STUB_FOR_ONE_SECOND: &str = "run --stub --backend plain -- sleep 1";
const RUN_STUB_FOR_THREE_SECONDS: &str = "run --stub --backend plain -- sleep 3";

static IO_ERROR_CALLS: AtomicUsize = AtomicUsize::new(0);

// ---------------------------------------------------------------------------
// The scratch tree
// ---------------------------------------------------------------------------

/// One test's whole world. Nothing here can reach a real config file, whose
/// `logs_dir` would otherwise decide what these tests discover, or the real
/// `/run/user/1000`, where a developer's own daemon may be listening.
struct Scratch {
    root: PathBuf,
    runtime: PathBuf,
    config_home: PathBuf,
}

fn scratch(tag: &str) -> Scratch {
    // The harness runs these in parallel inside one process, so the tag is what
    // makes each tree unique, not the pid.
    let root = std::env::temp_dir().join(format!("wisp-cli-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let runtime = root.join("runtime");
    let config_home = root.join("config");
    let data_home = root.join("data");
    for dir in [&runtime, &config_home, &data_home] {
        fs::create_dir_all(dir).unwrap();
    }
    Scratch { root, runtime, config_home }
}

impl Scratch {
    /// `$XDG_RUNTIME_DIR/wisp/wispd.sock`, spelled out rather than asked of
    /// `wisp_config::paths`: a test that derived the path from the code under
    /// test could not notice the rule changing.
    fn socket(&self) -> PathBuf {
        self.runtime.join("wisp").join("wispd.sock")
    }

    /// `$XDG_CONFIG_HOME/wisp/config`. The `wisp/` directory is *not* created
    /// here: `config_path()` does not create it, and a file at
    /// `$XDG_CONFIG_HOME/config` is read by nothing, which would silently turn
    /// a config test into a no-config test.
    fn config_file(&self) -> PathBuf {
        self.config_home.join("wisp").join("config")
    }

    /// Write the config file, creating the `wisp/` directory it lives in.
    fn write_config(&self, text: &str) {
        self.write_config_bytes(text.as_bytes());
    }

    /// The same, for a file that is not valid UTF-8: the case every binary has
    /// to report once and then ignore.
    fn write_config_bytes(&self, bytes: &[u8]) {
        let path = self.config_file();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
    }

    fn command(&self, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_CONFIG_HOME", &self.config_home)
            .env("XDG_DATA_HOME", self.root.join("data"))
            // A Flatpak id would move the socket under app/<id>/; these tests
            // are not running in one.
            .env_remove("FLATPAK_ID")
            .stdin(Stdio::null());
        cmd
    }

    /// Spawn one of the three binaries with this environment and its stderr
    /// captured.
    fn spawn(&self, program: &Path, args: &[&str]) -> Running {
        let mut cmd = self.command(program);
        cmd.args(args).stdout(Stdio::null()).stderr(Stdio::piped());
        Running::spawn(cmd)
    }

    /// Spawn with a command line given as one string and split on whitespace.
    fn spawn_words(&self, program: &Path, words: &str) -> Running {
        let args: Vec<&str> = words.split_whitespace().collect();
        self.spawn(program, &args)
    }

    /// Spawn `wispd --stub` and wait until it is listening.
    fn stub_daemon(&self) -> Running {
        let daemon = self.spawn(&bin("wispd"), &["--stub"]);
        wait_for_socket(&self.socket());
        daemon
    }

    /// Every process whose environment carries this scratch `XDG_RUNTIME_DIR`.
    ///
    /// The directory is unique to one test, so this is "our processes" without a
    /// name match: `pgrep -f 'wispd --stub'` matches the command line only, so
    /// it would also reach a developer's own stub daemon from another checkout,
    /// and killing that is not this suite's business.
    fn procs(&self) -> Vec<u32> {
        let want = format!("XDG_RUNTIME_DIR={}", self.runtime.display()).into_bytes();
        let mut found = Vec::new();
        let Ok(entries) = fs::read_dir("/proc") else { return found };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|text| text.parse::<u32>().ok()) else {
                continue;
            };
            if pid == std::process::id() {
                continue;
            }
            let Ok(environ) = fs::read(format!("/proc/{pid}/environ")) else { continue };
            if environ.split(|byte| *byte == 0).any(|var| var == want) {
                found.push(pid);
            }
        }
        found.sort_unstable();
        found
    }

    /// One of [`Scratch::procs`] whose executable is `name`, waited for.
    ///
    /// By environment and executable, never by command line: `pgrep -f 'wispd
    /// --stub'` would also match a developer's own stub daemon from another
    /// checkout, and killing that is not this suite's business.
    fn wait_for_child(&self, name: &str) -> u32 {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let found = self
                .procs()
                .into_iter()
                .find(|pid| exe_name(*pid).as_deref() == Some(name));
            match found {
                Some(pid) => return pid,
                None if Instant::now() < deadline => sleep_ms(20),
                None => panic!("no {name} of this test's own within {TIMEOUT:?}"),
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A spawned child and everything it printed. `Drop` kills it, so an assertion
/// that fails cannot leave a daemon behind for the next test to trip over.
struct Running {
    child: Child,
    stderr: Arc<Mutex<String>>,
}

impl Running {
    fn spawn(mut cmd: Command) -> Running {
        let mut child = cmd.spawn().expect("the binary spawns");
        let pipe = child.stderr.take().expect("stderr is piped");
        let stderr: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&stderr);
        // Drained by its own thread: a full pipe would block the child.
        std::thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => sink.lock().unwrap().push_str(&line),
                }
            }
        });
        Running { child, stderr }
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn stderr(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }

    /// The status it exited with, or a panic rather than a hung suite.
    fn wait_within(&mut self, timeout: Duration) -> ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return status,
                Ok(None) if Instant::now() < deadline => sleep_ms(50),
                Ok(None) => {
                    let _ = self.child.kill();
                    panic!("still running after {timeout:?}; stderr so far:\n{}", self.stderr());
                }
                Err(e) => panic!("cannot wait on the child: {e}"),
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Binaries, sockets, signals
// ---------------------------------------------------------------------------

/// `wisp` itself.
fn wisp() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_wisp"))
}

/// One of `wisp`'s siblings in the same target directory.
///
/// `CARGO_BIN_EXE_<name>` is set only for the package's own binaries, so
/// `wispd` and `wisp-hud` are found the way `wisp run` finds them: beside the
/// executable. They are workspace members, so `cargo test --workspace` has built
/// them; a bare `cargo test -p wisp` has not, and says so rather than skipping.
fn bin(name: &str) -> PathBuf {
    let dir = wisp().parent().expect("the wisp binary is in a directory").to_path_buf();
    let path = dir.join(name);
    assert!(
        path.is_file(),
        "{} is missing: run `cargo build --workspace` before `cargo test -p wisp`",
        path.display()
    );
    path
}

/// Readiness is a successful connect, never the appearance of the file: a dead
/// daemon leaves its socket behind, and `Server::bind` removes whatever is at
/// the path before binding, so a file test can pass too early.
fn wait_for_socket(path: &Path) {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if UnixStream::connect(path).is_ok() {
            return;
        }
        sleep_ms(20);
    }
    panic!("nothing listened on {} within {TIMEOUT:?}", path.display());
}

fn exe_name(pid: u32) -> Option<String> {
    let link = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    Some(link.file_name()?.to_string_lossy().into_owned())
}

/// No `libc` in this crate's dependencies, and a test that needs a signal can
/// ask the shell for one.
fn kill_pid(pid: u32) {
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("kill -9 {pid}"))
        .status()
        .expect("sh runs");
    assert!(status.success(), "kill -9 {pid} failed");
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

/// True — and loud about it — when there is no display, so the two tests that
/// start the HUD skip rather than fail. Printed rather than silent: a skip must
/// not be mistaken for a pass.
fn no_display(test: &str) -> bool {
    if std::env::var("DISPLAY").is_err() {
        eprintln!("skipping {test}: DISPLAY is not set, and wisp-hud needs one");
        return true;
    }
    false
}

fn create_log(dir: &Path, name: &str, secs_ago: u64) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, b"").unwrap();
    let file = fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(secs_ago)).unwrap();
    path
}

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

#[test]
fn config_set_then_show_round_trips_a_path_with_spaces() {
    let s = scratch("config-set-spaces");
    let value = "/a path/with spaces/Logs";
    let out = s
        .command(&wisp())
        .args(["config", "set", "logs_dir", value])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // `config set` creates the directory the file lives in; nothing else does.
    assert_eq!(fs::read_to_string(s.config_file()).unwrap(), format!("logs_dir = {value}\n"));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n", s.config_file().display()),
        "it prints the path it wrote"
    );

    let out = s.command(&wisp()).arg("config").arg("show").output().unwrap();
    let shown = String::from_utf8_lossy(&out.stdout).into_owned();
    // Byte for byte: the value is everything after the first `=`, trimmed, so a
    // path with spaces needs no quoting and survives the round trip whole.
    assert!(shown.contains(&format!("logs_dir = {value}\n")), "{shown}");
    assert!(shown.contains("log = (unset)\n"), "{shown}");
    assert!(shown.contains("scale = (unset)\n"), "{shown}");

    // Nothing but the config file: the write went through a temporary in the
    // same directory, and the rename took it away.
    let left: Vec<String> = fs::read_dir(s.config_file().parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["config"], "the atomic write left something behind");
}

#[test]
fn config_set_preserves_a_comment() {
    let s = scratch("config-set-comment");
    s.write_config("# Wisp\n# log = /commented-out\nlog = /a\n");
    let out = s
        .command(&wisp())
        .args(["config", "set", "scale", "32"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        fs::read_to_string(s.config_file()).unwrap(),
        "# Wisp\n# log = /commented-out\nlog = /a\nscale = 32\n"
    );

    // Setting a key that is already there replaces its line and leaves the rest.
    let out = s
        .command(&wisp())
        .args(["config", "set", "log", "/b"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        fs::read_to_string(s.config_file()).unwrap(),
        "# Wisp\n# log = /commented-out\nlog = /b\nscale = 32\n"
    );
}

#[test]
fn config_set_refuses_an_unknown_key_with_exit_2() {
    let s = scratch("config-set-unknown");
    let out = s.command(&wisp()).args(["config", "set", "nope", "x"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "wisp: unknown config key: nope (one of: log, logs_dir, spells_dir, scale, backend)\n"
    );
    // Nothing was written: a refusal is not a partial write.
    assert!(!s.config_file().exists());
}

#[test]
fn config_path_prints_the_file_it_would_use() {
    let s = scratch("config-path");
    let out = s.command(&wisp()).args(["config", "path"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{}\n", s.config_file().display()));
    assert!(String::from_utf8_lossy(&out.stderr).is_empty());
}

#[test]
fn version_prints_wisp_and_the_crate_version() {
    let s = scratch("version");
    for form in [["version"], ["--version"]] {
        let out = s.command(&wisp()).args(form).output().unwrap();
        assert!(out.status.success(), "{form:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("wisp {}\n", env!("CARGO_PKG_VERSION")),
            "{form:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

#[test]
fn status_json_against_a_stub_daemon_is_one_v3_line() {
    let s = scratch("status-json");
    let _daemon = s.stub_daemon();

    let out = s.command(&wisp()).args(["status", "--json"]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "one snapshot, one line: {stdout}");
    let snapshot = wisp_proto::decode(lines[0]).expect("a line the codec accepts");
    assert_eq!(snapshot.v, 3);
    assert_eq!(snapshot.v, wisp_proto::PROTOCOL_VERSION);

    // The text form of the same daemon names the fields rather than the JSON.
    let out = s.command(&wisp()).arg("status").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{stdout}");
    for label in ["log time:", "lines:", "kills:", "timers:", "fight:"] {
        assert!(stdout.contains(label), "{stdout}");
    }
    assert!(!stdout.contains('{'), "the text form is not JSON: {stdout}");
}

#[test]
fn status_without_a_daemon_exits_1_and_names_the_socket() {
    let s = scratch("status-no-daemon");
    let out = s.command(&wisp()).arg("status").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        format!("wisp: no daemon is listening on {}\n", s.socket().display())
    );
    assert!(String::from_utf8_lossy(&out.stdout).is_empty());
}

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

#[test]
fn run_refuses_to_start_when_a_daemon_is_already_listening() {
    let s = scratch("run-refuses");
    let mut daemon = s.stub_daemon();
    let before = s.procs();

    let out = s.command(&wisp()).args(["run", "--stub"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains(&format!("wisp: a daemon is already listening on {}", s.socket().display())),
        "{stderr}"
    );
    // Start nothing: the running daemon is untouched and no second process of
    // this environment exists.
    assert!(daemon.alive(), "the daemon that was already listening was disturbed");
    assert_eq!(s.procs(), before, "no child was spawned");
    assert!(String::from_utf8_lossy(&out.stdout).is_empty());
}

#[test]
fn run_reports_a_daemon_that_exits_before_listening() {
    let s = scratch("run-daemon-dies");
    // No log anywhere: no config file and no flags. wispd says what to do about
    // it and exits 2 at once, so the launcher has to report that instead of
    // waiting out its five seconds on a socket that will never open. No display
    // is needed: the HUD is started only once the socket answers, so it never
    // is.
    let started = Instant::now();
    let out = s.command(&wisp()).arg("run").output().unwrap();
    let elapsed = started.elapsed();

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains("wisp: wispd exited before listening: exit status: 2"),
        "{stderr}"
    );
    assert!(!stderr.contains("did not listen"), "the timeout report is the wrong one: {stderr}");
    // Bounded, because not waiting is the whole of the change: a socket-only
    // loop fails the two assertions above and passes this one five seconds
    // late, so this is what keeps the test from degrading into a slow pass.
    assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
    // Nothing was left to clean up, and nothing else was started.
    assert!(s.procs().is_empty());
}

#[test]
fn run_stub_sleep_starts_and_stops_everything() {
    if no_display("run_stub_sleep_starts_and_stops_everything") {
        return;
    }
    let s = scratch("run-stub-sleep");
    let mut running = s.spawn_words(&wisp(), RUN_STUB_FOR_ONE_SECOND);

    let status = running.wait_within(TIMEOUT);
    let stderr = running.stderr();
    assert_eq!(status.code(), Some(0), "stderr:\n{stderr}");
    // The HUD was really started, with the flag it was given: `--backend` is the
    // only one of the seven this launch uses on the HUD's side.
    assert!(stderr.contains("wisp-hud: backend PlainWindow"), "{stderr}");

    // Everything stopped with the command: no daemon survived to answer.
    let out = s.command(&wisp()).arg("status").output().unwrap();
    assert_eq!(out.status.code(), Some(1), "a daemon outlived the launch");
    assert!(s.procs().is_empty(), "left behind: {:?}", s.procs());
}

#[test]
fn a_dying_wisp_child_leaves_the_command_running() {
    if no_display("a_dying_wisp_child_leaves_the_command_running") {
        return;
    }
    let s = scratch("run-dying-child");
    let mut running = s.spawn_words(&wisp(), RUN_STUB_FOR_THREE_SECONDS);
    // The HUD is the signal that `wisp run` has passed its own readiness check:
    // it starts the HUD only once the socket answers, so the daemon killed below
    // is one that was running and not one still on its way up. Killing it too
    // early would test the 5 s timeout instead of this.
    let _hud = s.wait_for_child("wisp-hud");
    let daemon = s.wait_for_child("wispd");
    kill_pid(daemon);

    // The spec's ruling: the game is what the user launched, and losing an
    // overlay is no reason to close it. `sleep 3` is still running, so `wisp
    // run` must still be too — and must still be at two seconds, which is what
    // distinguishes waiting for the command from waiting for nothing.
    sleep_ms(700);
    assert!(running.alive(), "wisp run gave up while the command was still running");
    sleep_ms(700);
    assert!(running.alive(), "wisp run gave up while the command was still running");

    let status = running.wait_within(TIMEOUT);
    let stderr = running.stderr();
    // The command's own status is what comes back, not the daemon's.
    assert_eq!(status.code(), Some(0), "stderr:\n{stderr}");
    assert!(stderr.contains("wispd"), "it says which child died: {stderr}");
    // The other Wisp child went with it, and nothing of this test's survives.
    assert!(s.procs().is_empty(), "left behind: {:?}", s.procs());
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

#[test]
fn doctor_exits_1_and_prints_the_config_path_when_no_log_resolves() {
    let s = scratch("doctor-no-log");
    let out = s.command(&wisp()).arg("doctor").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains(&format!("version:   wisp {}\n", env!("CARGO_PKG_VERSION"))),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("config:    {} (missing)", s.config_file().display())),
        "{stdout}"
    );
    assert!(stdout.contains("log:       none\n"), "{stdout}");
    assert!(
        stdout.contains(&format!("socket:    {} (nothing listening)", s.socket().display())),
        "{stdout}"
    );
    assert!(stdout.contains("scale:     48 (default)\n"), "{stdout}");
    // Detection ran, so a backend is named with both of its observations; which
    // one it is depends on the machine, and is not this test's business.
    assert!(stdout.contains("backend:   "), "{stdout}");
}

#[test]
fn doctor_reports_a_missing_log_file_and_exits_1() {
    let s = scratch("doctor-missing-log");
    let missing = s.root.join("eqlog_Daggo_freeport.txt");
    s.write_config(&format!("log = {}\n", missing.display()));

    // A log that is not there is a launch that fails: `wispd` dies on this same
    // config, so the report must not call it healthy.
    let out = s.command(&wisp()).arg("doctor").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains(&format!("log:       {} (config log, missing)", missing.display())),
        "{stdout}"
    );

    // The same path named by a flag blames the flag, and is the same failure.
    let out = s.command(&wisp()).args(["doctor", "--log"]).arg(&missing).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains(&format!("log:       {} (--log flag, missing)", missing.display())),
        "{stdout}"
    );

    // A `logs_dir` holding no `eqlog_*.txt` is the same answer by the other
    // route, and the count is what says so: this is the wrong `Logs`, which is
    // the risk the spec names.
    let logs = s.root.join("install").join("Logs");
    fs::create_dir_all(&logs).unwrap();
    let out = s.command(&wisp()).args(["doctor", "--logs-dir"]).arg(&logs).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains(&format!("log:       {} (--logs-dir flag, newest of 0 files)", logs.display())),
        "{stdout}"
    );
}

#[test]
fn doctor_exits_0_and_names_the_newest_file_when_logs_dir_is_set() {
    let s = scratch("doctor-logs-dir");
    // Named `Logs` and nested under `install/`, so the real derivation rule for
    // the client install applies to it.
    let logs = s.root.join("install").join("Logs");
    fs::create_dir_all(&logs).unwrap();
    create_log(&logs, "eqlog_Daggo_rivervale.txt", 60);
    let newest = create_log(&logs, "eqlog_Daggo_freeport.txt", 0);
    s.write_config(&format!("logs_dir = {}\n", logs.display()));

    let out = s.command(&wisp()).arg("doctor").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "log:       {} (config logs_dir, newest of 2 files)",
            newest.display()
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("spells:    {}", s.root.join("install").display())),
        "{stdout}"
    );
    // The install has no `spells_us.txt` in it — nothing here may create one,
    // and no game data belongs in the repository — so doctor says it is missing
    // rather than guessing that timers will work.
    assert!(stdout.contains("spells_us.txt missing"), "{stdout}");

    // The same answers from the flags rather than the file, which is the launch
    // a user actually asks about. Every origin now says "flag", and the config
    // file's own `logs_dir` is beaten by the one on the command line.
    let install = s.root.join("install");
    let mut cmd = s.command(&wisp());
    cmd.arg("doctor")
        .arg("--logs-dir")
        .arg(&logs)
        .arg("--spells")
        .arg(&install)
        .arg("--scale")
        .arg("32");
    let out = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "log:       {} (--logs-dir flag, newest of 2 files)",
            newest.display()
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "spells:    {} (--spells flag, spells_us.txt missing)",
            install.display()
        )),
        "{stdout}"
    );
    assert!(stdout.contains("scale:     32 (--scale flag)"), "{stdout}");
}

// ---------------------------------------------------------------------------
// An unreadable config, in all three of the binaries that read one
// ---------------------------------------------------------------------------

/// The bytes are not valid UTF-8, so `Config::load` cannot return text.
const UNREADABLE: &[u8] = b"log = \xff\xfe\n";

#[test]
fn an_unreadable_config_is_reported_once_and_ignored() {
    let s = scratch("unreadable-config");
    s.write_config_bytes(UNREADABLE);
    let prefix = format!("wisp: ignoring unreadable config {}: ", s.config_file().display());

    // doctor still runs, and still exits 1 for the log that cannot be resolved
    // from a file it could not read.
    let out = s.command(&wisp()).arg("doctor").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(occurrences(&stderr, &prefix), 1, "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "said once and nothing else: {stderr}");
    assert!(stderr.contains(&io_error_for(UNREADABLE)), "{stderr}");

    // status says the same line, once, and still prints the snapshot: an
    // unreadable config is not a reason to refuse a daemon that is running.
    let _daemon = s.stub_daemon();
    let out = s.command(&wisp()).arg("status").output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(occurrences(&stderr, &prefix), 1, "{stderr}");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("kills:"), "{stdout}");
}

#[test]
fn config_set_refuses_to_overwrite_a_file_it_cannot_read() {
    let s = scratch("config-set-unreadable");
    s.write_config_bytes(UNREADABLE);

    let out = s.command(&wisp()).args(["config", "set", "scale", "32"]).output().unwrap();
    // A writer is the one exception: a file it cannot read is a file it must not
    // overwrite, so it stops rather than replace bytes it could not read with a
    // single key.
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        stderr,
        format!(
            "wisp: ignoring unreadable config {}: {}\n",
            s.config_file().display(),
            io_error_for(UNREADABLE)
        )
    );
    assert_eq!(fs::read(s.config_file()).unwrap(), UNREADABLE.to_vec(), "not one byte replaced");
}

/// The `io::Error` a file of these bytes earns, spelled the way the standard
/// library spells it, so the whole message can be asserted rather than a prefix
/// of it. Read here rather than through `wisp_config::config::Config::load`: an
/// expectation taken from the code under test would agree with it by
/// construction.
fn io_error_for(bytes: &[u8]) -> String {
    // One directory per call: the harness runs these in parallel inside one
    // process, and two tests ask for the same bytes.
    let call = IO_ERROR_CALLS.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("wisp-cli-io-error-{}-{call}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config");
    fs::write(&path, bytes).unwrap();
    let error = fs::read_to_string(&path).expect_err("the file is not text").to_string();
    fs::remove_dir_all(&dir).unwrap();
    error
}

#[test]
fn the_hud_refuses_a_bad_config_scale_with_exit_2() {
    let s = scratch("hud-bad-scale");
    s.write_config("scale = not-a-number\n");

    // No display is needed: the refusal happens before detection and attach, so
    // this runs in CI and must not be DISPLAY-gated. It is the only automated
    // test that reaches the HUD's refusal wiring, since the message alone could
    // be unit-tested but the process exit could not.
    let out = s.command(&bin("wisp-hud")).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "wisp-hud: invalid config scale: not-a-number\n"
    );
}

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

#[test]
fn bare_wisp_and_an_unknown_command_are_usage_with_exit_2() {
    let s = scratch("usage");
    for argv in [Vec::new(), vec!["nonsense"], vec!["config"], vec!["run", "--nonsense"]] {
        let out = s.command(&wisp()).args(&argv).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{argv:?}");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(stderr.starts_with("usage: wisp"), "{argv:?}: {stderr}");
        assert!(String::from_utf8_lossy(&out.stdout).is_empty(), "{argv:?}");
        // Only `--log` and `--logs-dir` are alternatives to each other.
        // `--spells` is an extra daemon flag and `--scale` and `--backend` are
        // two independent HUD flags, so the usage must not offer any of them as
        // a choice between the flags beside it.
        assert!(stderr.contains("[--log <path> | --logs-dir <dir>] [--spells <dir>]"), "{stderr}");
        assert!(stderr.contains("[--scale <px>] [--backend <name>]"), "{stderr}");
        // Usage names every command, so the dump is the documentation it is.
        for command in ["run", "status", "doctor", "config", "version"] {
            assert!(stderr.contains(command), "{argv:?}: {stderr}");
        }
    }
    // And nothing was started by any of them.
    assert!(s.procs().is_empty());
}
