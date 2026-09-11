// SPDX-License-Identifier: MIT
//! The config file: TOML, with a `[hud]` table and `[[block]]` array for
//! Spec 5's layout, plus the four path/backend keys from Spec 4.
//!
//! `Config::parse` never fails: a document is read as TOML first, and only
//! when it is not valid TOML at all does it fall back to Spec 4's flat
//! `key = value` grammar (values unquoted, `#` comments at line start) — a
//! file written before Spec 5 still reads. `to_toml` is a full regeneration
//! from the parsed model rather than a text edit: a comment in the user's
//! file is not preserved, and that loss is accepted by the spec in exchange
//! for one writer that both `wisp config set` and the HUD's own save (a
//! later task) can share.
//!
//! Because that regeneration is lossy by construction, no writer calls
//! `to_toml` directly: [`Config::to_toml_checked`] reads its own output back
//! and refuses to hand over text that did not come back as the config it was
//! given. A full regeneration with no post-condition is how a mistyped anchor
//! or a stray top-level key turned into a silently deleted layout.

use crate::layout::{Block, Hud, Layout};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
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
    /// As `--scale`. Text semantics: the `[hud] scale` value exactly as
    /// written — a float prints as `1.5`, a string (an invalid scale kept so
    /// the HUD can refuse it and blame the file) prints as itself — or, for a
    /// file still in the Spec 4 flat grammar, the converted multiplier
    /// formatted to two decimals; see [`Config::converted_scale`].
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

/// An unknown top-level entry's value, kept so `to_toml` can re-emit it
/// rather than silently drop a typo.
#[derive(Debug, Clone, PartialEq)]
enum UnknownValue {
    /// Came from a TOML document: re-emitted with its own TOML type.
    Toml(toml::Value),
    /// Came from the legacy grammar, which only ever has text: re-emitted as
    /// a quoted string.
    Text(String),
}

impl UnknownValue {
    /// The value as TOML would carry it. Legacy text has only ever been text,
    /// so it re-emits as a string.
    fn as_toml(&self) -> toml::Value {
        match self {
            UnknownValue::Toml(value) => value.clone(),
            UnknownValue::Text(text) => toml::Value::String(text.clone()),
        }
    }

    /// Whether TOML writes this value under a header of its own
    /// (`[name]`/`[[name]]`) rather than as a bare `name = value` line. It
    /// decides *where* in the file the entry is re-emitted: a bare key has to
    /// go before the first table header or the next read swallows it into
    /// that table, while a table has to go after the blocks or it would
    /// swallow them.
    fn needs_own_header(&self) -> bool {
        match self.as_toml() {
            toml::Value::Table(_) => true,
            toml::Value::Array(items) => !items.is_empty() && items.iter().all(|item| item.is_table()),
            _ => false,
        }
    }

    /// `name = value`, or the whole `[name]` section, as a TOML document
    /// fragment. Serialized by the `toml` crate rather than formatted here,
    /// so a name that needs quoting gets it and a nested table keeps its full
    /// dotted path.
    fn to_toml_entry(&self, name: &str) -> String {
        let mut doc = toml::Table::new();
        doc.insert(name.to_string(), self.as_toml());
        match toml::to_string(&doc) {
            Ok(text) => text,
            // Not reachable for anything `Config::parse` can produce; the
            // round-trip guard in `to_toml_checked` catches it if it ever is.
            Err(_) => format!("{name} = {}\n", self.as_toml()),
        }
    }
}

/// Why a write was refused.
///
/// Not a parse error — [`Config::parse`] never fails — but a post-condition
/// on [`Config::to_toml`]: what comes out has to read back as the config that
/// went in. Reaching one of these means Wisp generated a file it cannot read,
/// which is Wisp's bug rather than a mistake in the user's file, and the
/// message says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// `Config::parse(&to_toml())` is not the config it was meant to be. The
    /// text names the part that differed.
    NotRoundTrip(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::NotRoundTrip(detail) => write!(
                f,
                "the regenerated config did not round-trip ({detail}); \
                 this is a bug in Wisp rather than a mistake in the file, so nothing was written"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The `[hud]` table read leniently: `scale` is kept as a raw [`toml::Value`]
/// rather than `f32`, because an invalid scale (a string, say) must reach
/// [`Config::get`] rather than fail the whole document — the HUD's own
/// refusal handles it, not the parser.
#[derive(Debug, Deserialize)]
struct RawHud {
    #[serde(default = "raw_scale_default")]
    scale: toml::Value,
    #[serde(default = "default_chord")]
    chord: String,
    #[serde(default)]
    output: Option<String>,
}

fn raw_scale_default() -> toml::Value {
    toml::Value::Float(1.0)
}

fn default_chord() -> String {
    crate::layout::DEFAULT_CHORD.to_string()
}

/// A parsed config file.
///
/// The four path/name keys and `Key::Scale`'s display text are raw text —
/// parsing a value is the reader's business, because only the reader knows
/// whether it came from the file or from a flag, and an error has to blame
/// the right one. The layout (`[hud]`, `[[block]]`) is typed, because Spec 5
/// binaries read it structurally rather than key by key.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    values: BTreeMap<Key, String>,
    unknown_names: Vec<String>,
    unknown_values: Vec<(String, UnknownValue)>,
    layout: Layout,
    layout_error: Option<String>,
    converted_scale: Option<(String, f32)>,
}

impl Default for Config {
    /// The config nobody has written yet: every key unset, the default
    /// layout, no error.
    fn default() -> Config {
        Config::parse("")
    }
}

impl Config {
    /// Never fails. A document is read as TOML first; when the text is not
    /// TOML at all, it is read with Spec 4's flat `key = value` grammar
    /// (values unquoted), marked legacy.
    pub fn parse(text: &str) -> Config {
        match toml::from_str::<toml::Table>(text) {
            Ok(table) => Config::from_toml(table),
            Err(_) => Config::parse_legacy(text),
        }
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

    /// The value exactly as the file spelled it, for the reader to parse. See
    /// [`Key::Scale`] for that key's particular text semantics.
    pub fn get(&self, key: Key) -> Option<&str> {
        self.values.get(&key).map(String::as_str)
    }

    /// The value as a path, its spaces and vendor names intact.
    ///
    /// A key present but empty is no path at all. `logs_dir =` with nothing
    /// after it is an unfinished edit, and a reader that acted on it would go
    /// looking for a directory named "" — so it falls through to whatever the
    /// precedence rules try next, exactly as an absent key does.
    pub fn path_value(&self, key: Key) -> Option<PathBuf> {
        self.values
            .get(&key)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }

    /// Names in the file that are not keys Wisp has — `hud` and `block` are
    /// known — in order of first appearance and once each, for the binaries
    /// to report on stderr.
    pub fn unknown(&self) -> &[String] {
        &self.unknown_names
    }

    /// The HUD's layout: the last one that parsed without error.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The layout, for a caller building one up before writing it back.
    pub fn layout_mut(&mut self) -> &mut Layout {
        &mut self.layout
    }

    /// The layout-parse error, if the TOML parsed but `[hud]`/`[[block]]` did
    /// not (a string where a number belongs, an unknown anchor): the message
    /// names the key. The HUD keeps its last good layout and prints this
    /// once. A `[hud] scale` that is a string is not this error — see
    /// [`Key::Scale`].
    pub fn layout_error(&self) -> Option<&str> {
        self.layout_error.as_deref()
    }

    /// `Some((px_text, factor))` when a legacy top-level `scale` — Spec 4's
    /// font-size flag — was converted to a multiplier: `factor = px / 13.0`.
    /// `None` for a TOML file, or a legacy file with no `scale` line, or one
    /// whose value did not parse as a number.
    pub fn converted_scale(&self) -> Option<(&str, f32)> {
        self.converted_scale.as_ref().map(|(text, factor)| (text.as_str(), *factor))
    }

    /// Set `key`'s value. `Key::Scale` also updates [`Layout::hud`]'s `scale`
    /// when `value` parses as a number; a value that does not still reaches
    /// `to_toml` as a quoted string, so a bad value the user wrote survives
    /// through to the HUD's refusal.
    pub fn set(&mut self, key: Key, value: &str) {
        if key == Key::Scale {
            if let Ok(scale) = value.trim().parse::<f32>() {
                self.layout.hud.scale = scale;
            }
        }
        self.values.insert(key, value.to_string());
    }

    /// The whole file as TOML: the four keys and every unknown *bare* key
    /// (where set), then `[hud]`, every `[[block]]`, and last any unknown
    /// entry that is a table of its own. A legacy top-level `scale` is not
    /// re-emitted; it became `hud.scale`.
    ///
    /// The unknown bare keys go *before* the first table header deliberately.
    /// In TOML a bare key written after `[hud]` or `[[block]]` belongs to
    /// that table, so re-emitting a top-level key after them moves it into
    /// the last block on the next read — where serde drops it silently, or,
    /// when its name collides with one of `Block`'s own fields, makes the
    /// document a duplicate-key error and sends the whole file down the
    /// legacy fallback with every layout key reported unknown. The
    /// re-emission exists so a typo is not dropped; emitted in the wrong
    /// place it dropped the typo and the file with it.
    ///
    /// Prefer [`Config::to_toml_checked`] for anything that actually writes:
    /// this is a full regeneration with no post-condition of its own.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        for key in [Key::Log, Key::LogsDir, Key::SpellsDir, Key::Backend] {
            if let Some(value) = self.get(key) {
                out.push_str(&format!("{} = {}\n", key.name(), quote(value)));
            }
        }
        for (name, value) in &self.unknown_values {
            if !value.needs_own_header() {
                out.push_str(&value.to_toml_entry(name));
            }
        }
        out.push('\n');
        out.push_str(&self.layout_toml());
        for (name, value) in &self.unknown_values {
            if value.needs_own_header() {
                out.push('\n');
                out.push_str(&value.to_toml_entry(name));
            }
        }
        out
    }

    /// [`Config::to_toml`]'s output, having first read it back and checked
    /// that it says the same thing — the post-condition every writer needs
    /// and none of them can state for itself.
    ///
    /// `to_toml` regenerates the whole file from the parsed model, so any
    /// hole in either half of that round trip loses part of the user's file
    /// rather than one key: a layout the writer emits in a shape the reader
    /// will not accept, an unknown key that lands somewhere the reader will
    /// not look, a float with no TOML spelling. Checked here once, in the
    /// crate that owns both halves, so `wisp config set`, `wisp hud` and the
    /// HUD's own save cannot each get it differently right.
    ///
    /// What must match: the four path/backend keys as written, the layout's
    /// blocks, chord and output, the `[hud] scale` *as the file spells it*,
    /// and the set of unknown key names. Not the layout's `scale` float:
    /// converting Spec 4's pixel size reports two decimals
    /// (`Config::converted_scale`), and the two-decimal text is what every
    /// reader of the file then uses, so `48 / 13.0` legitimately writes back
    /// as `3.69`.
    pub fn to_toml_checked(&self) -> Result<String, ConfigError> {
        let text = self.to_toml();
        let back = Config::parse(&text);
        let refuse = |detail: &str| Err(ConfigError::NotRoundTrip(detail.to_string()));

        if let Some(e) = back.layout_error() {
            return refuse(&format!("the layout it wrote does not read back: {e}"));
        }
        for key in [Key::Log, Key::LogsDir, Key::SpellsDir, Key::Backend] {
            if back.get(key) != self.get(key) {
                return refuse(&format!("{} read back as {:?}", key.name(), back.get(key)));
            }
        }
        if back.layout.blocks != self.layout.blocks {
            return refuse("the blocks read back are not the ones written");
        }
        if back.layout.hud.chord != self.layout.hud.chord {
            return refuse("hud.chord read back as something else");
        }
        if back.layout.hud.output != self.layout.hud.output {
            return refuse("hud.output read back as something else");
        }
        if back.scale_toml() != self.scale_toml() {
            return refuse(&format!("hud.scale read back as {}", back.scale_toml()));
        }
        if back.unknown_key_names() != self.unknown_key_names() {
            return refuse(&format!(
                "unknown keys {:?} read back as {:?}",
                self.unknown_key_names(),
                back.unknown_key_names()
            ));
        }
        Ok(text)
    }

    /// Every unknown name once, sorted: what the round-trip guard compares,
    /// since the legacy grammar keeps them in the order the file listed them
    /// and a TOML document sorts them.
    fn unknown_key_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.unknown_values.iter().map(|(name, _)| name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// `[hud]` plus every `[[block]]`, in the shape `to_toml` appends after
    /// the four keys.
    fn layout_toml(&self) -> String {
        let mut out = String::new();
        out.push_str("[hud]\n");
        out.push_str(&format!("scale = {}\n", self.scale_toml()));
        out.push_str(&format!("chord = {}\n", quote(&self.layout.hud.chord)));
        if let Some(output) = &self.layout.hud.output {
            out.push_str(&format!("output = {}\n", quote(output)));
        }
        if !self.layout.blocks.is_empty() {
            #[derive(serde::Serialize)]
            struct BlocksOnly<'a> {
                block: &'a [Block],
            }
            out.push('\n');
            let blocks = toml::to_string(&BlocksOnly { block: &self.layout.blocks })
                .expect("a Layout's blocks always serialize");
            out.push_str(&blocks);
        }
        out
    }

    /// `[hud] scale`'s TOML literal: a bare number when the current text
    /// (`Key::Scale`'s value, or the layout's own scale when unset) parses as
    /// one, a quoted string otherwise — the same rule `Config::parse` reads
    /// back with, so a bad value round-trips.
    fn scale_toml(&self) -> String {
        match self.values.get(&Key::Scale) {
            Some(text) => match text.trim().parse::<f32>() {
                Ok(f) => fmt_f32(f),
                Err(_) => quote(text),
            },
            None => fmt_f32(self.layout.hud.scale),
        }
    }

    /// `#` starts a comment only at the start of a line, after trimming
    /// leading whitespace, so a value may contain one. Blank lines and lines
    /// with no `=` are ignored, a repeated key keeps its last value, and an
    /// unknown key is collected rather than refused.
    fn parse_legacy(text: &str) -> Config {
        let mut values: BTreeMap<Key, String> = BTreeMap::new();
        let mut unknown_names: Vec<String> = Vec::new();
        let mut unknown_values: Vec<(String, UnknownValue)> = Vec::new();
        let mut raw_scale: Option<String> = None;

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
            let value = value.trim().to_string();
            match Key::parse(name) {
                Some(Key::Scale) => raw_scale = Some(value),
                Some(key) => {
                    values.insert(key, value);
                }
                None => {
                    if let Some(entry) = unknown_values.iter_mut().find(|(seen, _)| seen == name) {
                        entry.1 = UnknownValue::Text(value);
                    } else {
                        unknown_names.push(name.to_string());
                        unknown_values.push((name.to_string(), UnknownValue::Text(value)));
                    }
                }
            }
        }

        let mut layout = Layout::default_layout();
        let converted_scale = raw_scale.and_then(|raw| apply_top_level_scale(raw, &mut layout, &mut values));

        Config { values, unknown_names, unknown_values, layout, layout_error: None, converted_scale }
    }

    /// A document that parsed as TOML syntax, however loosely it matches the
    /// schema Spec 5 expects.
    fn from_toml(mut table: toml::Table) -> Config {
        let mut values: BTreeMap<Key, String> = BTreeMap::new();
        for key in [Key::Log, Key::LogsDir, Key::SpellsDir, Key::Backend] {
            if let Some(value) = table.remove(key.name()) {
                values.insert(key, value_as_written(&value));
            }
        }

        // A bare top-level `scale`, the Spec 4 pixel value, may sit beside
        // an otherwise-Spec-5 document — exactly the file Spec 4's own
        // writer produced. It converts to the HUD's multiplier the same way
        // `parse_legacy`'s does, whether or not the rest of the document
        // happens to be valid TOML; an explicit `[hud] scale` wins over it
        // silently, since that is Spec 5's own key for the same thing.
        let top_scale = table.remove("scale").as_ref().map(value_as_written);

        let hud_entry = table.remove("hud");
        let block_entry = table.remove("block");

        let mut layout_error: Option<String> = None;

        let hud_scale_explicit =
            hud_entry.as_ref().and_then(|v| v.as_table()).and_then(|t| t.get("scale")).cloned();

        let raw_hud: Option<RawHud> = match hud_entry {
            Some(value) => match RawHud::deserialize(value) {
                Ok(hud) => Some(hud),
                Err(e) => {
                    layout_error = Some(format!("hud: {e}"));
                    None
                }
            },
            None => None,
        };

        // Deserialized one entry at a time rather than as a `Vec<Block>`, so
        // the message can name the block that is wrong. Once a writer refuses
        // to rewrite a layout it could not read, this message is the only
        // thing the user has to find the bad key with, and "unknown variant
        // `bottm-left`" in a file with six blocks is not enough to act on.
        let blocks: Option<Vec<Block>> = match block_entry {
            Some(toml::Value::Array(items)) => {
                let mut parsed = Vec::with_capacity(items.len());
                let mut failed = false;
                for (index, item) in items.into_iter().enumerate() {
                    match Block::deserialize(item) {
                        Ok(block) => parsed.push(block),
                        Err(e) => {
                            if layout_error.is_none() {
                                layout_error = Some(format!("[[block]] {index}: {e}"));
                            }
                            failed = true;
                            break;
                        }
                    }
                }
                if failed {
                    None
                } else {
                    Some(parsed)
                }
            }
            // `block` written as something other than an array of tables:
            // one message for the whole key, since there is no index to name.
            Some(other) => {
                if layout_error.is_none() {
                    layout_error = Some(format!("block: expected an array of [[block]] tables, found {}", other.type_str()));
                }
                None
            }
            None => None,
        };

        let mut layout = if layout_error.is_some() {
            Layout::default_layout()
        } else {
            let mut layout = Layout::default_layout();
            if let Some(raw_hud) = &raw_hud {
                layout.hud = resolve_hud(raw_hud);
            }
            if let Some(blocks) = blocks {
                layout.blocks = blocks;
            }
            layout
        };

        // `[hud] scale` wins silently over a bare top-level `scale`; failing
        // that, the top-level one converts like a legacy file's; failing
        // that, a `[hud]` table with no `scale` field of its own reports its
        // (default) resolved value.
        let converted_scale = if let Some(explicit) = &hud_scale_explicit {
            values.insert(Key::Scale, value_as_written(explicit));
            None
        } else if let Some(raw) = top_scale {
            apply_top_level_scale(raw, &mut layout, &mut values)
        } else {
            if let Some(raw_hud) = &raw_hud {
                values.insert(Key::Scale, value_as_written(&raw_hud.scale));
            }
            None
        };

        let mut unknown_names = Vec::new();
        let mut unknown_values = Vec::new();
        for (name, value) in table {
            unknown_names.push(name.clone());
            unknown_values.push((name, UnknownValue::Toml(value)));
        }

        Config { values, unknown_names, unknown_values, layout, layout_error, converted_scale }
    }
}

/// A top-level `scale` — Spec 4's font-point-size flag — found in either
/// parse path: the legacy grammar, or a bare key sitting in an otherwise
/// valid TOML document (exactly the file Spec 4's own writer produced for a
/// user who only ever ran `wisp config set scale <px>`). Shared so both
/// paths convert it the same way: a number becomes the HUD's multiplier
/// (`px / 13.0`, reported through [`Config::converted_scale`] and
/// [`Config::get`]'s two-decimal text); anything else is kept as raw text so
/// the HUD's own refusal can name it, with no conversion and no layout
/// change.
fn apply_top_level_scale(
    raw: String,
    layout: &mut Layout,
    values: &mut BTreeMap<Key, String>,
) -> Option<(String, f32)> {
    match raw.trim().parse::<f32>() {
        Ok(px) => {
            let factor = px / 13.0;
            layout.hud.scale = factor;
            values.insert(Key::Scale, format!("{factor:.2}"));
            Some((raw, factor))
        }
        Err(_) => {
            values.insert(Key::Scale, raw);
            None
        }
    }
}

fn resolve_hud(raw: &RawHud) -> Hud {
    let scale = match &raw.scale {
        toml::Value::Integer(i) => *i as f32,
        toml::Value::Float(f) => *f as f32,
        // Not a number: not a layout error (the HUD's own refusal handles
        // it), just the default multiplier.
        _ => 1.0,
    };
    Hud { scale, chord: raw.chord.clone(), output: raw.output.clone() }
}

/// A scalar TOML value the way `Config::get` reports it: a string as itself
/// (unquoted — the caller already knows it is text), everything else as TOML
/// would print it (a float keeps its decimal point, an integer and a bool
/// print bare).
fn value_as_written(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A basic-string TOML literal: quoted, with `"` and `\` escaped. Good enough
/// for the paths and names Wisp's own keys hold; not a general TOML encoder.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `f` as a TOML float literal: TOML requires a fractional part or exponent,
/// so a whole number gets `.0` appended (Rust's own `Display` would print
/// `1` for `1.0`).
///
/// The non-finite arm exists only so this function cannot produce text that
/// is not TOML at all: Rust spells them `NaN`, `inf` and `-inf`, and TOML
/// spells them `nan`, `inf` and `-inf`, so `NaN` written verbatim made the
/// whole file unreadable on the next open — every key reported unknown, the
/// paths carrying their own quote characters, the layout gone. No caller
/// should reach it: [`is_valid_scale`] is the one check `wisp config set
/// scale` and `wisp hud scale` share, and it refuses a non-finite value
/// before it can be stored.
fn fmt_f32(f: f32) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f.is_sign_negative() { "-inf".to_string() } else { "inf".to_string() };
    }
    let text = format!("{f}");
    if text.contains(['.', 'e', 'E']) {
        text
    } else {
        format!("{text}.0")
    }
}

/// Whether `factor` is a scale the HUD can render at: finite and greater than
/// zero.
///
/// One function rather than a check per entry point. `wisp hud scale` had
/// `factor > 0.0`, which refuses `NaN` (every comparison with it is false)
/// but accepts `inf`; `wisp config set scale` had no check at all and wrote
/// `scale = NaN`, which is not valid TOML and cost the user the rest of the
/// file on the next read. Both now ask here.
pub fn is_valid_scale(factor: f32) -> bool {
    factor.is_finite() && factor > 0.0
}

/// `text` with `key` set to `value`: `Config::parse(text)` → `set` →
/// [`Config::to_toml_checked`], for a caller that has the whole file as text
/// rather than a `Config` open across the two.
///
/// Checked rather than raw: an unchecked one-call convenience beside a
/// guarded writer is how a writer ends up unguarded. Note that it says
/// nothing about `layout_error` — a caller that is *writing* must refuse that
/// itself, with a message naming the block, before it ever gets here.
pub fn set_in_text(text: &str, key: Key, value: &str) -> Result<String, ConfigError> {
    let mut config = Config::parse(text);
    config.set(key, value);
    config.to_toml_checked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Anchor;
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
        // Valid TOML on its own (a bare top-level `scale`), so this exercises
        // the TOML path rather than the legacy fallback; a bare top-level
        // `scale` converts the same way in either path (it is Spec 4's pixel
        // value, wherever it turns up), so the text comes back as the
        // converted multiplier, not "1.5" itself.
        let c = Config::parse("   scale   =   1.5   \n");
        assert_eq!(c.get(Key::Scale), Some("0.12"));
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
        // "log /a" has no `=`, so the whole document is not valid TOML and
        // this is the legacy grammar: its `scale` is now Spec 5's converted
        // multiplier (1.5 / 13), not the raw "1.5" Spec 4 read back.
        let c = Config::parse("log /a\nscale = 1.5\n");
        assert_eq!(c.get(Key::Log), None);
        assert_eq!(c.get(Key::Scale), Some("0.12"), "a legacy scale is converted to a multiplier");
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
    fn an_empty_path_value_is_no_path_at_all() {
        let c = Config::parse("logs_dir =\nlog =    \nspells_dir = /games/EverQuest Legends\n");
        // `get` still reports the key as present, because that is the file's
        // own text; only the path accessor refuses it, so a precedence chain
        // moves on to the next source instead of resolving a directory named "".
        assert_eq!(c.get(Key::LogsDir), Some(""));
        assert_eq!(c.path_value(Key::LogsDir), None);
        assert_eq!(c.path_value(Key::Log), None, "whitespace alone trims to empty");
        assert_eq!(
            c.path_value(Key::SpellsDir),
            Some(PathBuf::from("/games/EverQuest Legends")),
            "a real value beside the empty ones is untouched"
        );
    }

    #[test]
    fn set_then_parse_reads_back_the_value_that_was_set() {
        for key in Key::ALL {
            let text = "# Wisp\nlog = /a\nscale = 1\n";
            let out = set_in_text(text, key, "/set by the test").unwrap();
            let c = Config::parse(&out);
            assert_eq!(c.get(key), Some("/set by the test"), "{}", key.name());
            assert_eq!(c.unknown(), &[] as &[String]);
        }
        // A repeated key becomes one line, so the file never grows a
        // duplicate — checked by parsing back rather than the exact text,
        // since `to_toml` regenerates the whole file rather than editing it
        // in place (Spec 5).
        let out = set_in_text("log = /a\nlog = /b\n", Key::Log, "/c").unwrap();
        assert_eq!(Config::parse(&out).get(Key::Log), Some("/c"));
    }

    // --- Spec 5: TOML, the layout model, one atomic writer ---

    #[test]
    fn a_legacy_file_still_parses_and_its_scale_is_converted() {
        let c = Config::parse("logs_dir = /a path/with spaces/Logs\nscale = 48\n");
        assert_eq!(c.get(Key::LogsDir), Some("/a path/with spaces/Logs"));
        assert_eq!(c.converted_scale().map(|(px, _)| px), Some("48"));
        assert!((c.layout().hud.scale - 48.0 / 13.0).abs() < 1e-6);
        assert_eq!(c.get(Key::Scale), Some("3.69"));
        assert_eq!(c.layout().blocks.len(), 2, "the default layout");
        assert_eq!(c.layout_error(), None);
    }

    #[test]
    fn a_bare_toml_scale_converts_like_a_legacy_one() {
        // `"scale = 48\n"` is valid TOML on its own (a bare top-level key),
        // but a top-level `scale` is always Spec 4's pixel value, whichever
        // parse path reached it: this is exactly the file Spec 4's own
        // writer produced for a user who only ever ran
        // `wisp config set scale 48`, and it must not silently lose the
        // scale on upgrade.
        let c = Config::parse("scale = 48\n");
        let (px, factor) = c.converted_scale().expect("a bare top-level scale converts");
        assert_eq!(px, "48");
        assert!((factor - 48.0 / 13.0).abs() < 1e-6);
        assert_eq!(c.get(Key::Scale), Some("3.69"));
        assert!((c.layout().hud.scale - 48.0 / 13.0).abs() < 1e-6);
        let out = c.to_toml();
        assert!(out.contains("[hud]\nscale = 3.69"), "{out}");
        assert!(!out.contains("\nscale = 48"), "the top-level scale is not re-emitted: {out}");

        // An explicit `[hud] scale` wins over a bare top-level one, silently:
        // Spec 5's own key beats a leftover Spec 4 one, with no conversion
        // and no trace of the dropped value.
        let c = Config::parse("scale = 48\n\n[hud]\nscale = 1.5\n");
        assert_eq!(c.get(Key::Scale), Some("1.5"), "[hud] scale wins");
        assert_eq!(c.converted_scale(), None, "the top-level one is dropped silently");
    }

    #[test]
    fn a_toml_file_round_trips_through_to_toml() {
        let text = "log = \"/l\"\n\n[hud]\nscale = 1.5\nchord = \"ctrl+shift+grave\"\n\n[[block]]\nkind = \"timers\"\nanchor = \"bottom-right\"\noffset = [20, 20]\n";
        let c = Config::parse(text);
        assert_eq!(c.get(Key::Log), Some("/l"));
        assert_eq!(c.get(Key::Scale), Some("1.5"));
        assert_eq!(c.layout().blocks[0].anchor, Anchor::BottomRight);
        assert_eq!(Config::parse(&c.to_toml()), c);
    }

    #[test]
    fn a_bad_scale_string_reaches_the_reader_and_a_bad_block_is_a_layout_error() {
        let c = Config::parse("[hud]\nscale = \"not-a-number\"\n");
        assert_eq!(c.get(Key::Scale), Some("not-a-number"));
        assert_eq!(c.layout_error(), None);
        assert_eq!(c.layout().hud.scale, 1.0);
        let c = Config::parse("[[block]]\nkind = \"meter\"\nanchor = \"middle\"\n");
        let err = c.layout_error().expect("an unknown anchor is a layout error");
        assert!(err.contains("anchor"), "{err}");
        assert_eq!(c.layout(), &Layout::default_layout());
    }

    #[test]
    fn set_writes_toml_and_converts_a_legacy_file_once() {
        let mut c = Config::parse("logs_dir = /x y\n");
        c.set(Key::Log, "/l");
        let out = c.to_toml();
        assert!(
            out.starts_with("log = \"/l\"\nlogs_dir = \"/x y\"\n\n[hud]\nscale = 1.0\nchord = \"ctrl+shift+grave\"\n\n[[block]]\n"),
            "{out}"
        );
        assert!(!out.contains("\nscale = 48"), "no legacy key survives");
        let mut c = Config::parse(&out);
        c.set(Key::Scale, "1.5");
        assert!(c.to_toml().contains("[hud]\nscale = 1.5\n"), "{}", c.to_toml());
        let out = set_in_text("nonsense = 1\n", Key::Backend, "plain").unwrap();
        assert!(out.contains("nonsense = ") && out.contains("backend = \"plain\""), "unknown keys are re-emitted: {out}");
    }

    // --- the round-trip guard, and the three ways regeneration lost a file ---

    #[test]
    fn an_unknown_bare_key_stays_top_level_across_a_round_trip() {
        // Emitted after `[[block]]` it would be a field of the last block on
        // the next read, where `Block`'s serde silently drops it: the
        // re-emission that exists so a typo is not lost would lose it, one
        // round trip later.
        let text = "nonsense = 5\n\n[hud]\nscale = 1.0\n\n[[block]]\nkind = \"meter\"\nanchor = \"top-left\"\n\n[[block]]\nkind = \"timers\"\nanchor = \"top-right\"\n";
        let c = Config::parse(text);
        assert_eq!(c.unknown(), &["nonsense".to_string()]);

        let out = c.to_toml_checked().expect("a document with an unknown key round-trips");
        let key_at = out.find("nonsense").expect("the key is re-emitted");
        let hud_at = out.find("[hud]").expect("the layout is written");
        assert!(key_at < hud_at, "an unknown bare key goes before the first table header: {out}");

        let again = Config::parse(&out);
        assert_eq!(again.unknown(), &["nonsense".to_string()], "still a top-level key, still reported: {out}");
        assert_eq!(again.layout().blocks.len(), 2);
        assert_eq!(again.layout(), c.layout());
    }

    #[test]
    fn a_top_level_key_named_like_a_block_field_does_not_become_the_blocks() {
        // `kind` is one of `Block`'s own field names. Re-emitted after
        // `[[block]]` it made a table with two `kind` keys, which is a
        // duplicate-key error, which sent the whole document down the Spec 4
        // legacy grammar: every layout key reported unknown, `log` carrying
        // its own quote characters, the scale read as pixels.
        let c = Config::parse("log = \"/x\"\nkind = \"banana\"\n\n[[block]]\nkind = \"meter\"\nanchor = \"top-left\"\n");
        let out = c.to_toml_checked().expect("a top-level `kind` round-trips");

        let again = Config::parse(&out);
        assert_eq!(again.layout_error(), None, "{out}");
        assert_eq!(again.get(Key::Log), Some("/x"), "the path did not grow quote characters: {out}");
        assert_eq!(again.unknown(), &["kind".to_string()]);
        assert_eq!(again.layout().blocks.len(), 1);
        assert_eq!(again.layout().blocks[0].kind, crate::layout::BlockKind::Meter);
    }

    #[test]
    fn a_legacy_unknown_key_is_re_emitted_as_a_quoted_string() {
        // The legacy grammar has only ever had text, so `nonsense = 1` comes
        // back as the string "1" rather than the integer 1 -- and as a
        // *quoted* string, or the regenerated document would not be TOML.
        // (`log /a` has no `=`, so the whole text is not valid TOML and this
        // is the legacy path.)
        let c = Config::parse("log /a\nnonsense = 1\nbackend = plain\n");
        assert_eq!(c.unknown(), &["nonsense".to_string()]);
        let out = c.to_toml_checked().expect("a legacy file round-trips");
        assert!(out.contains("nonsense = \"1\"\n"), "{out}");

        let again = Config::parse(&out);
        assert_eq!(again.unknown(), &["nonsense".to_string()]);
        assert_eq!(again.get(Key::Backend), Some("plain"));
    }

    #[test]
    fn an_unknown_table_keeps_its_own_header_after_the_blocks() {
        let c = Config::parse("[unknown_table]\na = 1\n");
        assert_eq!(c.unknown(), &["unknown_table".to_string()]);
        let out = c.to_toml_checked().expect("an unknown table round-trips");
        assert!(out.contains("[unknown_table]\na = 1\n"), "{out}");
        let block_at = out.find("[[block]]").expect("the layout is written");
        assert!(block_at < out.find("[unknown_table]").unwrap(), "a table goes after the blocks: {out}");
        assert_eq!(Config::parse(&out).unknown(), &["unknown_table".to_string()]);
    }

    #[test]
    fn to_toml_checked_refuses_text_that_does_not_read_back() {
        // `quote` escapes `"`, `\`, newline and tab, and nothing else, so a
        // value carrying a bare carriage return is written as a basic string
        // TOML will not accept -- which sends the next read down the legacy
        // fallback and loses the layout. The point of the guard is that a
        // hole like this one refuses the write instead of taking the file
        // with it; the guard is the post-condition, not a list of the holes
        // it knows about.
        let mut c = Config::parse("");
        c.set(Key::Log, "/a\rb");
        let refused = c.to_toml_checked().expect_err("a value that is not TOML is refused");
        let ConfigError::NotRoundTrip(detail) = &refused;
        assert!(detail.contains("log") || detail.contains("layout"), "{detail}");
        // The message says whose fault it is, because it is not the user's.
        assert!(refused.to_string().contains("bug in Wisp"), "{refused}");
        assert!(refused.to_string().contains("nothing was written"), "{refused}");
    }

    #[test]
    fn to_toml_checked_accepts_a_converted_legacy_scale() {
        // The one legitimate difference between what is written and what is
        // read back: `48 / 13.0` is reported and re-emitted to two decimals,
        // so the layout's own float is 3.6923 while the file says 3.69. The
        // guard compares the scale as the file spells it for exactly this
        // case -- it is the Spec 4 upgrade path, reached by any `wisp config
        // set` against a file written before Spec 5.
        let c = Config::parse("logs_dir = /a\nscale = 48\n");
        let out = c.to_toml_checked().expect("a converted legacy scale is not a round-trip failure");
        assert!(out.contains("[hud]\nscale = 3.69\n"), "{out}");
    }

    #[test]
    fn every_layout_error_names_the_block_it_came_from() {
        let c = Config::parse(
            "[[block]]\nkind = \"meter\"\nanchor = \"top-left\"\n\n[[block]]\nkind = \"timers\"\nanchor = \"bottm-left\"\n",
        );
        let err = c.layout_error().expect("a mistyped anchor is a layout error");
        assert!(err.starts_with("[[block]] 1: "), "{err}");
        assert!(err.contains("anchor"), "{err}");

        let c = Config::parse("block = 7\n");
        let err = c.layout_error().expect("`block` that is not an array of tables is a layout error");
        assert!(err.starts_with("block: "), "{err}");
    }

    #[test]
    fn a_non_finite_scale_has_a_toml_spelling_and_is_not_a_valid_scale() {
        // Rust prints `NaN`; TOML spells it `nan`, and `scale = NaN` is not a
        // TOML document at all. No entry point should ever get here --
        // `is_valid_scale` is the check both `wisp config set scale` and
        // `wisp hud scale` make first -- but what this function emits is
        // always readable TOML regardless.
        assert_eq!(fmt_f32(f32::NAN), "nan");
        assert_eq!(fmt_f32(f32::INFINITY), "inf");
        assert_eq!(fmt_f32(f32::NEG_INFINITY), "-inf");
        assert_eq!(fmt_f32(1.0), "1.0");
        assert_eq!(fmt_f32(1.5), "1.5");
        for text in ["nan", "inf", "-inf"] {
            let doc = format!("[hud]\nscale = {text}\n");
            assert!(toml::from_str::<toml::Table>(&doc).is_ok(), "{doc}");
        }

        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -1.0] {
            assert!(!is_valid_scale(bad), "{bad}");
        }
        for good in [0.5, 1.0, 1.5, 4.0] {
            assert!(is_valid_scale(good), "{good}");
        }
    }
}
