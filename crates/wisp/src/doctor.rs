// SPDX-License-Identifier: MIT
//! `wisp doctor`: what would happen, and why, without starting anything.
//!
//! One labelled line per item, in the spec's order, and every answer traced to
//! where it came from. Nothing here decides anything on its own: the log's
//! precedence is `wisp_config::source`'s, the log count is
//! `wisp_config::discover`'s, the backend names are `wisp_probe`'s and the
//! observations that led to one are `Detection::reason`'s. A doctor that
//! restated any of them could disagree with the daemon and the HUD it reports
//! on, which is worse than no doctor at all.

use std::path::Path;
use wisp_config::config::{Config, Key};
use wisp_config::discover::list_logs;
use wisp_config::paths::{config_path, socket_path};
use wisp_config::source::{resolve_log_source, resolve_spells_dir, LogSource};
use wisp_probe::{detect, BackendKind, Detection};

use crate::args::DoctorArgs;
use crate::labelled;

/// The scale the HUD renders at when neither the flag nor the config file says.
/// Printed by `Display`, so it reads as the `48` of the report and is the same
/// value as the HUD's `48.0`.
const DEFAULT_SCALE: f32 = 48.0;

/// Print the report and return the status `main` exits with: 1 when no log
/// resolves, 0 otherwise.
///
/// `args` are the five value flags a launch takes, so the report answers the
/// question anyone actually asks — what would *this* launch do — and not only
/// what a launch from the config file alone would do.
pub fn doctor(config: &Config, args: &DoctorArgs) -> i32 {
    println!("{}", labelled("version:", &format!("wisp {}", env!("CARGO_PKG_VERSION"))));

    match config_path() {
        Ok(path) => println!("{}", config_line(&path)),
        // No path is not a missing file: name the variable that is absent, and
        // offer no `wisp config set` that would have nowhere to write.
        Err(e) => println!("{}", labelled("config:", &format!("cannot resolve the path: {e}"))),
    }

    // The chain is called, never restated: the same function, with the same two
    // flags and the same config, that `wispd` calls.
    let source = resolve_log_source(args.log.clone(), args.logs_dir.clone(), config);
    let (log, resolved) = log_line(source.clone(), args);
    println!("{log}");
    println!("{}", spells_line(args, config, source.as_ref()));

    let socket = socket_path();
    let state = if crate::run::is_listening(&socket) {
        "a daemon is listening"
    } else {
        "nothing listening"
    };
    println!("{}", labelled("socket:", &format!("{} ({state})", socket.display())));

    println!("{}", scale_line(args.scale.as_deref(), config));
    println!("{}", backend_line(args.backend.as_deref(), config, &detect()));
    println!("{}", outputs_line());

    // The one condition the spec fixes: no log, no Wisp. A socket with nothing
    // on it is not a failure — starting the daemon is `wisp run`'s job, not this
    // report's.
    i32::from(!resolved)
}

/// The config file's path, and whether it is there at all.
fn config_line(path: &Path) -> String {
    let state = if path.exists() { "exists" } else { "missing" };
    labelled("config:", &format!("{} ({state})", path.display()))
}

/// The `log:` line, and whether it names a log to tail — which is the exit
/// status, the spec's one condition for failure.
///
/// Doctor stats what it prints and the log is no exception: a path that is not a
/// file there is a launch that fails, because `wispd` dies on the same config,
/// and a report that called it healthy would be the one place Wisp says
/// something is fine when it is not. The count comes from [`list_logs`] and the
/// origin from the source the daemon's own chain resolved, so this crate states
/// neither a precedence nor a discovery rule.
fn log_line(source: Option<LogSource>, args: &DoctorArgs) -> (String, bool) {
    match source {
        None => (labelled("log:", "none"), false),
        Some(LogSource::File(path)) => {
            let origin = log_origin(&LogSource::File(path.clone()), args);
            if path.is_file() {
                (labelled("log:", &format!("{} ({origin})", path.display())), true)
            } else {
                (labelled("log:", &format!("{} ({origin}, missing)", path.display())), false)
            }
        }
        Some(LogSource::Dir(dir)) => {
            let origin = log_origin(&LogSource::Dir(dir.clone()), args);
            match list_logs(&dir) {
                Ok(logs) => match logs.first() {
                    Some(newest) => (
                        labelled(
                            "log:",
                            &format!(
                                "{} ({origin}, newest of {} files)",
                                newest.display(),
                                logs.len()
                            ),
                        ),
                        true,
                    ),
                    // The count is printed even when it is zero, because the
                    // wrong `Logs` is the risk the spec names: the install has a
                    // second directory by that name and it holds no
                    // `eqlog_*.txt`, so a user can point at it and wait forever.
                    None => (
                        labelled(
                            "log:",
                            &format!("{} ({origin}, newest of 0 files)", dir.display()),
                        ),
                        false,
                    ),
                },
                // The daemon reports this once and keeps running; the report
                // says it and resolves no log.
                Err(e) => (
                    labelled(
                        "log:",
                        &format!("{} ({origin}, cannot read: {e})", dir.display()),
                    ),
                    false,
                ),
            }
        }
    }
}

/// Which of the two inputs named this source: the flag that carries it, or the
/// config key behind it.
///
/// A label and not a decision — the order that chose between them is
/// `wisp_config::source`'s and is called above. A path that both name is
/// reported as the flag's, because the flag is the one that won.
fn log_origin(source: &LogSource, args: &DoctorArgs) -> &'static str {
    match source {
        LogSource::File(path) => {
            if args.log.as_deref() == Some(path.as_path()) {
                "--log flag"
            } else {
                "config log"
            }
        }
        LogSource::Dir(path) => {
            if args.logs_dir.as_deref() == Some(path.as_path()) {
                "--logs-dir flag"
            } else {
                "config logs_dir"
            }
        }
    }
}

/// The `spells:` line: the directory the daemon would load `spells_us.txt`
/// from, where it came from, and whether the file is actually in it.
///
/// The directory is `resolve_spells_dir`'s answer — the same call `wispd` makes
/// — so neither the order nor the derivation from a log or a logs directory is
/// restated here. Only the label is.
fn spells_line(args: &DoctorArgs, config: &Config, source: Option<&LogSource>) -> String {
    let Some(dir) = resolve_spells_dir(args.spells.clone(), config, source) else {
        // The daemon still runs and still counts kills; it just publishes no
        // timers, which is what this says.
        return labelled("spells:", "none (timers disabled)");
    };
    let origin = spells_origin(&dir, args, config, source);
    let state = if dir.join("spells_us.txt").is_file() {
        "spells_us.txt present"
    } else {
        "spells_us.txt missing"
    };
    labelled("spells:", &format!("{} ({origin}, {state})", dir.display()))
}

/// Which of the three inputs named this directory. A label and not a decision,
/// exactly as [`log_origin`].
fn spells_origin(
    dir: &Path,
    args: &DoctorArgs,
    config: &Config,
    source: Option<&LogSource>,
) -> &'static str {
    if args.spells.as_deref() == Some(dir) {
        return "--spells flag";
    }
    if config.path_value(Key::SpellsDir).as_deref() == Some(dir) {
        return "config spells_dir";
    }
    match source {
        Some(LogSource::Dir(_)) => "derived from logs_dir",
        Some(LogSource::File(_)) => "derived from log",
        // Nothing to derive from, so `resolve_spells_dir` would have answered
        // `None` and this line would not have been reached.
        None => "derived",
    }
}

/// The `scale:` line — the effective value and where it came from.
///
/// The HUD's precedence, restated because the HUD is a binary crate and cannot
/// export it. An addition to the spec's list, which enumerates version, config,
/// log, spells, socket and backend: a doctor that reported one of the HUD's two
/// settings and not the other would be half a diagnostic.
fn scale_line(flag: Option<&str>, config: &Config) -> String {
    let Some((text, origin)) = resolve(flag, config.get(Key::Scale)) else {
        return labelled("scale:", &format!("{DEFAULT_SCALE} (default)"));
    };
    // Printed as the user wrote it: this is a report, and a value the HUD will
    // refuse has to be recognisable as the thing they typed.
    let state = match text.parse::<f32>() {
        Ok(_) => origin.label("scale"),
        Err(_) => format!("{}, which wisp-hud refuses at start", origin.label("scale")),
    };
    labelled("scale:", &format!("{text} ({state})"))
}

/// The `backend:` line — the backend the HUD will actually use, which is its
/// precedence and not detection alone.
///
/// With nothing set this is detection and the observations behind it. With a flag
/// or a config key set it names the winner and its origin *and* still reports
/// what detection would have chosen, so a surprise is diagnosable from the one
/// line. The name is read by [`BackendKind::parse`] and the observations come
/// from [`Detection::reason`]: this crate spells no backend name and states no
/// detection rule, not even in text it assembles itself.
fn backend_line(flag: Option<&str>, config: &Config, detection: &Detection) -> String {
    let reason = detection.reason();
    let Some((name, origin)) = resolve(flag, config.get(Key::Backend)) else {
        return labelled("backend:", &format!("{:?} ({reason})", detection.kind));
    };
    let detected = format!("detection would choose {:?}: {reason}", detection.kind);
    match BackendKind::parse(&name) {
        Some(kind) => labelled(
            "backend:",
            &format!("{kind:?} ({} = {name}; {detected})", origin.source("backend")),
        ),
        // The refusal the HUD would make, and still the detection that would
        // have applied had nothing been written down.
        None => labelled(
            "backend:",
            &format!(
                "none ({} = {name}, which wisp-hud refuses at start; {detected})",
                origin.source("backend")
            ),
        ),
    }
}

/// The `outputs:` line: the Wayland outputs the HUD could draw on, or the
/// reason there are none to name. `wisp_probe::outputs` opens no surface, so
/// this is safe to call from a report that starts nothing.
fn outputs_line() -> String {
    let outputs = wisp_probe::outputs();
    if outputs.is_empty() {
        labelled("outputs:", "none (no Wayland display)")
    } else {
        labelled("outputs:", &outputs.join(", "))
    }
}

/// Where a startup value was written down. The HUD's own two origins, restated
/// with it: a value from the file is fixed in a different place from a value
/// typed on a command line, and a report that did not say which would send the
/// user to the wrong one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Flag,
    Config,
}

impl Origin {
    /// `--backend`, `config backend`: the place, for a line that quotes the
    /// value after it.
    fn source(self, key: &str) -> String {
        match self {
            Origin::Flag => format!("--{key}"),
            Origin::Config => format!("config {key}"),
        }
    }

    /// `--scale flag`, `config scale`: the place, for a line that printed the
    /// value before the parenthesis and so has nothing to quote.
    fn label(self, key: &str) -> String {
        match self {
            Origin::Flag => format!("--{key} flag"),
            Origin::Config => self.source(key),
        }
    }
}

/// A flag beats the file; the file beats the default, which each reader owns.
/// The HUD's own `resolve`, restated for the same reason `compact` and `roman`
/// are restated in `status`: a binary crate cannot export one.
fn resolve(flag: Option<&str>, file: Option<&str>) -> Option<(String, Origin)> {
    match (flag, file) {
        (Some(value), _) => Some((value.to_string(), Origin::Flag)),
        (None, Some(value)) => Some((value.to_string(), Origin::Config)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use wisp_config::config::Config;
    use wisp_config::source::LogSource;
    use wisp_probe::{BackendKind, Detection};

    /// A scratch directory of its own per test, removed when the test ends
    /// whether it passed or panicked.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let path = std::env::temp_dir().join(format!("wisp-doctor-{}-{tag}", std::process::id()));
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

    fn set_mtime(path: &Path, secs_ago: u64) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(secs_ago)).unwrap();
    }

    /// A detection that chose the layer shell, with both observations in it.
    ///
    /// What `reason()` says about those observations is `wisp-probe`'s to fix,
    /// and its own five tests do it. These assert that the report embeds it, not
    /// what it spells, so no detection rule is written down in this crate -- not
    /// even in a test's expectation.
    fn layer_shell_detection() -> Detection {
        Detection {
            kind: BackendKind::WlrLayerShell,
            display: Some(":0".to_string()),
            x_connected: true,
            gamescope_root: false,
            wayland_connected: true,
            layer_shell: true,
        }
    }

    /// The three backend names are `wisp_probe::BackendKind::parse`'s, so this
    /// crate never spells one — not even in its tests. The name a test needs is
    /// read back out of a config file's text, which is where a user writes it.
    const CONFIG_WITH_A_BACKEND: &str = "backend = plain\n";

    fn configured_backend_name() -> String {
        Config::parse(CONFIG_WITH_A_BACKEND)
            .get(wisp_config::config::Key::Backend)
            .unwrap()
            .to_string()
    }

    #[test]
    fn doctor_names_the_source_of_the_log() {
        // A file the config named, and that is there: nothing to discover, so
        // nothing to count.
        let scratch = Scratch::new("log-line-file");
        let log = scratch.join("eqlog_Daggo_freeport.txt");
        fs::write(&log, b"").unwrap();
        let (line, resolved) = log_line(Some(LogSource::File(log.clone())), &DoctorArgs::default());
        assert_eq!(line, format!("log:       {} (config log)", log.display()));
        assert!(resolved);
        // The same file named by the flag blames the flag, because that is the
        // input that won.
        let flagged = DoctorArgs { log: Some(log.clone()), ..DoctorArgs::default() };
        let from_flag = format!("log:       {} (--log flag)", log.display());
        let (line, resolved) = log_line(Some(LogSource::File(log)), &flagged);
        assert_eq!(line, from_flag);
        assert!(resolved);

        // A directory: the file the daemon would open, and how many the
        // directory holds — counted by `wisp_config::discover::list_logs`, not
        // by a rule of this crate's own.
        let scratch = Scratch::new("log-line");
        let older = scratch.join("eqlog_Daggo_rivervale.txt");
        let newer = scratch.join("eqlog_Daggo_freeport.txt");
        fs::write(&older, b"").unwrap();
        fs::write(&newer, b"").unwrap();
        set_mtime(&older, 60);
        set_mtime(&newer, 0);
        let dir = scratch.path().to_path_buf();
        let (line, resolved) = log_line(Some(LogSource::Dir(dir.clone())), &DoctorArgs::default());
        assert_eq!(
            line,
            format!("log:       {} (config logs_dir, newest of 2 files)", newer.display())
        );
        assert!(resolved);
        let flagged = DoctorArgs { logs_dir: Some(dir.clone()), ..DoctorArgs::default() };
        let (line, _) = log_line(Some(LogSource::Dir(dir)), &flagged);
        assert_eq!(
            line,
            format!("log:       {} (--logs-dir flag, newest of 2 files)", newer.display())
        );

        // A directory holding none: the wrong `Logs`, which is the risk the
        // spec names and the reason doctor prints the count at all.
        let empty = Scratch::new("log-line-empty");
        let (line, resolved) =
            log_line(Some(LogSource::Dir(empty.path().to_path_buf())), &DoctorArgs::default());
        assert_eq!(
            line,
            format!("log:       {} (config logs_dir, newest of 0 files)", empty.path().display())
        );
        assert!(!resolved, "a directory with no log in it names no log to tail");

        // Nothing set anywhere.
        assert_eq!(
            log_line(None, &DoctorArgs::default()),
            ("log:       none".to_string(), false)
        );
    }

    #[test]
    fn doctor_reports_a_missing_log_file_and_exits_1() {
        // Stat-ed like everything else on this list. A log that is not there is
        // a launch that fails — `wispd` dies on the same config — so doctor must
        // not report the setup as healthy.
        let scratch = Scratch::new("log-line-missing");
        let missing = scratch.join("eqlog_Daggo_freeport.txt");
        assert!(!missing.exists(), "the test is about a path with nothing at it");

        let (line, resolved) =
            log_line(Some(LogSource::File(missing.clone())), &DoctorArgs::default());
        assert_eq!(line, format!("log:       {} (config log, missing)", missing.display()));
        assert!(!resolved, "a path with no file at it resolves no log");

        // Named by the flag instead, the origin says so and the answer is the
        // same failure.
        let flagged = DoctorArgs { log: Some(missing.clone()), ..DoctorArgs::default() };
        let (line, resolved) = log_line(Some(LogSource::File(missing.clone())), &flagged);
        assert_eq!(line, format!("log:       {} (--log flag, missing)", missing.display()));
        assert!(!resolved);

        // And a directory in the way of the file is no log to tail either.
        fs::create_dir_all(&missing).unwrap();
        let (_, resolved) = log_line(Some(LogSource::File(missing)), &DoctorArgs::default());
        assert!(!resolved);
    }

    #[test]
    fn doctor_reports_a_missing_config_file() {
        let scratch = Scratch::new("config-line");
        let path = scratch.join("config");
        assert_eq!(config_line(&path), format!("config:    {} (missing)", path.display()));
        fs::write(&path, "log = /a\n").unwrap();
        assert_eq!(config_line(&path), format!("config:    {} (exists)", path.display()));
    }

    #[test]
    fn doctor_reports_the_backend_the_hud_will_actually_use() {
        let detection = layer_shell_detection();
        let reason = detection.reason();
        let name = configured_backend_name();

        // The file said something, so the file's answer is what the HUD will
        // act on — and what detection would have chosen is still reported, so a
        // surprise is diagnosable from this one line.
        let config = Config::parse(CONFIG_WITH_A_BACKEND);
        assert_eq!(
            backend_line(None, &config, &detection),
            format!("backend:   PlainWindow (config backend = {name}; detection would choose WlrLayerShell: {reason})")
        );

        // The same value arriving as a flag wins over the file, as it does in
        // the HUD.
        assert_eq!(
            backend_line(Some(&name), &Config::default(), &detection),
            format!("backend:   PlainWindow (--backend = {name}; detection would choose WlrLayerShell: {reason})")
        );

        // Neither said: detection alone, and no origin to name.
        assert_eq!(
            backend_line(None, &Config::parse("# no backend here\n"), &detection),
            format!("backend:   WlrLayerShell ({reason})")
        );
    }

    #[test]
    fn doctor_refuses_an_unparseable_config_backend_as_the_hud_does() {
        // `BackendKind::parse` is the only reader of the name, here exactly as
        // in the HUD: a name it does not know is not guessed at.
        let detection = layer_shell_detection();
        let reason = detection.reason();
        let config = Config::parse("backend = nonsense\n");
        assert_eq!(
            backend_line(None, &config, &detection),
            format!(
                "backend:   none (config backend = nonsense, which wisp-hud refuses at start; \
                 detection would choose WlrLayerShell: {reason})"
            )
        );
        // The flag form says the flag, since that is where the user wrote it.
        assert_eq!(
            backend_line(Some("nonsense"), &Config::default(), &detection),
            format!(
                "backend:   none (--backend = nonsense, which wisp-hud refuses at start; \
                 detection would choose WlrLayerShell: {reason})"
            )
        );
    }

    #[test]
    fn doctor_reports_the_effective_scale_and_its_origin() {
        assert_eq!(scale_line(None, &Config::parse("# nothing in here\n")), "scale:     48 (default)");
        // `[hud] scale` (Spec 5's own key) is the multiplier as written, with
        // no Spec-4-pixel conversion — a bare top-level `scale` would be read
        // as that legacy pixel value and converted (see `wisp_config::config`'s
        // own tests), which is not what this test is about.
        assert_eq!(
            scale_line(None, &Config::parse("[hud]\nscale = 32\n")),
            "scale:     32 (config scale)"
        );
        assert_eq!(
            scale_line(Some("16"), &Config::parse("[hud]\nscale = 32\n")),
            "scale:     16 (--scale flag)"
        );
        // A value the HUD would refuse is printed as the user wrote it rather
        // than dressed up as the default: doctor reports, it does not repair.
        assert_eq!(
            scale_line(None, &Config::parse("scale = not-a-number\n")),
            "scale:     not-a-number (config scale, which wisp-hud refuses at start)"
        );
    }
}
