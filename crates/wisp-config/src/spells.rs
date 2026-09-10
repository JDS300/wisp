// SPDX-License-Identifier: MIT
//! Where the client install is, given only a log.
//!
//! The spell files are read at runtime and never shipped, so their location has
//! to be derived: the client writes its logs to `<install>/Logs/`, which makes
//! the install the parent of a directory with that name. Two entry points
//! because two different things get handed over — `--log` is a file and
//! `--logs-dir` a directory — and both must derive the same place.

use std::path::{Path, PathBuf};

/// The client install a log file lives in: `<install>/Logs/<file>` gives
/// `<install>`, and anything not under a `Logs` directory gives `None`.
pub fn spells_dir_from_log(log: &Path) -> Option<PathBuf> {
    spells_dir_from_logs_dir(log.parent()?)
}

/// The client install a `Logs/` directory belongs to. The name is matched
/// case-insensitively because it is the client's directory, not Wisp's.
pub fn spells_dir_from_logs_dir(dir: &Path) -> Option<PathBuf> {
    let is_logs = dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("logs"))
        .unwrap_or(false);
    if !is_logs {
        return None;
    }
    dir.parent().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn a_log_under_logs_gives_its_grandparent() {
        let log = Path::new("/games/EverQuest Legends/Logs/eqlog_Daggo_freeport.txt");
        assert_eq!(
            spells_dir_from_log(log),
            Some(PathBuf::from("/games/EverQuest Legends"))
        );
    }

    #[test]
    fn the_directory_name_matches_case_insensitively() {
        for name in ["LOGS", "Logs", "logs"] {
            let log = PathBuf::from("/games/EverQuest Legends").join(name).join("eqlog_x.txt");
            assert_eq!(
                spells_dir_from_log(&log),
                Some(PathBuf::from("/games/EverQuest Legends")),
                "{name}"
            );
        }
    }

    #[test]
    fn a_log_not_under_a_logs_directory_gives_none() {
        assert_eq!(spells_dir_from_log(Path::new("/tmp/eqlog_x.txt")), None);
        assert_eq!(
            spells_dir_from_log(Path::new("/games/EverQuest Legends/LaunchPad.libs/eqlog_x.txt")),
            None,
            "the launcher's own directory is not the client install"
        );
    }

    #[test]
    fn a_logs_directory_gives_its_parent() {
        assert_eq!(
            spells_dir_from_logs_dir(Path::new("/games/EverQuest Legends/Logs")),
            Some(PathBuf::from("/games/EverQuest Legends"))
        );
        assert_eq!(
            spells_dir_from_logs_dir(Path::new("/games/EverQuest Legends/LOGS")),
            Some(PathBuf::from("/games/EverQuest Legends"))
        );
    }

    #[test]
    fn a_directory_not_named_logs_gives_none() {
        assert_eq!(
            spells_dir_from_logs_dir(Path::new("/games/EverQuest Legends/LaunchPad.libs")),
            None
        );
        assert_eq!(spells_dir_from_logs_dir(Path::new("/tmp")), None);
    }

    #[test]
    fn the_file_and_directory_entry_points_agree() {
        // `--logs-dir` hands over a directory and `--log` a file; both must
        // derive the same client install, or the spell table would be loaded
        // from one place and the log read from another.
        let install = PathBuf::from(
            "/mnt/prefix/drive_c/users/Public/Daybreak Game Company/Installed Games/EverQuest Legends",
        );
        let dir = install.join("Logs");
        let log = dir.join("eqlog_Daggo_freeport.txt");
        assert_eq!(spells_dir_from_log(&log), Some(install.clone()));
        assert_eq!(spells_dir_from_logs_dir(&dir), Some(install));
    }
}
