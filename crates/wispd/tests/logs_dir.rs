// SPDX-License-Identifier: MIT
//! End-to-end log discovery: a daemon pointed at a directory finds the newest
//! `eqlog_*.txt`, follows it when a newer one appears, resets the session when
//! it switches, and keeps running when there is nothing to read.
//!
//! A discovered file is opened at its end, so every log here is created empty
//! and appended to only once the daemon holds it open: [`wait_until_open`] says
//! exactly when that happened. Lines already in a file when the daemon finds it
//! are never read, and a test that wrote them first would assert against a
//! count that could never move. The one exception is
//! [`from_start_applies_only_to_the_first_file_the_process_opens`], whose whole
//! subject is the content a file already holds when the daemon reaches it, and
//! which says so below.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use wisp_proto::{Confidence, Snapshot, PROTOCOL_VERSION};

/// The daemon ticks every 250 ms. Every wait here is bounded by this, so a
/// stuck daemon fails one test instead of hanging the suite.
const TIMEOUT: Duration = Duration::from_secs(3);
/// Three ticks: long enough that a message printed per tick would have repeated,
/// short enough to keep the suite quick. This is how "printed once" is proved.
const SETTLE: Duration = Duration::from_millis(750);

// ---------------------------------------------------------------------------
// The scratch tree, the spawned daemon, and the snapshot reader
// ---------------------------------------------------------------------------

/// One test's whole world: a scratch tree, the environment pointing into it,
/// and the command line to spawn. Nothing here can reach the developer's own
/// `~/.config`, `~/.local/share` or `/run/user` — a real config file with a
/// `logs_dir` in it would otherwise decide what these tests discover.
struct Setup {
    root: PathBuf,
    /// The directory the daemon is pointed at. Named `Logs` and nested under
    /// `install/` so the real derivation rule for the client install applies.
    logs: PathBuf,
    config: PathBuf,
    socket: PathBuf,
    durations: PathBuf,
    cmd: Command,
}

fn setup(tag: &str) -> Setup {
    // The harness runs these in parallel inside one process, so the tag is
    // what makes each tree unique, not the pid.
    let root = std::env::temp_dir().join(format!("wispd-logs-dir-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let runtime = root.join("runtime");
    let data = root.join("data");
    let config = root.join("config");
    let logs = root.join("install").join("Logs");
    for dir in [&runtime, &data, &config, &logs] {
        fs::create_dir_all(dir).unwrap();
    }

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wispd"));
    cmd.env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CONFIG_HOME", &config)
        // A Flatpak id would move the socket under app/<id>/; these tests are
        // not running in one.
        .env_remove("FLATPAK_ID")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    Setup {
        socket: runtime.join("wisp").join("wispd.sock"),
        durations: data.join("wisp").join("durations.json"),
        root,
        logs,
        config,
        cmd,
    }
}

impl Setup {
    /// Spawn `wispd --logs-dir <the scratch Logs/> [extra...]`.
    fn spawn_logs_dir(mut self, extra: &[&str]) -> Daemon {
        let logs = self.logs.clone();
        self.cmd.arg("--logs-dir").arg(logs).args(extra);
        Daemon::spawn(self)
    }
}

/// A spawned daemon and everything it owns.
///
/// `Drop` kills the child, restores any mode a test changed, and removes the
/// scratch tree — in that order, because a directory left at mode 000 cannot
/// be removed. A failing assertion therefore cannot leave a `wispd` behind to
/// break the next test.
struct Daemon {
    child: Child,
    root: PathBuf,
    logs: PathBuf,
    socket: PathBuf,
    durations: PathBuf,
    /// A directory whose mode a test changed, and the mode that makes it
    /// removable again.
    restore: Option<(PathBuf, u32)>,
    /// Everything the daemon has printed, drained by its own thread so a full
    /// pipe can never block it.
    stderr: Arc<Mutex<String>>,
}

impl Daemon {
    fn spawn(setup: Setup) -> Daemon {
        let Setup { root, logs, socket, durations, mut cmd, .. } = setup;
        let mut child = cmd.spawn().expect("wispd spawns");
        let pipe = child.stderr.take().expect("stderr is piped");
        let stderr: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&stderr);
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
        Daemon { child, root, logs, socket, durations, restore: None, stderr }
    }

    /// Still running: an unexpected exit is what most of these tests are about
    /// not doing.
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn stderr(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }

    /// How many times `needle` has appeared on stderr, waiting for `want` of
    /// them and then watching for [`SETTLE`] longer. Asking for 1 and getting
    /// 1 back means it was printed once, not once per tick; asking for 0 is how
    /// a message that must never appear is checked.
    fn count_stderr(&self, needle: &str, want: usize) -> usize {
        let deadline = Instant::now() + TIMEOUT;
        while occurrences(&self.stderr(), needle) < want && Instant::now() < deadline {
            sleep_ms(20);
        }
        std::thread::sleep(SETTLE);
        occurrences(&self.stderr(), needle)
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some((path, mode)) = self.restore.take() {
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(mode));
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Snapshots from the daemon's socket.
///
/// A raw `UnixStream` rather than `wisp_proto::client::connect`, which cannot
/// set a read timeout: a test that blocked forever on a daemon that had stopped
/// publishing would hang the suite instead of failing.
struct Client {
    reader: BufReader<UnixStream>,
}

impl Client {
    /// Connect, retrying until the daemon has bound its socket.
    fn connect(path: &Path) -> Client {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match UnixStream::connect(path) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
                    return Client { reader: BufReader::new(stream) };
                }
                Err(_) if Instant::now() < deadline => sleep_ms(20),
                Err(e) => panic!("cannot connect to {}: {e}", path.display()),
            }
        }
    }

    /// Snapshots until `want` accepts one, or [`TIMEOUT`] runs out.
    fn wait_for(&mut self, want: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let deadline = Instant::now() + TIMEOUT;
        let mut last: Option<Snapshot> = None;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                panic!("no snapshot matched within {TIMEOUT:?}; last was {last:?}");
            }
            self.reader.get_ref().set_read_timeout(Some(remaining)).unwrap();
            let snapshot = self.next();
            if want(&snapshot) {
                return snapshot;
            }
            last = Some(snapshot);
        }
    }

    fn next(&mut self) -> Snapshot {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => panic!("the daemon closed its socket"),
            Ok(_) => wisp_proto::decode(&line).expect("a decodable snapshot"),
            Err(e) => panic!("no snapshot within the read timeout: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Log files
// ---------------------------------------------------------------------------

/// One log line in the client's own shape. The timestamp is fabricated: the
/// daemon works in differences between log timestamps and never in wall clock,
/// so a test advances a session by writing a later timestamp rather than
/// waiting for one.
fn line(secs: i64, body: &str) -> String {
    let t = 20 * 3600 + secs;
    format!(
        "[Mon Aug 10 {:02}:{:02}:{:02} 2026] {body}\n",
        t / 3600,
        (t / 60) % 60,
        t % 60
    )
}

/// The player's own kill — the only line that moves `session_kills`.
fn kill(secs: i64) -> String {
    line(secs, "You have slain a rat!")
}

/// Append to a log the daemon is already holding open.
fn append(path: &Path, text: &str) {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

/// Create `name` in `dir` holding nothing, stamped `secs_ago` in the past so an
/// ordering is never an accident of creation order.
fn create_log(dir: &Path, name: &str, secs_ago: u64) -> PathBuf {
    let path = dir.join(name);
    fs::File::create(&path).unwrap();
    stamp(&path, secs_ago);
    path
}

fn stamp(path: &Path, secs_ago: u64) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(secs_ago))
        .unwrap();
}

/// Wait until the daemon holds `path` open, so the append that follows lands
/// after the tailer seeked to the end rather than before it.
///
/// `/proc/<pid>/fd` is the only signal that says this precisely: the daemon
/// prints nothing when it opens its first file, and sleeping a fixed number of
/// ticks would be a guess that a loaded machine could contradict.
fn wait_until_open(d: &Daemon, path: &Path) {
    let fds = format!("/proc/{}/fd", d.child.id());
    // A descriptor's link is the kernel's resolved path, so compare against
    // the same: `/tmp` is a symlink on some distributions.
    let want = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(entries) = fs::read_dir(&fds) {
            for entry in entries.flatten() {
                if fs::read_link(entry.path()).ok().as_deref() == Some(want.as_path()) {
                    return;
                }
            }
        }
        sleep_ms(20);
    }
    panic!("the daemon never opened {}", path.display());
}

fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

/// Wait for a file to exist and hold something, and return its bytes.
fn wait_for_bytes(path: &Path) -> Vec<u8> {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(bytes) = fs::read(path) {
            if !bytes.is_empty() {
                return bytes;
            }
        }
        sleep_ms(20);
    }
    panic!("{} never appeared", path.display());
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn a_directory_with_no_log_publishes_zero_counters_and_an_empty_ts() {
    let mut d = setup("no-log").spawn_logs_dir(&[]);
    let logs = d.logs.display().to_string();
    let mut client = Client::connect(&d.socket);

    // The game has not logged in yet, so there is nothing to read: the daemon
    // publishes anyway rather than waiting to have something to say.
    let snapshot = client.next();
    assert_eq!(snapshot.v, PROTOCOL_VERSION);
    assert_eq!((snapshot.lines_ingested, snapshot.session_kills), (0, 0));
    assert!(snapshot.ts.is_empty(), "ts was {:?}", snapshot.ts);
    assert_eq!(snapshot.log, None, "no file open, so no name to report");
    assert!(snapshot.timers.is_empty());
    assert!(snapshot.encounter.is_none());

    assert_eq!(
        d.count_stderr("waiting for a log", 1),
        1,
        "said once, not once per tick; stderr was:\n{}",
        d.stderr()
    );
    assert!(d.stderr().contains(&logs), "the message names the directory");
    assert!(d.alive(), "nothing to tail is not a reason to exit");
    assert!(
        client.wait_for(|s| s.seq > snapshot.seq).seq > snapshot.seq,
        "and it keeps publishing"
    );
}

#[test]
fn a_log_appearing_is_picked_up_without_a_restart() {
    let mut d = setup("appearing").spawn_logs_dir(&[]);
    let mut client = Client::connect(&d.socket);

    let before = client.next();
    assert_eq!((before.lines_ingested, before.session_kills), (0, 0));
    assert!(before.ts.is_empty());
    assert_eq!(d.count_stderr("waiting for a log", 1), 1);

    // The game logs in: the file appears empty, the daemon finds it on a later
    // tick, and only then is there anything to append.
    let log = create_log(&d.logs, "eqlog_Daggo_freeport.txt", 0);
    wait_until_open(&d, &log);
    append(&log, &(kill(1) + &kill(2)));

    let after = client.wait_for(|s| s.session_kills == 2);
    assert_eq!(after.lines_ingested, 2);
    assert_eq!(
        after.log.as_deref(),
        Some("eqlog_Daggo_freeport.txt"),
        "the daemon names the file it is tailing, not its path"
    );
    // `seq` is monotonic per process run, so a climb proves the daemon that
    // counted these lines is the one that said it was waiting.
    assert!(after.seq > before.seq, "no restart: seq went {} -> {}", before.seq, after.seq);
    assert!(d.alive());
    assert_eq!(
        d.count_stderr("newest log is now", 0),
        0,
        "the first file is an open, not a switch; stderr was:\n{}",
        d.stderr()
    );
}

#[test]
fn the_newest_of_two_files_is_tailed() {
    let mut d = setup("newest-of-two").spawn_logs_dir(&[]);
    let mut client = Client::connect(&d.socket);

    // Two logs, freeport the newer — the ordering the real install has.
    let older = create_log(&d.logs, "eqlog_Daggo_rivervale.txt", 100);
    let newer = create_log(&d.logs, "eqlog_Daggo_freeport.txt", 50);
    wait_until_open(&d, &newer);

    // One kill line in each, at different log times so the snapshot says which
    // file was read rather than only how many lines arrived.
    append(&older, &kill(1));
    // Writing to the older file made it the newest for a moment, so it is
    // stamped back before the daemon can act on it. Whichever way that race
    // falls the assertions below hold: had the daemon switched to the older
    // file it would have opened it at its end, after this line, and read
    // nothing.
    stamp(&older, 100);
    append(&newer, &kill(2));

    let snapshot = client.wait_for(|s| s.session_kills == 1);
    assert_eq!(snapshot.lines_ingested, 1, "only the newest file is tailed");
    assert!(snapshot.ts.contains("20:00:02"), "the newest file's line, ts was {:?}", snapshot.ts);
    assert_eq!(
        snapshot.log.as_deref(),
        Some("eqlog_Daggo_freeport.txt"),
        "the name proves it is the current source, not the first one seen"
    );
    assert!(d.alive());
}

#[test]
fn a_second_file_becoming_newest_switches_and_resets() {
    let mut d = setup("switch").spawn_logs_dir(&[]);
    let mut client = Client::connect(&d.socket);

    let first = create_log(&d.logs, "eqlog_Daggo_freeport.txt", 0);
    wait_until_open(&d, &first);
    append(&first, &(kill(1) + &kill(2)));
    let before = client.wait_for(|s| s.session_kills == 2);
    assert_eq!(before.lines_ingested, 2);

    // Another character logs in: a second file, newer, and the daemon follows
    // it without being restarted.
    let second = create_log(&d.logs, "eqlog_Other_rivervale.txt", 0);
    wait_until_open(&d, &second);
    append(&second, &kill(3));

    let after = client.wait_for(|s| s.lines_ingested == 1 && s.session_kills == 1);
    assert_eq!(
        (after.lines_ingested, after.session_kills),
        (1, 1),
        "the session was reset: the two kills from the first file are gone"
    );
    assert!(after.seq > before.seq, "the daemon was not restarted");
    assert!(d.alive(), "a switch is not fatal");
    assert_eq!(
        d.count_stderr("newest log is now", 1),
        1,
        "the switch is announced; stderr was:\n{}",
        d.stderr()
    );
    assert!(
        d.stderr().contains(&second.display().to_string()),
        "and the announcement names the new file; stderr was:\n{}",
        d.stderr()
    );
}

#[test]
fn the_player_name_is_rederived_on_a_switch() {
    // Asserted in its strong form: that a self-heal line naming the *new*
    // file's player counts as yours, and the same line naming the *old* one no
    // longer does. The weaker form — the stderr line naming the new path and
    // the counters resetting — is covered by
    // `a_second_file_becoming_newest_switches_and_resets`.
    let mut d = setup("player-name").spawn_logs_dir(&[]);
    let mut client = Client::connect(&d.socket);

    let first = create_log(&d.logs, "eqlog_Daggo_freeport.txt", 0);
    wait_until_open(&d, &first);

    let second = create_log(&d.logs, "eqlog_Other_rivervale.txt", 0);
    wait_until_open(&d, &second);

    // A fight has to be open for a heal to count inside it, and only damage
    // opens one.
    append(
        &second,
        &(line(0, "You kick a rat for 100 points of damage.")
            + &line(1, "Other healed himself for 50 hit points.")
            + &line(2, "Daggo healed himself for 7 hit points.")),
    );

    // All three lines consumed, and only Other's heal is yours: had the name
    // not been re-derived, Other's 50 would sit in the meter as somebody else
    // and Daggo's 7 would be booked as yours instead.
    let snapshot = client.wait_for(|s| s.lines_ingested == 3);
    let encounter = snapshot.encounter.expect("a fight is open");
    assert_eq!(
        encounter.you.healing, 50,
        "the player is now Other, read from the new filename; Daggo is not you any more"
    );
    assert!(d.alive());
}

#[test]
fn the_duration_store_on_disk_is_unchanged_across_a_switch() {
    let s = setup("durations");
    // There is no duration store at all without a spell table, so the table
    // has to come from somewhere. With WISP_EQL_DIR set it is the real client
    // install — 73,975 rows and 38 MB. Without it, a one-row table this test
    // writes itself, holding the same Mesmerization cap the real client has, so
    // the assertion runs in full in the ordinary gate instead of being a skip
    // that looks like a pass.
    let stub = s.root.join("spells");
    let spells = match std::env::var_os("WISP_EQL_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => {
            write_stub_spell_table(&stub);
            stub.clone()
        }
    };
    let mut d = s.spawn_logs_dir(&["--spells", spells.to_str().unwrap()]);
    if std::env::var_os("WISP_EQL_DIR").is_none() {
        println!(
            "note: the_duration_store_on_disk_is_unchanged_across_a_switch ran against a \
             one-row stub spell table; set WISP_EQL_DIR to run it against the real client"
        );
    }
    let mut client = Client::connect(&d.socket);

    // Three wear-offs of 40 s each, so the store has a measured median and is
    // written to disk. Log time is fabricated: the tracker never consults the
    // wall clock.
    let first = create_log(&d.logs, "eqlog_Daggo_freeport.txt", 0);
    wait_until_open(&d, &first);
    let mut cycles = String::new();
    for i in 0..3 {
        let base = i * 100;
        cycles += &line(base, "You begin casting Mesmerization VI.");
        cycles += &line(base + 3, "a rat has been mesmerized.");
        cycles += &line(base + 43, "Your Mesmerization spell has worn off of a rat.");
    }
    append(&first, &cycles);
    let before = wait_for_bytes(&d.durations);

    let second = create_log(&d.logs, "eqlog_Other_rivervale.txt", 0);
    wait_until_open(&d, &second);
    assert_eq!(
        d.count_stderr("newest log is now", 1),
        1,
        "the switch happened; stderr was:\n{}",
        d.stderr()
    );
    let after = fs::read(&d.durations).unwrap();
    assert_eq!(before, after, "the store on disk is byte-identical across the switch");

    // Still loaded in memory, too: a fresh cast after the switch is armed from
    // the samples recorded before it, which only a surviving table and store
    // can produce. Reparsing 38 MB on a poll tick would not have got here
    // within the timeout either.
    append(
        &second,
        &(line(0, "You begin casting Mesmerization VI.")
            + &line(3, "a rat has been mesmerized.")),
    );
    let snapshot = client.wait_for(|s| !s.timers.is_empty());
    let timer = &snapshot.timers[0];
    assert_eq!((timer.spell.as_str(), timer.rank), ("Mesmerization", 6));
    assert_eq!(timer.duration_ms, 40_000, "the measured median, not the 38 s seed");
    assert_eq!(timer.confidence, Confidence::Measured);
    assert!(d.alive());
}

#[test]
fn an_unreadable_directory_does_not_kill_the_daemon() {
    let mut d = setup("unreadable-dir").spawn_logs_dir(&[]);
    let mut client = Client::connect(&d.socket);

    let log = create_log(&d.logs, "eqlog_Daggo_freeport.txt", 0);
    wait_until_open(&d, &log);
    append(&log, &kill(1));
    let before = client.wait_for(|s| s.session_kills == 1);

    // An append handle opened while the directory was still searchable: mode
    // 000 removes the execute bit, so a path lookup inside it fails afterwards
    // even though an already-open descriptor keeps working — which is exactly
    // the position the daemon's tailer is in.
    let mut held = fs::OpenOptions::new().append(true).open(&log).unwrap();

    // A directory that exists but cannot be read. Restored in `Drop`, before
    // the tree is removed, or the cleanup itself fails.
    fs::set_permissions(&d.logs, fs::Permissions::from_mode(0o000)).unwrap();
    d.restore = Some((d.logs.clone(), 0o755));
    if fs::read_dir(&d.logs).is_ok() {
        println!(
            "note: an_unreadable_directory_does_not_kill_the_daemon skipped — mode 000 does \
             not exclude this process, which is what running as root looks like"
        );
        return;
    }

    let needle = format!("wispd: cannot read {}: ", d.logs.display());
    assert_eq!(
        d.count_stderr(&needle, 1),
        1,
        "reported once, not once per tick; stderr was:\n{}",
        d.stderr()
    );
    assert!(d.alive(), "a directory that cannot be read is not fatal");
    let later = client.wait_for(|s| s.seq > before.seq);
    assert!(later.seq > before.seq, "and the daemon keeps publishing");

    // A line written while the directory was unreadable. The tailer cannot see it
    // yet: `Tailer::poll` re-stats the log by path every tick, which is how it
    // notices truncation and replacement, and that lookup needs search
    // permission on the parent. What the rule requires is that the daemon
    // survive and keep publishing, not that it read through a closed directory.
    held.write_all(kill(2).as_bytes()).unwrap();
    assert_eq!(client.wait_for(|s| s.seq > later.seq).session_kills, 1);

    // Readable again, and the session resumes where it left off: the scan
    // succeeds, the newest file is still the one already open, so the line that
    // arrived during the outage is read rather than lost.
    fs::set_permissions(&d.logs, fs::Permissions::from_mode(0o755)).unwrap();
    d.restore = None;
    assert_eq!(
        client.wait_for(|s| s.session_kills == 2).session_kills,
        2,
        "the daemon recovers; stderr was:\n{}",
        d.stderr()
    );
    assert_eq!(
        d.count_stderr(&needle, 1),
        1,
        "recovery reported nothing new; stderr was:\n{}",
        d.stderr()
    );
}

#[test]
fn an_unreadable_config_is_reported_once_and_ignored() {
    let s = setup("unreadable-config");
    let config = s.config.join("wisp").join("config");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    // Not valid UTF-8, so `Config::load` returns an io::Error rather than a
    // config. The file is a system boundary and the daemon must not trust it.
    fs::write(&config, b"logs_dir = \xff\xfe/nowhere\n").unwrap();

    let mut d = s.spawn_logs_dir(&[]);
    let needle = format!("wispd: ignoring unreadable config {}: ", config.display());
    assert_eq!(
        d.count_stderr(&needle, 1),
        1,
        "reported once; stderr was:\n{}",
        d.stderr()
    );
    assert!(d.alive(), "an unreadable config is not fatal");

    // The flag still wins: the daemon discovers and tails the directory it was
    // given, and never acts on the unreadable file's `logs_dir`.
    let log = create_log(&d.logs, "eqlog_Daggo_freeport.txt", 0);
    wait_until_open(&d, &log);
    append(&log, &kill(1));
    let mut client = Client::connect(&d.socket);
    let snapshot = client.wait_for(|s| s.session_kills == 1);
    assert_eq!(snapshot.lines_ingested, 1);
}

#[test]
fn from_start_applies_only_to_the_first_file_the_process_opens() {
    // The one test that writes before the daemon starts, and it has to: the
    // question is what a switched-to file's *existing* content does, so that
    // file must already hold lines when the daemon reaches it. Nothing is
    // written after the switch but the final append, so no mtime can move under
    // the discovery rule while it is deciding.
    let s = setup("from-start");
    let first = s.logs.join("eqlog_Daggo_rivervale.txt");
    let second = s.logs.join("eqlog_Other_freeport.txt");

    // Both already hold kills. `rivervale` is the newest, so it is the file
    // `--from-start` is spent on; `freeport` is older for now.
    fs::File::create(&first).unwrap();
    append(&first, &kill(1));
    stamp(&first, 50);
    fs::File::create(&second).unwrap();
    append(&second, &(kill(2) + &kill(3) + &kill(4)));
    stamp(&second, 100);

    let mut d = s.spawn_logs_dir(&["--from-start"]);
    let mut client = Client::connect(&d.socket);
    let opened = client.wait_for(|s| s.lines_ingested == 1);
    assert_eq!(
        opened.session_kills, 1,
        "--from-start read the whole of the file it opened first"
    );

    // `freeport` becomes the newest, and is opened at its end: its three
    // existing lines are history, not this session.
    stamp(&second, 0);
    assert_eq!(
        d.count_stderr("newest log is now", 1),
        1,
        "stderr was:\n{}",
        d.stderr()
    );
    wait_until_open(&d, &second);
    let switched = client.wait_for(|s| s.lines_ingested == 0 && s.session_kills == 0);
    assert_eq!(
        (switched.lines_ingested, switched.session_kills),
        (0, 0),
        "a switched-to file is opened at its end, so --from-start cannot reach it"
    );

    append(&second, &kill(5));
    let after = client.wait_for(|s| s.session_kills == 1);
    assert_eq!(after.lines_ingested, 1, "only the line appended after the switch");
    assert!(d.alive());
}

/// A one-row spell table in the client's own format, written by hand: 173
/// `^`-separated fields with the five Wisp reads set, and the landing prose in
/// the strings file. `Mesmerization`'s cap of 4.0 ticks is the value the real
/// client has, so a rank-VI seed is 38 s either way and the same log lines work
/// against both. This is the trick `crates/wispd/src/spells.rs`'s own unit tests
/// use; no game data is copied and none is committed.
fn write_stub_spell_table(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let mut row: Vec<String> = vec!["0".to_string(); 173];
    row[0] = "307".to_string();
    row[1] = "Mesmerization".to_string();
    row[8] = "3000".to_string(); // cast time
    row[12] = "4".to_string(); // duration cap in ticks
    row[28] = "0".to_string(); // good_effect: detrimental
    fs::write(dir.join("spells_us.txt"), row.join("^") + "\n").unwrap();

    let mut prose: Vec<String> = vec![String::new(); 6];
    prose[0] = "307".to_string();
    prose[4] = " has been mesmerized.".to_string();
    let strings = format!(
        "#SPELLINDEX^CASTERMETXT^CASTEROTHERTXT^CASTEDMETXT^CASTEDOTHERTXT^SPELLGONE^\n{}^\n",
        prose.join("^")
    );
    fs::write(dir.join("spells_us_str.txt"), strings).unwrap();
}
