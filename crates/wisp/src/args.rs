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
use wisp_config::layout::{Anchor, BlockKind};

/// What `wisp` was asked to do.
///
/// Not `Eq`: [`HudCommand::Scale`] carries an `f32`, which has none.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Run(RunArgs),
    Status { json: bool },
    Doctor(DoctorArgs),
    Config(ConfigCommand),
    Hud(HudCommand),
    Version,
    /// Usage on stderr and exit 2. Bare `wisp` lands here rather than meaning
    /// `run`: the desktop entry and the AppImage's `AppRun` say `run`
    /// explicitly, so a bare `wisp` on `PATH` is a mistake and not a launch.
    Usage,
}

/// What `wisp hud` was asked to do.
///
/// `Set`'s key and value stay text: a block's key vocabulary depends on
/// nothing this module knows, so validating either is `hud_cmd`'s job, once
/// the config is loaded and the target block's kind is in hand.
#[derive(Debug, Clone, PartialEq)]
pub enum HudCommand {
    List,
    Place { index: usize, anchor: Anchor, x: i32, y: i32 },
    Nudge { index: usize, dx: i32, dy: i32 },
    Set { index: usize, key: String, value: String },
    Add(BlockKind),
    Remove { index: usize },
    Scale(f32),
    /// `hud.output`: `Some(name)` pins the HUD to the `wl_output` (or X
    /// screen) of that name, `None` -- spelled `auto` on the command line --
    /// removes the key so the compositor picks.
    Output(Option<String>),
    /// Bad syntax at parse time: an unknown verb, the wrong number of
    /// arguments, or a value that does not even parse (a non-numeric index, an
    /// anchor word not among the nine, a scale that is not a positive number).
    /// Carries the usage text to print, so `hud_cmd::hud` alone owns the exit
    /// code for every form this command takes.
    Invalid(String),
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
        Some("hud") => hud_command(args),
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

/// `wisp hud`'s seven forms. A malformed one is [`HudCommand::Invalid`]
/// carrying the whole usage text (`crate::USAGE`, the one place it is
/// spelled), so `hud_cmd::hud` is the only place that decides an exit code
/// for this command.
fn hud_command(args: &[OsString]) -> Command {
    let Some(first) = args.first() else {
        return Command::Hud(HudCommand::List);
    };
    let Some(sub) = first.to_str() else {
        return invalid_hud();
    };
    let rest = &args[1..];
    match (sub, rest) {
        ("list", []) => Command::Hud(HudCommand::List),
        ("place", [n, anchor, x, y]) => {
            match (parse_usize(n), parse_anchor(anchor), parse_i32(x), parse_i32(y)) {
                (Some(index), Some(anchor), Some(x), Some(y)) => {
                    Command::Hud(HudCommand::Place { index, anchor, x, y })
                }
                _ => invalid_hud(),
            }
        }
        ("nudge", [n, dx, dy]) => match (parse_usize(n), parse_i32(dx), parse_i32(dy)) {
            (Some(index), Some(dx), Some(dy)) => Command::Hud(HudCommand::Nudge { index, dx, dy }),
            _ => invalid_hud(),
        },
        ("set", [n, key, value]) => match parse_usize(n) {
            Some(index) => Command::Hud(HudCommand::Set {
                index,
                key: text_of(key),
                value: text_of(value),
            }),
            None => invalid_hud(),
        },
        ("add", [kind]) => match kind.to_str().and_then(parse_block_kind) {
            Some(kind) => Command::Hud(HudCommand::Add(kind)),
            None => invalid_hud(),
        },
        ("remove", [n]) => match parse_usize(n) {
            Some(index) => Command::Hud(HudCommand::Remove { index }),
            None => invalid_hud(),
        },
        // `is_valid_scale` rather than a `> 0.0` of its own: the same check
        // `wisp config set scale` makes, in the crate that has to write the
        // value afterwards. `> 0.0` refused `NaN` (every comparison with it
        // is false) but let `inf` through.
        ("scale", [factor]) => match factor.to_str().and_then(|text| text.trim().parse::<f32>().ok())
        {
            Some(factor) if wisp_config::config::is_valid_scale(factor) => {
                Command::Hud(HudCommand::Scale(factor))
            }
            _ => invalid_hud(),
        },
        // An output name is whatever the compositor calls it (`DP-1`,
        // `HDMI-A-1`), so the only shape check is that there is one and it
        // has no whitespace; whether it exists is the HUD's to report.
        ("output", [name]) => match name.to_str() {
            Some("auto") => Command::Hud(HudCommand::Output(None)),
            Some(name) if !name.is_empty() && !name.contains(char::is_whitespace) => {
                Command::Hud(HudCommand::Output(Some(name.to_string())))
            }
            _ => invalid_hud(),
        },
        _ => invalid_hud(),
    }
}

fn invalid_hud() -> Command {
    Command::Hud(HudCommand::Invalid(crate::USAGE.to_string()))
}

/// `n` as an index: no sign, no leading `+`, nothing but digits -- exactly
/// what a block's position in the list is.
fn parse_usize(arg: &OsStr) -> Option<usize> {
    arg.to_str()?.parse().ok()
}

/// An offset or a delta: whole, signed.
fn parse_i32(arg: &OsStr) -> Option<i32> {
    arg.to_str()?.parse().ok()
}

/// The nine words [`Anchor`]'s own `serde(rename_all = "kebab-case")` reads,
/// matched by hand rather than through `toml`: this crate depends on neither
/// `toml` nor `serde`, and nine strings are not worth adding either for.
fn parse_anchor(arg: &OsStr) -> Option<Anchor> {
    match arg.to_str()? {
        "top-left" => Some(Anchor::TopLeft),
        "top" => Some(Anchor::Top),
        "top-right" => Some(Anchor::TopRight),
        "left" => Some(Anchor::Left),
        "center" => Some(Anchor::Center),
        "right" => Some(Anchor::Right),
        "bottom-left" => Some(Anchor::BottomLeft),
        "bottom" => Some(Anchor::Bottom),
        "bottom-right" => Some(Anchor::BottomRight),
        _ => None,
    }
}

/// `wisp hud add`'s one argument: the same two words `BlockKind`'s own
/// `serde(rename_all = "lowercase")` reads.
fn parse_block_kind(text: &str) -> Option<BlockKind> {
    match text {
        "meter" => Some(BlockKind::Meter),
        "timers" => Some(BlockKind::Timers),
        _ => None,
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
    fn hud_with_no_verb_or_list_is_the_report() {
        assert_eq!(parse(&argv(&["wisp", "hud"])), Command::Hud(HudCommand::List));
        assert_eq!(parse(&argv(&["wisp", "hud", "list"])), Command::Hud(HudCommand::List));
        assert!(matches!(
            parse(&argv(&["wisp", "hud", "list", "extra"])),
            Command::Hud(HudCommand::Invalid(_))
        ));
    }

    #[test]
    fn hud_place_parses_the_index_anchor_and_offset() {
        assert_eq!(
            parse(&argv(&["wisp", "hud", "place", "1", "bottom-right", "20", "-5"])),
            Command::Hud(HudCommand::Place { index: 1, anchor: Anchor::BottomRight, x: 20, y: -5 })
        );
        // Every one of the nine words parses.
        let words = [
            ("top-left", Anchor::TopLeft),
            ("top", Anchor::Top),
            ("top-right", Anchor::TopRight),
            ("left", Anchor::Left),
            ("center", Anchor::Center),
            ("right", Anchor::Right),
            ("bottom-left", Anchor::BottomLeft),
            ("bottom", Anchor::Bottom),
            ("bottom-right", Anchor::BottomRight),
        ];
        for (word, anchor) in words {
            assert_eq!(
                parse(&argv(&["wisp", "hud", "place", "0", word, "0", "0"])),
                Command::Hud(HudCommand::Place { index: 0, anchor, x: 0, y: 0 }),
                "{word}"
            );
        }
        // A word that is not one of the nine, a non-numeric index or offset,
        // and the wrong number of arguments are all the same refusal.
        assert!(matches!(
            parse(&argv(&["wisp", "hud", "place", "0", "middle", "0", "0"])),
            Command::Hud(HudCommand::Invalid(_))
        ));
        assert!(matches!(
            parse(&argv(&["wisp", "hud", "place", "x", "top-left", "0", "0"])),
            Command::Hud(HudCommand::Invalid(_))
        ));
        assert!(matches!(
            parse(&argv(&["wisp", "hud", "place", "0", "top-left", "0"])),
            Command::Hud(HudCommand::Invalid(_))
        ));
    }

    #[test]
    fn hud_nudge_parses_the_index_and_delta() {
        assert_eq!(
            parse(&argv(&["wisp", "hud", "nudge", "1", "-10", "5"])),
            Command::Hud(HudCommand::Nudge { index: 1, dx: -10, dy: 5 })
        );
        assert!(matches!(
            parse(&argv(&["wisp", "hud", "nudge", "1", "-10"])),
            Command::Hud(HudCommand::Invalid(_))
        ));
    }

    #[test]
    fn hud_set_keeps_the_key_and_value_as_text() {
        assert_eq!(
            parse(&argv(&["wisp", "hud", "set", "0", "shows", "healing"])),
            Command::Hud(HudCommand::Set { index: 0, key: "shows".to_string(), value: "healing".to_string() })
        );
        // The key and value are not validated here -- only `hud_cmd` knows the
        // five keys, and it needs the loaded block to check them against.
        assert_eq!(
            parse(&argv(&["wisp", "hud", "set", "0", "nonsense", "also-nonsense"])),
            Command::Hud(HudCommand::Set {
                index: 0,
                key: "nonsense".to_string(),
                value: "also-nonsense".to_string()
            })
        );
        assert!(
            matches!(
                parse(&argv(&["wisp", "hud", "set", "x", "shows", "healing"])),
                Command::Hud(HudCommand::Invalid(_))
            ),
            "a non-numeric index is refused here; a bad key is not"
        );
    }

    #[test]
    fn hud_add_parses_the_two_block_kinds() {
        assert_eq!(parse(&argv(&["wisp", "hud", "add", "meter"])), Command::Hud(HudCommand::Add(BlockKind::Meter)));
        assert_eq!(
            parse(&argv(&["wisp", "hud", "add", "timers"])),
            Command::Hud(HudCommand::Add(BlockKind::Timers))
        );
        assert!(matches!(
            parse(&argv(&["wisp", "hud", "add", "nonsense"])),
            Command::Hud(HudCommand::Invalid(_))
        ));
    }

    #[test]
    fn hud_remove_parses_the_index() {
        assert_eq!(parse(&argv(&["wisp", "hud", "remove", "2"])), Command::Hud(HudCommand::Remove { index: 2 }));
        assert!(
            matches!(
                parse(&argv(&["wisp", "hud", "remove", "-1"])),
                Command::Hud(HudCommand::Invalid(_))
            ),
            "a negative number is not a usize"
        );
    }

    #[test]
    fn hud_scale_parses_a_positive_float() {
        assert_eq!(parse(&argv(&["wisp", "hud", "scale", "1.5"])), Command::Hud(HudCommand::Scale(1.5)));
        assert_eq!(parse(&argv(&["wisp", "hud", "scale", "2"])), Command::Hud(HudCommand::Scale(2.0)));
        for bad in ["0", "-1", "not-a-number", "", "nan", "NaN", "inf", "-inf"] {
            assert!(
                matches!(parse(&argv(&["wisp", "hud", "scale", bad])), Command::Hud(HudCommand::Invalid(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn hud_output_names_an_output_or_auto() {
        assert_eq!(
            parse(&argv(&["wisp", "hud", "output", "DP-1"])),
            Command::Hud(HudCommand::Output(Some("DP-1".to_string())))
        );
        assert_eq!(parse(&argv(&["wisp", "hud", "output", "auto"])), Command::Hud(HudCommand::Output(None)));
        for bad in ["", " ", "auto ", "with space"] {
            assert!(
                matches!(parse(&argv(&["wisp", "hud", "output", bad])), Command::Hud(HudCommand::Invalid(_))),
                "{bad:?}"
            );
        }
        assert!(matches!(parse(&argv(&["wisp", "hud", "output"])), Command::Hud(HudCommand::Invalid(_))));
        assert!(matches!(
            parse(&argv(&["wisp", "hud", "output", "DP-1", "DP-2"])),
            Command::Hud(HudCommand::Invalid(_))
        ));
    }

    #[test]
    fn an_unknown_hud_verb_is_invalid() {
        assert!(matches!(parse(&argv(&["wisp", "hud", "nonsense"])), Command::Hud(HudCommand::Invalid(_))));
        // The usage text it carries is the one `main` prints for everything else.
        let Command::Hud(HudCommand::Invalid(usage)) = parse(&argv(&["wisp", "hud", "nonsense"])) else {
            panic!("expected Invalid");
        };
        assert_eq!(usage, crate::USAGE);
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
