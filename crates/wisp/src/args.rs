// SPDX-License-Identifier: MIT
//! The command line, as data.
//!
//! Hand-rolled like the other two binaries: `args_os`, no clap. Pure and with
//! no I/O, so every rule — which flags exist, which take a value, where `--`
//! falls — is unit-testable without spawning anything.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use wisp_config::config::Key;

/// What `wisp` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run(RunArgs),
    Status { json: bool },
    Doctor(DoctorArgs),
    Config(ConfigCommand),
    Version,
    /// Usage on stderr and exit 2. Bare `wisp` lands here rather than meaning
    /// `run`: the desktop entry and the AppImage's `AppRun` say `run`
    /// explicitly, so a bare `wisp` on `PATH` is a mistake and not a launch.
    Usage,
}

/// What `wisp config` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigCommand {
    Path,
    Show,
    Set(Key, String),
    /// A `set` whose key is not one of the five.
    ///
    /// Carried rather than folded into [`Command::Usage`] because the answer is
    /// not a usage dump: the user typed a real command and got one word of it
    /// wrong, so the message names that word and lists the five keys. `Key`
    /// cannot hold it, which is the point — an unknown key is not a key.
    SetUnknown(String),
}

/// What `wisp run` starts, and what it wraps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunArgs {
    /// Forwarded to `wispd` verbatim.
    pub wispd: Vec<OsString>,
    /// Forwarded to `wisp-hud` verbatim.
    pub hud: Vec<OsString>,
    /// Everything after `--`: run as a child, waited for, and the source of the
    /// exit status.
    pub command: Option<Vec<OsString>>,
}

/// The five value flags a launch takes, as `wisp doctor` reads them.
///
/// Doctor answers "what would this launch do", so it takes the same inputs and
/// no others. `--from-start` and `--stub` change nothing it reports, and a
/// command after `--` is something it would have to start, which it never does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorArgs {
    pub log: Option<PathBuf>,
    pub logs_dir: Option<PathBuf>,
    pub spells: Option<PathBuf>,
    pub scale: Option<String>,
    pub backend: Option<String>,
}

/// Which of the two children takes a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Wispd,
    Hud,
}

/// One row of the flag table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flag {
    name: &'static str,
    target: Target,
    /// The argument after this flag is its value. The two rows that are not are
    /// presence flags, which `wispd` reads with `iter().any(…)`: they are
    /// forwarded bare and swallow nothing.
    takes_value: bool,
}

/// The seven flags the spec lists — five for the daemon, two for the HUD. An
/// eighth is usage and exit 2: `wisp run` passes nothing along that the user did
/// not give it, and does not guess at what they meant.
const FLAGS: [Flag; 7] = [
    Flag { name: "--log", target: Target::Wispd, takes_value: true },
    Flag { name: "--logs-dir", target: Target::Wispd, takes_value: true },
    Flag { name: "--spells", target: Target::Wispd, takes_value: true },
    Flag { name: "--from-start", target: Target::Wispd, takes_value: false },
    Flag { name: "--stub", target: Target::Wispd, takes_value: false },
    Flag { name: "--scale", target: Target::Hud, takes_value: true },
    Flag { name: "--backend", target: Target::Hud, takes_value: true },
];

/// `argv` as a command. `argv[0]` is the program's own name, as `args_os` hands
/// it over, and is never itself a command — so bare `wisp`, with no `argv[1]`,
/// is usage and not `run`.
pub fn parse(argv: &[OsString]) -> Command {
    let Some(name) = argv.get(1) else {
        return Command::Usage;
    };
    let args = &argv[2..];
    // A command name that is not text is not a command Wisp has. The arguments
    // after one stay `OsString` all the way to the child that receives them: a
    // log path is arbitrary bytes on Linux.
    match name.to_str() {
        Some("run") => run_command(args),
        Some("status") => match args {
            [] => Command::Status { json: false },
            [flag] if flag == OsStr::new("--json") => Command::Status { json: true },
            _ => Command::Usage,
        },
        Some("doctor") => doctor_command(args),
        Some("config") => config_command(args),
        Some("version") | Some("--version") if args.is_empty() => Command::Version,
        _ => Command::Usage,
    }
}

fn run_command(args: &[OsString]) -> Command {
    let (flags, command) = split_at_double_dash(args);
    match partition_run_flags(flags) {
        // A `--` with nothing after it is no command at all: `wisp run --stub --`
        // starts Wisp and waits, exactly as it would without the separator.
        Ok((wispd, hud)) => Command::Run(RunArgs {
            wispd,
            hud,
            command: (!command.is_empty()).then(|| command.to_vec()),
        }),
        // `partition_run_flags` named the offender; this module does no I/O, so
        // naming it to the user is `main`'s, which prints the usage listing all
        // seven flags and exits 2.
        Err(_) => Command::Usage,
    }
}

fn doctor_command(args: &[OsString]) -> Command {
    // The five value flags and nothing else. `scan_flags` refuses a presence
    // flag here exactly as it refuses one `run` does not have, and a `--` is not
    // in the table, so it is refused too.
    let scanned = match scan_flags(args, |flag| flag.takes_value) {
        Ok(scanned) => scanned,
        Err(_) => return Command::Usage,
    };
    let mut parsed = DoctorArgs::default();
    for (flag, value) in scanned {
        // A flag given twice: the first wins, which is what `wispd` and
        // `wisp-hud` do with `position(…)`, so the report matches the launch.
        let value = value.expect("a value flag carries its value");
        match flag.name {
            "--log" => {
                parsed.log.get_or_insert(PathBuf::from(value));
            }
            "--logs-dir" => {
                parsed.logs_dir.get_or_insert(PathBuf::from(value));
            }
            "--spells" => {
                parsed.spells.get_or_insert(PathBuf::from(value));
            }
            "--scale" => {
                parsed.scale.get_or_insert(text_of(value));
            }
            "--backend" => {
                parsed.backend.get_or_insert(text_of(value));
            }
            // The two rows left are presence flags, which `scan_flags` refused.
            _ => unreachable!("every value flag in the table is a DoctorArgs field"),
        };
    }
    Command::Doctor(parsed)
}

fn config_command(args: &[OsString]) -> Command {
    let Some(sub) = args.first().and_then(|arg| arg.to_str()) else {
        return Command::Usage;
    };
    match (sub, &args[1..]) {
        ("path", []) => Command::Config(ConfigCommand::Path),
        ("show", []) => Command::Config(ConfigCommand::Show),
        ("set", [name, value]) => match name.to_str().and_then(Key::parse) {
            // Lossy because the config file is UTF-8 text and `set_in_text`
            // takes a `&str`: a value that is not text has no spelling in the
            // file to be written to.
            Some(key) => Command::Config(ConfigCommand::Set(key, text_of(value))),
            None => Command::Config(ConfigCommand::SetUnknown(text_of(name))),
        },
        _ => Command::Usage,
    }
}

/// Everything before the first `--`, and everything after it.
///
/// The separator belongs to neither side. Without one, all of `args` is flags
/// and there is no command.
pub fn split_at_double_dash(args: &[OsString]) -> (&[OsString], &[OsString]) {
    match args.iter().position(|arg| arg == OsStr::new("--")) {
        Some(at) => (&args[..at], &args[at + 1..]),
        None => (args, &[]),
    }
}

/// The flags of a `run`, split between the two children that take them.
///
/// `Err` carries the offending argument, which is either not one of the seven or
/// is one of the five that need a value and has none. Nothing is silently
/// dropped and nothing is forwarded half-understood.
pub fn partition_run_flags(
    args: &[OsString],
) -> Result<(Vec<OsString>, Vec<OsString>), OsString> {
    let mut wispd: Vec<OsString> = Vec::new();
    let mut hud: Vec<OsString> = Vec::new();
    for (flag, value) in scan_flags(args, |_| true)? {
        let list = match flag.target {
            Target::Wispd => &mut wispd,
            Target::Hud => &mut hud,
        };
        // The table's own spelling, which the argument matched byte for byte.
        list.push(OsString::from(flag.name));
        if let Some(value) = value {
            list.push(value.clone());
        }
    }
    Ok((wispd, hud))
}

/// The flags in `args`, each with the value that follows it, or the argument
/// that offended.
///
/// One rule, shared by `run` and `doctor` so it cannot be stated twice and drift:
/// a flag `accepts` refuses is the offender, and so is one of the five value
/// flags with no value after it — where "no value" covers both the end of the
/// list and a next argument beginning with `--`, since forwarding `--stub` as
/// `--log`'s value would hand the daemon a file named `--stub` and the HUD a
/// backend named `--backend`. A single leading dash is not a flag: `-relative`
/// is a relative path and stays a value.
fn scan_flags(
    args: &[OsString],
    accepts: impl Fn(Flag) -> bool,
) -> Result<Vec<(Flag, Option<&OsString>)>, OsString> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < args.len() {
        let arg = &args[at];
        let Some(&flag) = FLAGS.iter().find(|flag| arg == OsStr::new(flag.name)) else {
            return Err(arg.clone());
        };
        if !accepts(flag) {
            return Err(arg.clone());
        }
        if flag.takes_value {
            let Some(value) = args.get(at + 1) else {
                return Err(arg.clone());
            };
            if value.as_bytes().starts_with(b"--") {
                return Err(arg.clone());
            }
            found.push((flag, Some(value)));
            at += 1;
        } else {
            found.push((flag, None));
        }
        at += 1;
    }
    Ok(found)
}

/// An argument as text, lossily: `--scale` and `--backend` are read as text by
/// whoever parses them, and a value that is not UTF-8 is still shown as what
/// survives of it rather than vanishing.
fn text_of(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    fn argv(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    /// A stand-in value for `--backend`, and deliberately not one of the three
    /// names a backend has. This module partitions flags and never reads a
    /// value, and the three names are `wisp_probe::BackendKind::parse`'s to
    /// match against text -- so this crate spells none of them, not even here,
    /// and a placeholder makes that visible rather than accidental.
    const A_BACKEND: &str = "a-backend-name";

    #[test]
    fn bare_wisp_is_usage() {
        // Bare `wisp` on PATH does not mean `run`: the desktop entry and the
        // AppImage's AppRun say `run` explicitly.
        assert_eq!(parse(&argv(&["wisp"])), Command::Usage);
        assert_eq!(parse(&[]), Command::Usage, "no argv[0] either");
    }

    #[test]
    fn an_unknown_command_is_usage() {
        assert_eq!(parse(&argv(&["wisp", "nonsense"])), Command::Usage);
        assert_eq!(parse(&argv(&["wisp", "Run"])), Command::Usage, "commands are case-sensitive");
        assert_eq!(parse(&argv(&["wisp", "--nonsense"])), Command::Usage);
    }

    #[test]
    fn version_in_both_forms() {
        assert_eq!(parse(&argv(&["wisp", "version"])), Command::Version);
        assert_eq!(parse(&argv(&["wisp", "--version"])), Command::Version);
        assert_eq!(parse(&argv(&["wisp", "--version", "extra"])), Command::Usage);
    }

    #[test]
    fn status_takes_an_optional_json_flag() {
        assert_eq!(parse(&argv(&["wisp", "status"])), Command::Status { json: false });
        assert_eq!(parse(&argv(&["wisp", "status", "--json"])), Command::Status { json: true });
        assert_eq!(parse(&argv(&["wisp", "status", "--nonsense"])), Command::Usage);
    }

    #[test]
    fn doctor_takes_the_five_value_flags_and_nothing_else() {
        // Each of the five on its own.
        assert_eq!(
            parse(&argv(&["wisp", "doctor", "--log", "/a/eqlog_x.txt"])),
            Command::Doctor(DoctorArgs {
                log: Some(PathBuf::from("/a/eqlog_x.txt")),
                ..DoctorArgs::default()
            })
        );
        assert_eq!(
            parse(&argv(&["wisp", "doctor", "--logs-dir", "/a/Logs"])),
            Command::Doctor(DoctorArgs { logs_dir: Some(PathBuf::from("/a/Logs")), ..DoctorArgs::default() })
        );
        assert_eq!(
            parse(&argv(&["wisp", "doctor", "--spells", "/a/install"])),
            Command::Doctor(DoctorArgs { spells: Some(PathBuf::from("/a/install")), ..DoctorArgs::default() })
        );
        assert_eq!(
            parse(&argv(&["wisp", "doctor", "--scale", "32"])),
            Command::Doctor(DoctorArgs { scale: Some("32".to_string()), ..DoctorArgs::default() })
        );
        assert_eq!(
            parse(&argv(&["wisp", "doctor", "--backend", A_BACKEND])),
            Command::Doctor(DoctorArgs { backend: Some(A_BACKEND.to_string()), ..DoctorArgs::default() })
        );

        // All five at once, which is the launch a user actually asks about.
        assert_eq!(
            parse(&argv(&[
                "wisp", "doctor",
                "--log", "/a/eqlog_x.txt",
                "--logs-dir", "/a/Logs",
                "--spells", "/a/install",
                "--scale", "32",
                "--backend", A_BACKEND,
            ])),
            Command::Doctor(DoctorArgs {
                log: Some(PathBuf::from("/a/eqlog_x.txt")),
                logs_dir: Some(PathBuf::from("/a/Logs")),
                spells: Some(PathBuf::from("/a/install")),
                scale: Some("32".to_string()),
                backend: Some(A_BACKEND.to_string()),
            })
        );
        // Bare `doctor` is the report about the config file alone.
        assert_eq!(parse(&argv(&["wisp", "doctor"])), Command::Doctor(DoctorArgs::default()));

        // Neither presence flag changes anything doctor reports, a `--` is a
        // command it would have to start, and `--json` belongs to `status`: all
        // of them usage plus exit 2.
        assert_eq!(parse(&argv(&["wisp", "doctor", "--from-start"])), Command::Usage);
        assert_eq!(parse(&argv(&["wisp", "doctor", "--stub"])), Command::Usage);
        assert_eq!(parse(&argv(&["wisp", "doctor", "--"])), Command::Usage);
        assert_eq!(
            parse(&argv(&["wisp", "doctor", "--log", "/a/x", "--", "sleep", "1"])),
            Command::Usage
        );
        assert_eq!(parse(&argv(&["wisp", "doctor", "--json"])), Command::Usage);
        // A value flag with no value is the refusal `run` makes, by the same rule.
        assert_eq!(parse(&argv(&["wisp", "doctor", "--log"])), Command::Usage);
        assert_eq!(parse(&argv(&["wisp", "doctor", "--scale", "--backend"])), Command::Usage);
        // A flag given twice reports the first, which is what the child reads.
        assert_eq!(
            parse(&argv(&["wisp", "doctor", "--scale", "32", "--scale", "16"])),
            Command::Doctor(DoctorArgs { scale: Some("32".to_string()), ..DoctorArgs::default() })
        );
    }

    #[test]
    fn config_path_show_and_set_parse() {
        assert_eq!(parse(&argv(&["wisp", "config", "path"])), Command::Config(ConfigCommand::Path));
        assert_eq!(parse(&argv(&["wisp", "config", "show"])), Command::Config(ConfigCommand::Show));
        assert_eq!(
            parse(&argv(&["wisp", "config", "set", "logs_dir", "/a path/Logs"])),
            Command::Config(ConfigCommand::Set(Key::LogsDir, "/a path/Logs".to_string()))
        );
        // Every key spells itself the way the file spells it.
        for key in Key::ALL {
            assert_eq!(
                parse(&argv(&["wisp", "config", "set", key.name(), "v"])),
                Command::Config(ConfigCommand::Set(key, "v".to_string())),
                "{}",
                key.name()
            );
        }
    }

    #[test]
    fn config_set_needs_two_arguments() {
        assert_eq!(parse(&argv(&["wisp", "config"])), Command::Usage);
        assert_eq!(parse(&argv(&["wisp", "config", "nonsense"])), Command::Usage);
        assert_eq!(parse(&argv(&["wisp", "config", "set"])), Command::Usage);
        assert_eq!(parse(&argv(&["wisp", "config", "set", "logs_dir"])), Command::Usage);
        assert_eq!(parse(&argv(&["wisp", "config", "set", "logs_dir", "/a", "/b"])), Command::Usage);
    }

    #[test]
    fn config_set_refuses_an_unknown_key() {
        // Named rather than buried in a usage dump: the user typed a real
        // command and got one word of it wrong.
        assert_eq!(
            parse(&argv(&["wisp", "config", "set", "nope", "x"])),
            Command::Config(ConfigCommand::SetUnknown("nope".to_string()))
        );
        assert_eq!(
            parse(&argv(&["wisp", "config", "set", "LOG", "x"])),
            Command::Config(ConfigCommand::SetUnknown("LOG".to_string())),
            "keys are case-sensitive"
        );
    }

    #[test]
    fn split_at_double_dash_partitions_the_command() {
        let args = argv(&["--stub", "--backend", A_BACKEND, "--", "sleep", "1"]);
        let (flags, command) = split_at_double_dash(&args);
        assert_eq!(flags, argv(&["--stub", "--backend", A_BACKEND]));
        assert_eq!(command, argv(&["sleep", "1"]));
        // Everything after the first `--` is the command, `--` included.
        let args = argv(&["--", "echo", "--", "x"]);
        let (flags, command) = split_at_double_dash(&args);
        assert!(flags.is_empty());
        assert_eq!(command, argv(&["echo", "--", "x"]));
        // No `--` at all: all flags and no command.
        let args = argv(&["--stub"]);
        let (flags, command) = split_at_double_dash(&args);
        assert_eq!(flags, argv(&["--stub"]));
        assert!(command.is_empty());
    }

    #[test]
    fn a_double_dash_with_nothing_after_it_is_no_command() {
        assert_eq!(
            parse(&argv(&["wisp", "run", "--stub", "--"])),
            Command::Run(RunArgs { wispd: argv(&["--stub"]), hud: Vec::new(), command: None })
        );
    }

    #[test]
    fn every_wispd_flag_lands_in_the_wispd_list() {
        let args = argv(&["--log", "/a", "--logs-dir", "/b", "--spells", "/c", "--from-start", "--stub"]);
        let (wispd, hud) = partition_run_flags(&args).unwrap();
        assert_eq!(wispd, args);
        assert!(hud.is_empty());
    }

    #[test]
    fn every_hud_flag_lands_in_the_hud_list() {
        let args = argv(&["--scale", "32", "--backend", A_BACKEND]);
        let (wispd, hud) = partition_run_flags(&args).unwrap();
        assert_eq!(hud, args);
        assert!(wispd.is_empty());
    }

    #[test]
    fn stub_goes_to_wispd() {
        let (wispd, hud) = partition_run_flags(&argv(&["--stub", "--backend", A_BACKEND])).unwrap();
        assert_eq!(wispd, argv(&["--stub"]));
        assert_eq!(hud, argv(&["--backend", A_BACKEND]));
        // The same split through `parse`, command included.
        assert_eq!(
            parse(&argv(&["wisp", "run", "--stub", "--backend", A_BACKEND, "--", "sleep", "1"])),
            Command::Run(RunArgs {
                wispd: argv(&["--stub"]),
                hud: argv(&["--backend", A_BACKEND]),
                command: Some(argv(&["sleep", "1"])),
            })
        );
    }

    #[test]
    fn an_unrecognised_flag_is_an_error_naming_it() {
        assert_eq!(partition_run_flags(&argv(&["--nonsense"])), Err(OsString::from("--nonsense")));
        assert_eq!(
            partition_run_flags(&argv(&["--stub", "sleep"])),
            Err(OsString::from("sleep")),
            "a bare word is not a flag; the command belongs after --"
        );
        assert_eq!(
            partition_run_flags(&argv(&["--scale", "32", "--log"])),
            Err(OsString::from("--log")),
            "the offender, not the first flag"
        );
        // `parse` turns that into usage, which `main` prints on stderr with
        // exit 2.
        assert_eq!(parse(&argv(&["wisp", "run", "--nonsense"])), Command::Usage);
    }

    #[test]
    fn a_value_flag_without_a_value_is_an_error() {
        // The flag last.
        assert_eq!(partition_run_flags(&argv(&["--scale"])), Err(OsString::from("--scale")));
        assert_eq!(partition_run_flags(&argv(&["--stub", "--log"])), Err(OsString::from("--log")));
        // The flag followed by another flag: `--backend` is not a scale, and
        // forwarding it as one would hand the HUD a backend named "--backend".
        assert_eq!(partition_run_flags(&argv(&["--scale", "--backend"])), Err(OsString::from("--scale")));
        // A wispd value flag followed by a HUD one, and by one of its own.
        assert_eq!(partition_run_flags(&argv(&["--log", "--scale"])), Err(OsString::from("--log")));
        assert_eq!(partition_run_flags(&argv(&["--log", "--stub"])), Err(OsString::from("--log")));
        // Nothing was forwarded in any of those cases: a silent drop is the
        // failure this pins.
        assert_eq!(parse(&argv(&["wisp", "run", "--scale"])), Command::Usage);
    }

    #[test]
    fn presence_flags_are_forwarded_bare() {
        // `wispd` reads both with `iter().any(…)`, so neither takes a value:
        // each is forwarded as itself and swallows nothing after it.
        let (wispd, hud) = partition_run_flags(&argv(&["--from-start", "--stub"])).unwrap();
        assert_eq!(wispd, argv(&["--from-start", "--stub"]));
        assert!(hud.is_empty());
        // A value flag after a presence flag keeps its own value...
        let (wispd, _) = partition_run_flags(&argv(&["--stub", "--log", "/a"])).unwrap();
        assert_eq!(wispd, argv(&["--stub", "--log", "/a"]));
        // ...and an argument after a presence flag that is not itself a flag is
        // the offender, never a value the presence flag took.
        assert_eq!(
            partition_run_flags(&argv(&["--from-start", "sleep"])),
            Err(OsString::from("sleep"))
        );
    }

    #[test]
    fn a_value_with_one_leading_dash_is_a_value() {
        // Only `--` begins a flag here: every flag Wisp has is a long one, so a
        // path with a single leading dash is the relative path the user meant
        // and forwarding it is right.
        let (wispd, hud) = partition_run_flags(&argv(&["--log", "-eqlog_x.txt"])).unwrap();
        assert_eq!(wispd, argv(&["--log", "-eqlog_x.txt"]));
        assert!(hud.is_empty());
        let (_, hud) = partition_run_flags(&argv(&["--scale", "-1"])).unwrap();
        assert_eq!(hud, argv(&["--scale", "-1"]), "the reader refuses it, not the partition");
        let (wispd, _) = partition_run_flags(&argv(&["--spells", "-"])).unwrap();
        assert_eq!(wispd, argv(&["--spells", "-"]));
        // Two dashes are still a flag, so this is a value flag with no value.
        assert_eq!(partition_run_flags(&argv(&["--log", "--stub"])), Err(OsString::from("--log")));
    }

    #[test]
    fn flags_keep_their_values_next_to_them() {
        let (wispd, hud) =
            partition_run_flags(&argv(&["--backend", A_BACKEND, "--log", "/a", "--scale", "32"])).unwrap();
        assert_eq!(wispd, argv(&["--log", "/a"]));
        assert_eq!(hud, argv(&["--backend", A_BACKEND, "--scale", "32"]));
        assert_eq!(partition_run_flags(&[]).unwrap(), (Vec::new(), Vec::new()));
    }

    #[test]
    fn non_utf8_values_survive_partitioning() {
        // A log path is arbitrary bytes on Linux. OsStr-clean throughout: no
        // String round-trip anywhere in this module.
        let value = OsString::from_vec(b"/games/\xff\xfe/Logs".to_vec());
        let args = vec![OsString::from("--logs-dir"), value.clone()];
        let (wispd, hud) = partition_run_flags(&args).unwrap();
        assert_eq!(wispd, vec![OsString::from("--logs-dir"), value.clone()]);
        assert!(hud.is_empty());
        assert_eq!(
            parse(&[OsString::from("wisp"), OsString::from("run"), OsString::from("--log"), value.clone()]),
            Command::Run(RunArgs {
                wispd: vec![OsString::from("--log"), value.clone()],
                hud: Vec::new(),
                command: None,
            })
        );
        // A value that is not text is still a value, not a flag.
        let undecodable = OsString::from_vec(b"\xff\xfe".to_vec());
        assert_eq!(
            partition_run_flags(&[OsString::from("--scale"), undecodable.clone()]).unwrap(),
            (Vec::new(), vec![OsString::from("--scale"), undecodable])
        );
    }
}
