// SPDX-License-Identifier: MIT
//! Follows an append-only log, surviving truncation and replacement.
//!
//! Polled rather than watched with inotify: fewer filesystem edge cases, and
//! at a 5 Hz snapshot rate the latency is invisible.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::io;

pub struct Tailer {
    path: PathBuf,
    file: Option<File>,
    offset: u64,
    ident: Option<(u64, u64)>, // (dev, ino)
    partial: String,
}

impl Tailer {
    pub fn open(path: &Path, from_start: bool) -> io::Result<Tailer> {
        let mut t = Tailer {
            path: path.to_path_buf(),
            file: None,
            offset: 0,
            ident: None,
            partial: String::new(),
        };
        t.reopen(from_start)?;
        Ok(t)
    }

    fn reopen(&mut self, from_start: bool) -> io::Result<()> {
        let file = File::open(&self.path)?;
        let meta = file.metadata()?;
        self.ident = Some((meta.dev(), meta.ino()));
        self.offset = if from_start { 0 } else { meta.len() };
        self.partial.clear();
        self.file = Some(file);
        Ok(())
    }

    /// Whole lines appended since the last call. A trailing partial line is
    /// withheld until its newline arrives -- emitting half a log line would
    /// produce a parse result that is wrong rather than merely late.
    pub fn poll(&mut self) -> io::Result<Vec<String>> {
        // Has the file been replaced, or truncated below our read offset?
        match std::fs::metadata(&self.path) {
            Ok(meta) => {
                let ident = (meta.dev(), meta.ino());
                if Some(ident) != self.ident || meta.len() < self.offset {
                    // The file may be replaced again between this metadata
                    // call and `reopen`'s own `File::open` -- log rotation
                    // is exactly this race. A failure here is transient, not
                    // a reason to propagate and kill the daemon: `reopen`
                    // hasn't touched `self.file`/`self.ident` yet on this
                    // path, so they are left as they were and the next tick
                    // tries again.
                    if self.reopen(true).is_err() {
                        return Ok(Vec::new());
                    }
                }
            }
            Err(_) => return Ok(Vec::new()), // gone for now; try again next tick
        }

        let Some(file) = self.file.as_mut() else {
            return Ok(Vec::new());
        };
        file.seek(SeekFrom::Start(self.offset))?;

        let mut buf = Vec::new();
        let read = file.read_to_end(&mut buf)?;
        self.offset += read as u64;
        if read == 0 {
            return Ok(Vec::new());
        }

        // EverQuest logs are effectively ASCII, but never assume: lossy decode
        // keeps one odd byte from killing the daemon.
        self.partial.push_str(&String::from_utf8_lossy(&buf));

        // The fixture is 117 MB and 1.44 M lines; a per-line drain from the front of
        // a String holding the whole file would take hours. Split once instead.
        let mut lines = Vec::new();
        if let Some(last_newline) = self.partial.rfind('\n') {
            let rest = self.partial.split_off(last_newline + 1);
            let complete = std::mem::replace(&mut self.partial, rest);
            lines.extend(
                complete
                    .lines()
                    .map(|l| l.trim_end_matches('\r').to_string()),
            );
        }
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_log(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("wisp-tail-{}-{}.log", name, std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn append(path: &std::path::Path, text: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn reads_lines_appended_after_opening() {
        let path = temp_log("append");
        append(&path, "first\n");
        let mut t = Tailer::open(&path, false).unwrap();
        assert!(t.poll().unwrap().is_empty(), "from_start=false skips history");

        append(&path, "second\nthird\n");
        assert_eq!(t.poll().unwrap(), vec!["second", "third"]);
    }

    #[test]
    fn from_start_reads_existing_content() {
        let path = temp_log("fromstart");
        append(&path, "a\nb\n");
        let mut t = Tailer::open(&path, true).unwrap();
        assert_eq!(t.poll().unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn a_partial_line_is_withheld_until_its_newline_arrives() {
        let path = temp_log("partial");
        append(&path, "x\n");
        let mut t = Tailer::open(&path, true).unwrap();
        assert_eq!(t.poll().unwrap(), vec!["x"]);

        append(&path, "incomp");
        assert!(t.poll().unwrap().is_empty(), "half a line must not be emitted");
        append(&path, "lete\n");
        assert_eq!(t.poll().unwrap(), vec!["incomplete"]);
    }

    #[test]
    fn truncation_is_detected_and_reread_from_the_start() {
        let path = temp_log("truncate");
        append(&path, "one\ntwo\n");
        let mut t = Tailer::open(&path, true).unwrap();
        assert_eq!(t.poll().unwrap(), vec!["one", "two"]);

        std::fs::write(&path, b"fresh\n").unwrap();
        assert_eq!(t.poll().unwrap(), vec!["fresh"]);
    }

    #[test]
    fn a_transient_reopen_failure_during_rotation_does_not_error_the_poll() {
        // The file vanishing between wispd's poll ticks -- e.g. logrotate's
        // rename-then-recreate -- must not propagate an Err out of poll():
        // that would kill the daemon and lose every counter (spec §6:
        // "survives log truncation and file replacement without
        // restarting").
        let path = temp_log("rotate");
        append(&path, "one\n");
        let mut t = Tailer::open(&path, true).unwrap();
        assert_eq!(t.poll().unwrap(), vec!["one"]);

        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            t.poll().unwrap(),
            Vec::<String>::new(),
            "a missing file must poll empty, not Err"
        );

        append(&path, "two\n");
        assert_eq!(t.poll().unwrap(), vec!["two"], "recreated file is picked up on the next tick");
    }

    #[test]
    fn replacement_by_a_different_file_is_detected() {
        let path = temp_log("replace");
        append(&path, "old\n");
        let mut t = Tailer::open(&path, true).unwrap();
        assert_eq!(t.poll().unwrap(), vec!["old"]);

        std::fs::remove_file(&path).unwrap();
        append(&path, "brand new\n");
        assert_eq!(t.poll().unwrap(), vec!["brand new"]);
    }
}
