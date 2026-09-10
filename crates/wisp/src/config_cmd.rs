// SPDX-License-Identifier: MIT
//! `wisp config path` / `show` / `set <key> <value>`.
//!
//! The only part of Wisp that writes anything of the user's, and so the only
//! part that has to be careful about a file it cannot read: replacing bytes it
//! could not parse with a single key would silently delete every other setting
//! in the file. Reading, parsing and rewriting a key is
//! `wisp_config::config`'s; what is here is the refusal and the write.

use std::fs;
use std::io;
use std::path::Path;
use wisp_config::config::{set_in_text, Config, Key};
use wisp_config::paths::config_path;

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

/// Write one key, keeping every other line and comment byte for byte.
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

    let text = set_in_text(&existing, key, value);
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

/// Write `text` to `path` through a temporary file in the same directory, then
/// rename it into place.
///
/// The same directory because a rename across filesystems is not a rename, and
/// the rename because a config file half-written is a config file that has lost
/// every key the user did not touch. The temporary carries this process's id, so
/// two `config set` commands cannot race over one name.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
    let Some(name) = name else {
        return Err(io::Error::other("the config path has no file name"));
    };
    let temporary = path.with_file_name(format!("{name}.tmp-{}", std::process::id()));
    {
        use std::io::Write;
        let mut file = fs::File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        // On the device before the rename, as `DurationStore::save` does. A
        // rename that lands first can survive a crash pointing at a file whose
        // contents never arrived, which is not a config that lost its last
        // write but one that lost every key.
        file.sync_all()?;
    }
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        // A failure here is a failure to have written anything, and the
        // temporary must not be left beside the file it was going to become.
        Err(e) => {
            let _ = fs::remove_file(&temporary);
            Err(e)
        }
    }
}
