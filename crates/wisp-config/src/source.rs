// SPDX-License-Identifier: MIT
//! Which log to read, and which spell data goes with it, given the flags and the
//! config file.
//!
//! One precedence rule with three consumers: `wispd` resolves it to decide what
//! to tail, `wisp run` to decide what it was told, and `wisp doctor` to explain
//! what it *would* tail and where that answer came from. Three copies of a
//! four-step chain is three chances for the daemon and the doctor to disagree
//! about the same machine. The spells chain beside it exists for the same
//! reason, and rests on the log chain's answer rather than asking again.

use crate::config::{Config, Key};
use crate::spells::{spells_dir_from_log, spells_dir_from_logs_dir};
use std::path::PathBuf;

/// The log a binary should read: one file it was given, or a directory in which
/// to find the newest `eqlog_*.txt` (see [`crate::discover`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogSource {
    /// `--log`, or the config's `log`.
    File(PathBuf),
    /// `--logs-dir`, or the config's `logs_dir`.
    Dir(PathBuf),
}

/// The precedence: a flag beats the file, the file beats the built-in default —
/// which is none — and a file beats a directory at both levels, because naming
/// one log is more specific than naming a folder of them.
///
/// `None` means nothing named a log, which is not an error here. `wispd --stub`
/// wants exactly that answer, and it is the caller that decides whether to run
/// on without one or to refuse and say why. A caller that must not consult the
/// config passes an empty one rather than growing a flag of its own here.
///
/// Empty config values never reach this function as paths: [`Config::path_value`]
/// reports a key written with nothing after the `=` as absent, so an unfinished
/// edit falls through to the next source instead of resolving to `""`.
pub fn resolve_log_source(
    log_flag: Option<PathBuf>,
    logs_dir_flag: Option<PathBuf>,
    config: &Config,
) -> Option<LogSource> {
    log_flag
        .map(LogSource::File)
        .or_else(|| logs_dir_flag.map(LogSource::Dir))
        .or_else(|| config.path_value(Key::Log).map(LogSource::File))
        .or_else(|| config.path_value(Key::LogsDir).map(LogSource::Dir))
}

/// Where the client's spell data is, given the flag, the config file, and the log
/// source [`resolve_log_source`] settled on.
///
/// The order is the spec's: `--spells`, else the config's `spells_dir`, else the
/// location derived from the source — the install being the parent of a directory
/// named `Logs`, or of the parent of a log file under one. Derivation is
/// [`crate::spells`]' and is not restated here.
///
/// `None` means no timers, which is not an error: the daemon still counts kills
/// and says once that timers are disabled. It sits beside the log chain for the
/// same reason that chain sits here — `wispd` resolves it to load the table and
/// `wisp doctor` resolves it to report where the table would come from, and two
/// copies of a three-step order are two chances for the doctor to disagree with
/// the daemon.
pub fn resolve_spells_dir(
    spells_flag: Option<PathBuf>,
    config: &Config,
    source: Option<&LogSource>,
) -> Option<PathBuf> {
    spells_flag
        .or_else(|| config.path_value(Key::SpellsDir))
        .or_else(|| match source {
            Some(LogSource::Dir(dir)) => spells_dir_from_logs_dir(dir),
            Some(LogSource::File(log)) => spells_dir_from_log(log),
            None => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    /// A config holding both keys, so every test that consults it is testing
    /// the order rather than the absence of a value.
    fn both_keys() -> Config {
        Config::parse("log = /config/eqlog_Daggo_freeport.txt\nlogs_dir = /config/Logs\n")
    }

    #[test]
    fn the_log_flag_wins_over_everything() {
        // All four sources set: the most specific thing the user typed wins.
        assert_eq!(
            resolve_log_source(Some(p("/flag/eqlog_x.txt")), Some(p("/flag/Logs")), &both_keys()),
            Some(LogSource::File(p("/flag/eqlog_x.txt")))
        );
    }

    #[test]
    fn the_logs_dir_flag_wins_when_there_is_no_log_flag() {
        assert_eq!(
            resolve_log_source(None, Some(p("/flag/Logs")), &both_keys()),
            Some(LogSource::Dir(p("/flag/Logs")))
        );
    }

    #[test]
    fn config_log_is_used_when_neither_flag_is_set() {
        assert_eq!(
            resolve_log_source(None, None, &both_keys()),
            Some(LogSource::File(p("/config/eqlog_Daggo_freeport.txt")))
        );
    }

    #[test]
    fn config_logs_dir_is_the_last_resort() {
        let config = Config::parse("logs_dir = /config/Logs\n");
        assert_eq!(
            resolve_log_source(None, None, &config),
            Some(LogSource::Dir(p("/config/Logs")))
        );
    }

    #[test]
    fn a_file_beats_a_directory_at_the_same_level() {
        // The tie-break is applied at each level independently, not once at the
        // top: two flags resolve to the file, and two keys resolve to the file,
        // for the same reason — one named log beats a folder that might hold any.
        assert_eq!(
            resolve_log_source(Some(p("/flag/eqlog_x.txt")), Some(p("/flag/Logs")), &Config::default()),
            Some(LogSource::File(p("/flag/eqlog_x.txt"))),
            "both flags set"
        );
        assert_eq!(
            resolve_log_source(None, None, &both_keys()),
            Some(LogSource::File(p("/config/eqlog_Daggo_freeport.txt"))),
            "both keys set"
        );
    }

    #[test]
    fn nothing_set_is_none() {
        // A config that exists and is read, but names no log: the HUD's two keys
        // are set and neither says where the game writes.
        let config = Config::parse("scale = 48\nbackend = plain\n");
        assert_eq!(resolve_log_source(None, None, &config), None);
    }

    #[test]
    fn an_empty_config_value_is_unset_and_does_not_win() {
        // `log =` with nothing after it is an unfinished edit. Were it a path it
        // would be `""`, it would beat `logs_dir` on specificity, and the daemon
        // would go looking for a file named nothing.
        let config = Config::parse("log =\nlogs_dir = /config/Logs\n");
        assert_eq!(
            resolve_log_source(None, None, &config),
            Some(LogSource::Dir(p("/config/Logs"))),
            "the empty `log` falls through to `logs_dir`"
        );

        let both_empty = Config::parse("log =\nlogs_dir =\n");
        assert_eq!(resolve_log_source(None, None, &both_empty), None);

        assert_eq!(
            resolve_log_source(None, Some(p("/flag/Logs")), &both_empty),
            Some(LogSource::Dir(p("/flag/Logs"))),
            "and a flag still wins over two empty keys"
        );
    }

    #[test]
    fn a_stub_run_with_no_source_resolves_to_none() {
        // The shape `wispd --stub` calls this in: no flags, and an empty config
        // handed over deliberately, because `--stub` reads neither key and a
        // config left over from a real session must not override it. `None` is
        // the answer the stub feed runs on, not an error.
        assert_eq!(
            resolve_log_source(None, None, &Config::default()),
            None,
            "nothing to tail, and the caller decides what that means"
        );
    }

    /// The install as the game lays it out, and the two ways a source can name
    /// it: a `Logs/` directory, or a log file inside one.
    const INSTALL: &str = "/games/EverQuest Legends";

    fn dir_source() -> LogSource {
        LogSource::Dir(p("/games/EverQuest Legends/Logs"))
    }

    fn file_source() -> LogSource {
        LogSource::File(p("/games/EverQuest Legends/Logs/eqlog_Daggo_freeport.txt"))
    }

    #[test]
    fn the_spells_flag_wins() {
        // All three sources set: what the user typed on the command line is what
        // gets loaded, so a mis-derived location can always be overridden.
        let config = Config::parse("spells_dir = /config/install\n");
        assert_eq!(
            resolve_spells_dir(Some(p("/flag/install")), &config, Some(&dir_source())),
            Some(p("/flag/install"))
        );
        assert_eq!(
            resolve_spells_dir(Some(p("/flag/install")), &config, Some(&file_source())),
            Some(p("/flag/install")),
            "the same, whichever shape the source has"
        );
    }

    #[test]
    fn config_spells_dir_beats_derivation() {
        let config = Config::parse("spells_dir = /config/install\n");
        assert_eq!(
            resolve_spells_dir(None, &config, Some(&dir_source())),
            Some(p("/config/install"))
        );
        // An empty value is unset here exactly as it is in the log chain, so the
        // derivation still gets its turn.
        let empty = Config::parse("spells_dir =\n");
        assert_eq!(
            resolve_spells_dir(None, &empty, Some(&dir_source())),
            Some(p(INSTALL))
        );
    }

    #[test]
    fn spells_is_derived_from_the_source_when_nothing_names_it() {
        let config = Config::default();
        // A directory named `Logs` gives its parent...
        assert_eq!(resolve_spells_dir(None, &config, Some(&dir_source())), Some(p(INSTALL)));
        // ...and a log inside one gives its grandparent, so the two entry points
        // agree about the same install.
        assert_eq!(resolve_spells_dir(None, &config, Some(&file_source())), Some(p(INSTALL)));
        // A source that derives nothing — a logs directory not named `Logs` —
        // leaves the answer empty rather than guessing at a parent.
        let elsewhere = LogSource::Dir(p("/somewhere/eqlogs"));
        assert_eq!(resolve_spells_dir(None, &config, Some(&elsewhere)), None);
    }

    #[test]
    fn no_source_and_no_key_is_none() {
        // `wispd --stub`'s shape: no log to derive from and nothing written
        // down, so no spell table and no timers. Not an error.
        assert_eq!(resolve_spells_dir(None, &Config::default(), None), None);
        // A flag alone still resolves, with no source at all to derive from.
        assert_eq!(
            resolve_spells_dir(Some(p("/flag/install")), &Config::default(), None),
            Some(p("/flag/install"))
        );
    }
}
