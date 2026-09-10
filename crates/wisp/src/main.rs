// SPDX-License-Identifier: MIT
//! `wisp` — the CLI and the launcher.
//!
//! Spec 0's third component: one command that starts the daemon and the HUD,
//! reports what either is doing, explains what it *would* do before doing any of
//! it, and owns the config file all three read. Argument handling is hand-rolled
//! like the other two binaries — `args_os`, no clap — and the parsing itself is
//! in [`args`], pure and unit-tested, so what is left here is dispatch, the one
//! config warning, and the exit codes.

mod args;
mod config_cmd;
mod doctor;
mod run;
mod status;

use args::{Command, ConfigCommand};
use std::ffi::OsString;
use wisp_config::config::{Config, Key};

/// Every command, in the order the spec's table lists them, and the flags each
/// takes. Printed to stderr with exit 2, which is the code for "you invoked me
/// wrongly" throughout Wisp: a usage dump is not a failure of the program.
///
/// Only `--log` and `--logs-dir` are alternatives to each other, so only they
/// share a bracket: `--spells` is an independent third setting, and `--scale`
/// and `--backend` are two settings of the HUD's that a launch may give together.
const USAGE: &str = "\
usage: wisp run [--log <path> | --logs-dir <dir>] [--spells <dir>] [--from-start] [--stub]
                [--scale <px>] [--backend <name>] [-- <command>...]
       wisp status [--json]
       wisp doctor [--log <path> | --logs-dir <dir>] [--spells <dir>] [--scale <px>] [--backend <name>]
       wisp config path | show | set <key> <value>
       wisp version | --version
       config keys: log, logs_dir, spells_dir, scale, backend
";

fn main() {
    // args_os, not args: a log path is arbitrary bytes on Linux and must not
    // panic on non-UTF-8, so nothing here becomes text until it has to — a value
    // being written into the config file, which is UTF-8 whether or not the
    // path is.
    let argv: Vec<OsString> = std::env::args_os().collect();
    std::process::exit(dispatch(args::parse(&argv)));
}

fn dispatch(command: Command) -> i32 {
    match command {
        Command::Run(args) => {
            // The config is read here for its warning alone: `run` forwards only
            // what the user typed, and both children read the file themselves.
            // But a file that cannot be read is this process's to report too,
            // once, before anything is spawned — the daemon and the HUD each say
            // the same line about the same file afterwards, and a user who
            // launched from a desktop entry sees none of theirs.
            let _config = config_or_report();
            run::run(args)
        }
        Command::Status { json } => {
            // Read for the warning, as above: `status` takes nothing else from
            // it, and an unreadable config is not a reason to refuse a daemon
            // that is running.
            let _config = config_or_report();
            status::status(json)
        }
        Command::Doctor(args) => doctor::doctor(&config_or_report(), &args),
        Command::Config(ConfigCommand::Path) => config_cmd::path(),
        Command::Config(ConfigCommand::Show) => config_cmd::show(&config_or_report()),
        Command::Config(ConfigCommand::Set(key, value)) => config_cmd::set(key, &value),
        Command::Config(ConfigCommand::SetUnknown(name)) => {
            // The five keys come from `Key::ALL` rather than being spelled here,
            // so the message cannot drift from the file format it describes.
            let keys: Vec<&str> = Key::ALL.iter().map(Key::name).collect();
            eprintln!("wisp: unknown config key: {name} (one of: {})", keys.join(", "));
            2
        }
        Command::Version => {
            println!("wisp {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Command::Usage => {
            eprint!("{USAGE}");
            2
        }
    }
}

/// The config file, or an empty config when there is none or it cannot be read.
///
/// An unreadable one is said once, here, and never again by this process. The
/// line is the one `wispd` and `wisp-hud` say about the same file: the program
/// name differs and nothing after it does. It is never fatal — Wisp runs on its
/// defaults with no config file at all, so it runs on them with a broken one.
fn config_or_report() -> Config {
    // No path means nowhere to look, which is an empty config rather than an
    // error: `config path` is where a missing variable gets named.
    let Ok(path) = wisp_config::paths::config_path() else {
        return Config::default();
    };
    match Config::load(&path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("wisp: ignoring unreadable config {}: {e}", path.display());
            Config::default()
        }
    }
}

/// One labelled line of a report: the label padded to ten columns and a space,
/// so every value starts in the same column.
///
/// Shared by `status` and `doctor` because they are the same report in two
/// shapes — one snapshot and one machine — and two copies of the width would
/// drift. A value that is empty, which a daemon with no log open has for its
/// timestamp, would otherwise leave the padding behind it as trailing
/// whitespace.
pub(crate) fn labelled(label: &str, value: &str) -> String {
    format!("{label:<10} {value}").trim_end().to_string()
}
