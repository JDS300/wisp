// SPDX-License-Identifier: MIT
//! Which log to tail, given a directory.
//!
//! The rule is a pure function over a listing of names and mtimes, so the
//! newest-wins choice and its tie-break are unit-testable without touching the
//! disk; the polling that runs it every 250 ms stays in `wispd`. A directory
//! that does not exist yet is an ordinary answer rather than an error: the game
//! creates `Logs/` on login and the daemon must not need restarting.

use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, io};

/// The newest log in a listing of `(name, mtime)`, or `None` if it holds none.
pub fn newest_log(entries: impl IntoIterator<Item = (OsString, SystemTime)>) -> Option<OsString> {
    entries
        .into_iter()
        .filter(|(name, _)| is_log_name(name))
        .max_by(by_mtime_then_name)
        .map(|(name, _)| name)
}

/// Every log in `dir`, newest first, as full paths. `wisp doctor` counts the
/// list to report "newest of N files".
pub fn list_logs(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut found: Vec<(OsString, SystemTime)> = Vec::new();
    // An entry the listing itself cannot read is skipped, exactly like one that
    // cannot be stat'd below: a single bad file must not hide the others from a
    // daemon that polls this every 250 ms. Only failing to read the directory at
    // all is an error.
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !is_log_name(&name) {
            continue;
        }
        // `fs::metadata` rather than `DirEntry::metadata`, which does not follow
        // symlinks: a log the user has linked into the directory is still a log.
        let Ok(metadata) = fs::metadata(dir.join(&name)) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(mtime) = metadata.modified() else { continue };
        found.push((name, mtime));
    }
    found.sort_by(|a, b| by_mtime_then_name(b, a));
    Ok(found.into_iter().map(|(name, _)| dir.join(name)).collect())
}

/// The log to tail in `dir`: the head of [`list_logs`], or `None` when the
/// directory holds none yet.
pub fn scan_logs_dir(dir: &Path) -> io::Result<Option<PathBuf>> {
    Ok(list_logs(dir)?.into_iter().next())
}

/// Chronological order, oldest first; a tie goes to the earlier name in byte
/// order. Written once and used by both the pure rule — where the greatest under
/// it is the log to tail — and the directory scan, which reverses it, so the two
/// cannot drift.
fn by_mtime_then_name(a: &(OsString, SystemTime), b: &(OsString, SystemTime)) -> Ordering {
    a.1.cmp(&b.1).then_with(|| a.0.as_bytes().cmp(b.0.as_bytes()))
}

/// `eqlog_*.txt`, case-sensitively at both ends. The client's own spelling is
/// `eqlog_<Name>_<Server>.txt`; requiring the prefix and the suffix is what
/// stops the launcher's directory — which is also called `Logs` — from ever
/// satisfying the rule.
fn is_log_name(name: &OsStr) -> bool {
    name.as_bytes().starts_with(b"eqlog_") && name.as_bytes().ends_with(b".txt")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    /// A scratch directory of its own per test, since the harness runs them in
    /// parallel inside one process.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("wisp-config-discover-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A directory that must not exist: the daemon starts before the game has
    /// created `Logs/`, so this is the ordinary case, not an error.
    fn absent() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wisp-config-absent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// Create `name` holding nothing and stamp its mtime `secs_ago` in the past,
    /// so an ordering cannot be an accident of the names or of creation order.
    fn log_file(dir: &Path, name: &str, secs_ago: u64) {
        log_file_at(dir, name, SystemTime::now() - Duration::from_secs(secs_ago));
    }

    /// The same at an exact instant. Two files stamped by two `log_file` calls
    /// would differ by the microseconds between the calls, so a tie could never
    /// actually be set up through that helper.
    fn log_file_at(dir: &Path, name: &str, at: SystemTime) {
        let f = fs::File::create(dir.join(name)).unwrap();
        f.set_modified(at).unwrap();
    }

    fn entry(name: &str, secs_ago: u64) -> (OsString, SystemTime) {
        (OsString::from(name), SystemTime::now() - Duration::from_secs(secs_ago))
    }

    fn entry_at(name: &str, at: SystemTime) -> (OsString, SystemTime) {
        (OsString::from(name), at)
    }

    #[test]
    fn the_newest_mtime_wins() {
        // The install's Logs/ on the development box: freeport (2026-09-09)
        // beats rivervale (2026-08-12) whichever order the listing arrives in.
        let freeport = entry("eqlog_Daggo_freeport.txt", 60);
        let rivervale = entry("eqlog_Daggo_rivervale.txt", 2_419_200);
        let want = Some(OsString::from("eqlog_Daggo_freeport.txt"));
        assert_eq!(newest_log([freeport.clone(), rivervale.clone()]), want);
        assert_eq!(newest_log([rivervale, freeport]), want);
    }

    #[test]
    fn a_tie_is_broken_by_the_later_name_in_byte_order() {
        let at = SystemTime::now();
        let arago = entry_at("eqlog_Arago.txt", at);
        let berto = entry_at("eqlog_Berto.txt", at);
        let want = Some(OsString::from("eqlog_Berto.txt"));
        assert_eq!(newest_log([arago.clone(), berto.clone()]), want);
        assert_eq!(newest_log([berto, arago]), want);
    }

    #[test]
    fn names_that_are_not_eqlog_prefix_and_txt_suffix_are_ignored() {
        let now = SystemTime::now();
        // Case-sensitive at both ends.
        let not_logs = [
            entry_at("EQLOG_x.txt", now),
            entry_at("eqlog_x.log", now),
            entry_at("eqlog_x.TXT", now),
            entry_at("x.txt", now),
        ];
        let freeport = entry_at("eqlog_Daggo_freeport.txt", now - Duration::from_secs(3600));
        let want = Some(OsString::from("eqlog_Daggo_freeport.txt"));
        assert_eq!(newest_log([freeport.clone()]), want);
        let mut both = not_logs.to_vec();
        both.push(freeport);
        assert_eq!(newest_log(both), want, "an older real log beats a newer file that is not one");
        // The launcher's own `Logs` directory holds no eqlog_*.txt at all.
        assert_eq!(newest_log(not_logs.to_vec()), None);
    }

    #[test]
    fn an_empty_listing_gives_none() {
        assert_eq!(newest_log(Vec::new()), None);
    }

    #[test]
    fn list_logs_is_ordered_newest_first() {
        let dir = scratch("list-order");
        // Names ascending, mtimes deliberately not: Berto is the newest, so it
        // comes first however the names sort. Arago and Cailo share one exact
        // mtime, so their order can only come from the tie-break — the later
        // name in byte order first.
        let tied = SystemTime::now() - Duration::from_secs(200);
        log_file_at(&dir, "eqlog_Arago.txt", tied);
        log_file(&dir, "eqlog_Berto.txt", 100);
        log_file_at(&dir, "eqlog_Cailo.txt", tied);
        log_file(&dir, "not-a-log.txt", 1);
        assert_eq!(
            list_logs(&dir).unwrap(),
            vec![
                dir.join("eqlog_Berto.txt"),
                dir.join("eqlog_Cailo.txt"),
                dir.join("eqlog_Arago.txt"),
            ],
            "every matching file, newest first, as full paths"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_that_holds_no_log_is_ok_empty_and_ok_none() {
        // The two states a real install is found in before the first login: a
        // freshly created `Logs/` with nothing in it, and the launcher's own
        // `Logs/`, which holds files but no `eqlog_*.txt`. Neither is an error,
        // because the game creates the log later and the daemon must already be
        // running when it does.
        let dir = scratch("no-logs");
        fs::write(dir.join("readme.txt"), b"not a log").unwrap();
        fs::write(dir.join("EQLOG_uppercase.txt"), b"not a log").unwrap();
        fs::create_dir(dir.join("eqlog_a_directory.txt")).unwrap();
        assert_eq!(list_logs(&dir).unwrap(), Vec::<PathBuf>::new());
        assert_eq!(scan_logs_dir(&dir).unwrap(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_logs_of_a_missing_directory_is_ok_empty() {
        assert_eq!(list_logs(&absent()).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn scan_returns_the_full_path_joined_onto_the_directory() {
        let dir = scratch("scan-join");
        log_file(&dir, "eqlog_Daggo_freeport.txt", 60);
        assert_eq!(
            scan_logs_dir(&dir).unwrap(),
            Some(dir.join("eqlog_Daggo_freeport.txt"))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_returns_the_head_of_list_logs() {
        let dir = scratch("scan-head");
        log_file(&dir, "eqlog_Arago.txt", 300);
        log_file(&dir, "eqlog_Berto.txt", 100);
        log_file(&dir, "eqlog_Cailo.txt", 200);
        let head = list_logs(&dir).unwrap().into_iter().next();
        assert_eq!(scan_logs_dir(&dir).unwrap(), head);
        assert_eq!(head, Some(dir.join("eqlog_Berto.txt")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_of_a_missing_directory_is_ok_none() {
        assert_eq!(scan_logs_dir(&absent()).unwrap(), None);
    }

    #[test]
    fn scan_of_a_temp_dir_picks_the_file_whose_mtime_is_newest() {
        let dir = scratch("scan-newest");
        log_file(&dir, "eqlog_Daggo_rivervale.txt", 2_419_200);
        log_file(&dir, "eqlog_Daggo_freeport.txt", 60);
        log_file(&dir, "eqlog_Arago.txt", 3600);
        assert_eq!(
            scan_logs_dir(&dir).unwrap(),
            Some(dir.join("eqlog_Daggo_freeport.txt"))
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
