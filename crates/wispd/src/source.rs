// SPDX-License-Identifier: MIT
//! What one poll tick should do about the log the daemon is reading.
//!
//! Split out of `main` so the whole switch decision is a pure function. The
//! daemon scans a directory four times a second and has to answer "is this the
//! file I am already reading?" correctly every time; a rule that can be pinned
//! without a daemon, a directory or a running game is a rule that stays pinned.
//!
//! *Which* log to read in the first place — `--log` against `--logs-dir` against
//! the config file — is `wisp_config::source::resolve_log_source`, because
//! `wisp doctor` has to reach the same answer about the same machine.

use std::path::{Path, PathBuf};

/// What one tick's scan says the daemon should do.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Keep reading what is open, or keep waiting for something to be.
    Keep,
    /// Nothing was open; start on this file.
    Open(PathBuf),
    /// A different file is now the newest: end the session and start on it.
    Switch(PathBuf),
}

/// The switch decision — what is open now, against what the directory says is
/// newest.
///
/// A newest file that has gone away is `Keep`, not a reason to tear the session
/// down. The spec does not rule on this case and it is a plan decision:
/// `Tailer::poll` already tolerates a vanished file by returning no lines, the
/// game rotating or rewriting its log is normal, and dropping back to the
/// waiting state would throw a session's counters away over a filesystem
/// hiccup that the next tick may well reverse.
pub fn next_action(current: Option<&Path>, scanned: Option<PathBuf>) -> Action {
    let Some(newest) = scanned else {
        return Action::Keep;
    };
    match current {
        None => Action::Open(newest),
        Some(open) if open == newest => Action::Keep,
        Some(_) => Action::Switch(newest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The two files of a real install's `Logs/`, and the character who logged
    /// in on top of it.
    fn freeport() -> std::path::PathBuf {
        std::path::PathBuf::from("/games/EverQuest Legends/Logs/eqlog_Daggo_freeport.txt")
    }

    fn rivervale() -> std::path::PathBuf {
        std::path::PathBuf::from("/games/EverQuest Legends/Logs/eqlog_Daggo_rivervale.txt")
    }

    #[test]
    fn nothing_open_and_nothing_found_keeps_waiting() {
        // The state before the game's first login: the daemon runs, publishes
        // zero counters and waits. Not an error and not a reason to exit.
        assert_eq!(next_action(None, None), Action::Keep);
    }

    #[test]
    fn the_first_file_found_is_opened() {
        assert_eq!(next_action(None, Some(freeport())), Action::Open(freeport()));
    }

    #[test]
    fn the_same_file_again_is_kept() {
        // The common tick: one readdir that changes nothing.
        assert_eq!(
            next_action(Some(Path::new(&freeport())), Some(freeport())),
            Action::Keep
        );
    }

    #[test]
    fn a_different_newest_file_is_a_switch() {
        // Another character or another server: the newest mtime moved, so the
        // session ends and a new one starts on the new file.
        assert_eq!(
            next_action(Some(Path::new(&freeport())), Some(rivervale())),
            Action::Switch(rivervale())
        );
    }

    #[test]
    fn a_vanished_newest_file_keeps_the_current_one() {
        // The file being tailed was deleted, moved or is momentarily invisible.
        assert_eq!(next_action(Some(Path::new(&freeport())), None), Action::Keep);
    }
}
