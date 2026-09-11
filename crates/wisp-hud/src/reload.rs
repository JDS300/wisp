// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/reload.rs
//! Watches the config file for edits made outside the HUD (a text editor, or
//! `wisp config set`) so a running HUD picks them up without a restart.
//!
//! An mtime poll, not `inotify`: the file also gets rewritten by the HUD's
//! own save, and a poll throttled to twice a second is simpler than teaching
//! a watch to tell its own write apart from someone else's.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use wisp_config::config::Config;

/// How often `poll` actually stats the file; calls in between are free reads
/// of state already known.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub struct ConfigWatch {
    path: PathBuf,
    last_check: Option<Instant>,
    last_mtime: Option<SystemTime>,
}

impl ConfigWatch {
    pub fn new(path: PathBuf) -> ConfigWatch {
        let last_mtime = mtime_of(&path);
        ConfigWatch { path, last_check: None, last_mtime }
    }

    /// Checks the mtime at most every 500 ms. `Some(config)` when the file
    /// changed and parsed; a file that changed but failed to read is
    /// reported once via `Err`.
    pub fn poll(&mut self, now: Instant) -> Option<Result<Config, String>> {
        if let Some(last) = self.last_check {
            if now.duration_since(last) < POLL_INTERVAL {
                return None;
            }
        }
        self.last_check = Some(now);

        let mtime = mtime_of(&self.path);
        if mtime == self.last_mtime {
            return None;
        }
        self.last_mtime = mtime;

        // `mtime` went from `None` to `Some` (the file appeared) or from
        // `Some` to a different `Some` (it was rewritten); either way it is
        // worth a read. A `Some` -> `None` transition (the file vanished)
        // is not a config change to report -- there is nothing to parse --
        // so it falls through with no result, the mtime already updated.
        mtime?;
        match fs::read_to_string(&self.path) {
            Ok(text) => Some(Ok(Config::parse(&text))),
            Err(e) => Some(Err(e.to_string())),
        }
    }

    /// Called after the HUD itself saved, so its own write does not reload.
    pub fn mark_saved(&mut self) {
        self.last_mtime = mtime_of(&self.path);
    }
}

fn mtime_of(path: &std::path::Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn scratch_file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wisp-hud-reload-{}-{tag}", std::process::id()))
    }

    /// Sets a file's mtime to a value strictly after whatever it has now, so
    /// a fast-running test still produces a change `poll` can see even on a
    /// filesystem with coarse timestamp resolution.
    fn touch_after(path: &std::path::Path, after: SystemTime) {
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(after + Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn poll_reports_a_change_once_then_is_quiet_again() {
        let path = scratch_file("change");
        fs::write(&path, "log = /a\n").unwrap();
        let start = mtime_of(&path).unwrap();

        let mut watch = ConfigWatch::new(path.clone());
        let base = Instant::now();

        assert!(watch.poll(base).is_none(), "no change yet");

        touch_after(&path, start);
        let after_throttle = base + POLL_INTERVAL;
        match watch.poll(after_throttle) {
            Some(Ok(config)) => assert_eq!(config.get(wisp_config::config::Key::Log), Some("/a")),
            other => panic!("expected Some(Ok(_)), got {other:?}"),
        }

        assert!(
            watch.poll(after_throttle + POLL_INTERVAL).is_none(),
            "the same mtime is not reported twice"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_poll_inside_the_throttle_window_does_not_check_at_all() {
        let path = scratch_file("throttle");
        fs::write(&path, "log = /a\n").unwrap();
        let mtime = mtime_of(&path).unwrap();

        let mut watch = ConfigWatch::new(path.clone());
        let base = Instant::now();
        watch.poll(base);

        touch_after(&path, mtime);
        // Well inside the 500 ms window: the change is real, but the throttle
        // must skip the stat entirely and report nothing yet.
        assert!(watch.poll(base + Duration::from_millis(100)).is_none());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn an_unreadable_change_is_reported_as_err_once() {
        let path = scratch_file("unreadable");
        fs::write(&path, "log = /a\n").unwrap();
        let start = mtime_of(&path).unwrap();

        let mut watch = ConfigWatch::new(path.clone());
        let base = Instant::now();
        watch.poll(base);

        fs::write(&path, b"log = \xff\xfe\n").unwrap();
        touch_after(&path, start);
        match watch.poll(base + POLL_INTERVAL) {
            Some(Err(_)) => {}
            other => panic!("expected Some(Err(_)), got {other:?}"),
        }
        assert!(
            watch.poll(base + POLL_INTERVAL * 2).is_none(),
            "the same failed read is not reported twice"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn mark_saved_after_the_huds_own_write_suppresses_the_next_poll() {
        let path = scratch_file("mark-saved");
        fs::write(&path, "log = /a\n").unwrap();
        let start = mtime_of(&path).unwrap();

        let mut watch = ConfigWatch::new(path.clone());
        let base = Instant::now();
        watch.poll(base);

        // The HUD's own save: write, then tell the watch about it.
        fs::write(&path, "log = /b\n").unwrap();
        touch_after(&path, start);
        watch.mark_saved();

        assert!(
            watch.poll(base + POLL_INTERVAL).is_none(),
            "the HUD's own write is not seen as an external change"
        );

        let _ = fs::remove_file(&path);
    }
}
