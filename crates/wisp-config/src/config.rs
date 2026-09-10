// SPDX-License-Identifier: MIT
//! The config file: five keys, one `key = value` per line.
//!
//! Not TOML and it does not claim to be — five keys do not justify a dependency,
//! and the project's hand-rolled argument parsing sets the precedent. A value is
//! everything after the first `=`, trimmed, so a path with spaces needs no
//! quoting: the game's own directories are full of them.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A key the config file may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    /// One log file, as `--log`.
    Log,
    /// The game's `Logs/` directory, as `--logs-dir`.
    LogsDir,
    /// The client install directory, as `--spells`.
    SpellsDir,
    /// As `--scale`.
    Scale,
    /// `gamescope`, `layer-shell` or `plain`, as `--backend`.
    Backend,
}

impl Key {
    /// Every key, in the order `wisp config show` prints them.
    pub const ALL: [Key; 5] = [Key::Log, Key::LogsDir, Key::SpellsDir, Key::Scale, Key::Backend];

    /// A key from its spelling in the file. A name Wisp does not have is not an
    /// error: it is collected in [`Config::unknown`] and skipped, so a config
    /// written for a newer Wisp still runs an older one.
    pub fn parse(name: &str) -> Option<Key> {
        match name {
            "log" => Some(Key::Log),
            "logs_dir" => Some(Key::LogsDir),
            "spells_dir" => Some(Key::SpellsDir),
            "scale" => Some(Key::Scale),
            "backend" => Some(Key::Backend),
            _ => None,
        }
    }

    /// The key's spelling in the file.
    pub fn name(&self) -> &'static str {
        match self {
            Key::Log => "log",
            Key::LogsDir => "logs_dir",
            Key::SpellsDir => "spells_dir",
            Key::Scale => "scale",
            Key::Backend => "backend",
        }
    }
}

/// A parsed config file.
///
/// Values are the raw text and there are deliberately no typed accessors:
/// parsing a value is the reader's business, because only the reader knows
/// whether it came from the file or from a flag, and an error has to blame the
/// right one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    values: BTreeMap<Key, String>,
    unknown: Vec<String>,
}

impl Config {
    /// `#` starts a comment only at the start of a line, after trimming leading
    /// whitespace, so a value may contain one. Blank lines and lines with no `=`
    /// are ignored, a repeated key keeps its last value, and an unknown key is
    /// collected rather than refused.
    pub fn parse(text: &str) -> Config {
        let mut config = Config::default();
        for line in text.lines() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            match Key::parse(name) {
                Some(key) => {
                    config.values.insert(key, value.trim().to_string());
                }
                None => {
                    if !config.unknown.iter().any(|seen| seen == name) {
                        config.unknown.push(name.to_string());
                    }
                }
            }
        }
        config
    }

    /// The file at `path`, or an empty config when there is no such file: the
    /// user has not written one yet and Wisp runs on its defaults. Any other
    /// `io::Error` — a file that is not valid UTF-8, say — is the caller's to
    /// report once on stderr and then treat as empty.
    pub fn load(path: &Path) -> io::Result<Config> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Config::parse(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e),
        }
    }

    /// The value exactly as the file spelled it, for the reader to parse.
    pub fn get(&self, key: Key) -> Option<&str> {
        self.values.get(&key).map(String::as_str)
    }

    /// The value as a path, its spaces and vendor names intact.
    pub fn path_value(&self, key: Key) -> Option<PathBuf> {
        self.values.get(&key).map(PathBuf::from)
    }

    /// Names in the file that are not keys Wisp has, in order of first
    /// appearance and once each, for the binaries to report on stderr.
    pub fn unknown(&self) -> &[String] {
        &self.unknown
    }
}

/// `text` with `key` set to `value`.
///
/// The key's own line is replaced where it stands and appended when the file has
/// none; every other line, comments included, is kept byte for byte. Line
/// endings are the one exception: the result is normalised to LF, so a CRLF file
/// is rewritten with LF. It always ends with exactly one newline, so a file
/// `wisp config set` has written is a file it can read back. A second line for
/// the same key goes: one key, one line.
///
/// Values are not validated here. `wisp config set` checks the key, not the
/// value, so `backend = nonsense` is written and `wisp-hud` refuses it at start
/// exactly as it refuses `--backend nonsense`.
pub fn set_in_text(text: &str, key: Key, value: &str) -> String {
    let name = key.name();
    let line = format!("{name} = {value}\n");
    let mut out = String::with_capacity(text.len() + line.len());
    let mut set = false;
    for existing in text.lines() {
        let is_the_key = !existing.trim_start().starts_with('#')
            && existing
                .split_once('=')
                .is_some_and(|(found, _)| found.trim() == name);
        if is_the_key {
            if !set {
                out.push_str(&line);
                set = true;
            }
        } else {
            out.push_str(existing);
            out.push('\n');
        }
    }
    if !set {
        out.push_str(&line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;
    use std::path::PathBuf;

    /// A file of its own per test, since the harness runs them in parallel
    /// inside one process.
    fn scratch_file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wisp-config-config-{}-{tag}", std::process::id()))
    }

    #[test]
    fn every_key_round_trips_through_its_file_spelling() {
        // The spec's table order, which is the order `wisp config show` prints.
        assert_eq!(
            Key::ALL,
            [Key::Log, Key::LogsDir, Key::SpellsDir, Key::Scale, Key::Backend]
        );
        for key in Key::ALL {
            assert_eq!(Key::parse(key.name()), Some(key), "{}", key.name());
        }
        assert_eq!(Key::parse("nonsense"), None);
        assert_eq!(Key::parse("LOG"), None, "keys are case-sensitive");
        assert_eq!(Key::parse("logs-dir"), None, "the file spells it with an underscore");
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let c = Config::parse("# Wisp\n\n   # an indented comment\n# scale = 2\nlog = /a\n");
        assert_eq!(c.get(Key::Log), Some("/a"));
        assert_eq!(c.get(Key::Scale), None, "a commented-out key is not a value");
        assert_eq!(c.unknown(), &[] as &[String]);
    }

    #[test]
    fn a_value_keeps_its_spaces() {
        let c = Config::parse("logs_dir = /a path/with spaces/Logs\n");
        assert_eq!(c.get(Key::LogsDir), Some("/a path/with spaces/Logs"));
        assert_eq!(
            c.path_value(Key::LogsDir),
            Some(PathBuf::from("/a path/with spaces/Logs"))
        );
    }

    #[test]
    fn only_the_first_equals_sign_splits() {
        let c = Config::parse("backend = a=b\n");
        assert_eq!(c.get(Key::Backend), Some("a=b"));
    }

    #[test]
    fn values_are_trimmed() {
        let c = Config::parse("   scale   =   1.5   \n");
        assert_eq!(c.get(Key::Scale), Some("1.5"));
    }

    #[test]
    fn unknown_keys_are_collected_and_skipped() {
        let c = Config::parse("nonsense = 1\nlog = /a\nalso_unknown = 2\nnonsense = 3\n");
        assert_eq!(
            c.unknown(),
            &["nonsense".to_string(), "also_unknown".to_string()],
            "in order of first appearance, once each"
        );
        assert_eq!(c.get(Key::Log), Some("/a"));
    }

    #[test]
    fn a_repeated_key_keeps_the_last() {
        let c = Config::parse("log = /first\nlog = /second\n");
        assert_eq!(c.get(Key::Log), Some("/second"));
    }

    #[test]
    fn a_line_without_an_equals_sign_is_ignored() {
        let c = Config::parse("log /a\nscale = 1.5\n");
        assert_eq!(c.get(Key::Log), None);
        assert_eq!(c.get(Key::Scale), Some("1.5"));
        assert_eq!(c.unknown(), &[] as &[String], "a keyless line is not an unknown key");
    }

    #[test]
    fn a_missing_file_loads_empty() {
        let path = scratch_file("missing");
        let _ = fs::remove_file(&path);
        let c = Config::load(&path).expect("a config file the user has not written yet");
        assert_eq!(c, Config::default());
        assert_eq!(c.get(Key::Log), None);
    }

    #[test]
    fn a_non_utf8_file_is_an_error() {
        let path = scratch_file("non-utf8");
        // 0xff is not a valid UTF-8 leading byte. Each binary reports this once
        // on stderr and treats the config as empty.
        fs::write(&path, b"log = \xff\xfe\n").unwrap();
        let err = Config::load(&path).expect_err("the file is not text");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_loaded_file_is_the_parsed_text() {
        let path = scratch_file("loaded");
        fs::write(&path, "# written by wisp config set\nlogs_dir = /a path/Logs\n").unwrap();
        let c = Config::load(&path).unwrap();
        assert_eq!(c.get(Key::LogsDir), Some("/a path/Logs"));
        assert_eq!(c, Config::parse("# written by wisp config set\nlogs_dir = /a path/Logs\n"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn get_returns_the_raw_text_verbatim() {
        // The reader parses. An unusable value comes back exactly as written so
        // the reader can refuse it and blame the config file rather than a flag.
        let c = Config::parse("scale = not-a-number\nbackend = nonsense\n");
        assert_eq!(c.get(Key::Scale), Some("not-a-number"));
        assert_eq!(c.get(Key::Backend), Some("nonsense"));
        assert_eq!(c.get(Key::Log), None);
    }

    #[test]
    fn path_value_is_the_raw_text_as_a_path() {
        let c = Config::parse("spells_dir = /games/Daybreak Game Company/Installed Games\n");
        assert_eq!(
            c.path_value(Key::SpellsDir),
            Some(PathBuf::from("/games/Daybreak Game Company/Installed Games"))
        );
        assert_eq!(c.path_value(Key::Log), None, "an unset key has no path");
    }

    #[test]
    fn set_replaces_a_key_in_place() {
        assert_eq!(set_in_text("log = /a\nscale = 1\n", Key::Log, "/b"), "log = /b\nscale = 1\n");
    }

    #[test]
    fn set_appends_when_the_key_is_absent() {
        assert_eq!(set_in_text("scale = 1\n", Key::Log, "/b"), "scale = 1\nlog = /b\n");
    }

    #[test]
    fn set_preserves_comments_and_other_keys_byte_for_byte() {
        let text = "# Wisp\n# log = /commented-out\n\nlog = /a\n\nscale = 1  # not a comment\n";
        let out = set_in_text(text, Key::Log, "/b");
        assert_eq!(
            out,
            "# Wisp\n# log = /commented-out\n\nlog = /b\n\nscale = 1  # not a comment\n"
        );
        assert_eq!(out.lines().zip(text.lines()).filter(|(a, b)| a != b).count(), 1);
        // A commented-out key is not the key, so it is left alone and the live
        // one is appended.
        assert_eq!(set_in_text("# log = /a\n", Key::Log, "/b"), "# log = /a\nlog = /b\n");
    }

    #[test]
    fn set_ends_with_exactly_one_newline() {
        // The terminator is added when the input lacked one and never doubled
        // when it had one, whether the key was replaced or appended.
        assert_eq!(set_in_text("log = /a", Key::Log, "/b"), "log = /b\n");
        assert_eq!(set_in_text("log = /a\n", Key::Log, "/b"), "log = /b\n");
        assert_eq!(set_in_text("scale = 1", Key::Log, "/b"), "scale = 1\nlog = /b\n");
        assert_eq!(set_in_text("scale = 1\n", Key::Log, "/b"), "scale = 1\nlog = /b\n");
        assert_eq!(set_in_text("", Key::Log, "/b"), "log = /b\n");
    }

    #[test]
    fn set_then_parse_reads_back_the_value_that_was_set() {
        for key in Key::ALL {
            let text = "# Wisp\nlog = /a\nscale = 1\n";
            let out = set_in_text(text, key, "/set by the test");
            let c = Config::parse(&out);
            assert_eq!(c.get(key), Some("/set by the test"), "{}", key.name());
            assert_eq!(c.unknown(), &[] as &[String]);
        }
        // A repeated key becomes one line, so the file never grows a duplicate.
        let out = set_in_text("log = /a\nlog = /b\n", Key::Log, "/c");
        assert_eq!(out, "log = /c\n");
        assert_eq!(Config::parse(&out).get(Key::Log), Some("/c"));
    }
}
