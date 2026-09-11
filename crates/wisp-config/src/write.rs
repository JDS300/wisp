// SPDX-License-Identifier: MIT
//! Writing the config file without ever leaving it half-written.
//!
//! Moved here from `wisp`'s `config_cmd` so `wisp config set` and the HUD's
//! own save (Spec 5) are one function rather than two copies that could drift.

use std::fs;
use std::io;
use std::path::Path;

/// Write `text` to `path` through a temporary file in the same directory,
/// then rename it into place.
///
/// The same directory because a rename across filesystems is not a rename,
/// and the rename because a config file half-written is a config file that
/// has lost every setting the write did not touch. The temporary carries
/// this process's id, so two writers cannot race over one name.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
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
        // rename that lands first can survive a crash pointing at a file
        // whose contents never arrived, which is not a file that lost its
        // last write but one that lost everything.
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
