// SPDX-License-Identifier: MIT
//! `wisp config path` / `show` / `set <key> <value>`.
//!
//! The only part of Wisp that writes anything of the user's, and so the only
//! part that has to be careful about a file it cannot read: replacing bytes it
//! could not parse with a single key would silently delete every other setting
//! in the file. Reading, parsing and regenerating the file as TOML is
//! `wisp_config::config`'s; the atomic write is `wisp_config::write`'s (the
//! HUD's own save shares it too); what is here is the refusal.

use std::fs;
use std::io;
use wisp_config::config::{is_valid_scale, Config, Key};
use wisp_config::paths::config_path;
use wisp_config::write::write_atomic;

/// The refusal every writing verb owes a config file whose layout did not
/// parse, in `wisp hud` as much as here: `Some(2)` when there is nothing safe
/// to write.
///
/// `Config::parse` never fails, so a mistyped anchor or a `rows = "8"` is not
/// an error the loader can raise — it keeps the last good layout, which for a
/// file read from disk is [`wisp_config::layout::Layout::default_layout`],
/// and records why. A reader can carry on with that. A writer cannot:
/// `to_toml` regenerates the whole file from the parsed model, so writing one
/// key back would replace the user's blocks with the default two, and their
/// widths, rows and offsets would be gone — at exit 0, with nothing said. The
/// error names the block, because once the write is refused that message is
/// the only thing the user has to find the bad key with.
pub fn refuse_unreadable_layout(config: &Config) -> Option<i32> {
    let error = config.layout_error()?;
    eprintln!("wisp: config: {error}");
    eprintln!(
        "wisp: refusing to rewrite a layout it could not read (that would replace it with the default); nothing was written"
    );
    Some(2)
}

/// [`Config::to_toml_checked`]'s text, or the exit code its refusal earns.
///
/// The guard is a post-condition rather than a check on anything the user
/// did, so its message says whose bug it is; the exit code is still 2,
/// because from the caller's side it is the same fact — this command wrote
/// nothing.
pub fn checked_toml(config: &Config) -> Result<String, i32> {
    config.to_toml_checked().map_err(|e| {
        eprintln!("wisp: {e}");
        2
    })
}

/// The file every binary reads, whether or not it exists yet.
pub fn path() -> i32 {
    match config_path() {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        // The spec's one case: with neither `XDG_CONFIG_HOME` absolute nor
        // `HOME` set there is no file to name, and exit 1 says so.
        Err(e) => {
            eprintln!("wisp: cannot resolve the config path: {e}");
            1
        }
    }
}

/// Every key, set or not, then the names in the file that are not keys Wisp has.
///
/// An unset key is printed rather than omitted so the report is the same shape
/// every time, and so a user can see the five keys without reading the source.
/// Three forms, and all three are this command's rendering rather than the
/// file's own text: a value as written, `(empty)` for a key written with nothing
/// after the `=`, and `(unset)` for a key the file does not mention. `(empty)`
/// is the word the HUD uses when it refuses such a value, so the two agree; and
/// neither parenthesis is a value, so neither is something `config set` could be
/// given back. The unknown names go under a comment because that is what they
/// are in the file as well.
pub fn show(config: &Config) -> i32 {
    for key in Key::ALL {
        match config.get(key) {
            Some("") => println!("{} = (empty)", key.name()),
            Some(value) => println!("{} = {value}", key.name()),
            None => println!("{} = (unset)", key.name()),
        }
    }
    if !config.unknown().is_empty() {
        println!("# unknown: {}", config.unknown().join(", "));
    }
    0
}

/// Write one key. The file is regenerated as TOML from the parsed model
/// (Spec 5): every other key the file already had is kept, but a comment is
/// not.
pub fn set(key: Key, value: &str) -> i32 {
    let path = match config_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("wisp: cannot resolve the config path: {e}");
            return 1;
        }
    };
    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        // No file yet is an empty one: `config set` is how the file comes to
        // exist in the first place, and creating its directory is this
        // command's job rather than the test's or the installer's.
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        // The one command that must not carry on with an empty config. Readers
        // ignore an unreadable file because Wisp runs on its defaults; a writer
        // would replace bytes it could not read with the single key it was
        // given, and the other four would be gone.
        Err(e) => {
            eprintln!("wisp: ignoring unreadable config {}: {e}", path.display());
            return 1;
        }
    };

    let mut config = Config::parse(&existing);
    if let Some(code) = refuse_unreadable_layout(&config) {
        return code;
    }
    // A scale that is a number has to be one the HUD can render at. Text that
    // is not a number at all still goes through: `Key::Scale`'s value is kept
    // as written so the HUD can refuse it and blame the file, which is what
    // `wisp doctor` reports and what the HUD's own startup refusal says. A
    // non-finite float is the one that cannot: `NaN` has no TOML spelling
    // Rust's `Display` writes, so the file stopped being readable at all.
    if key == Key::Scale {
        if let Ok(factor) = value.trim().parse::<f32>() {
            if !is_valid_scale(factor) {
                eprintln!("wisp: config set scale: {value} is not a scale (a finite factor greater than 0)");
                return 2;
            }
        }
    }
    config.set(key, value);
    let text = match checked_toml(&config) {
        Ok(text) => text,
        Err(code) => return code,
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("wisp: cannot create {}: {e}", parent.display());
            return 1;
        }
    }
    match write_atomic(&path, &text) {
        Ok(()) => {
            println!("{}", path.display());
            0
        }
        Err(e) => {
            eprintln!("wisp: cannot write {}: {e}", path.display());
            1
        }
    }
}
