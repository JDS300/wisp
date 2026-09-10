// SPDX-License-Identifier: MIT
//! Where Wisp's own two files live: the config file and the daemon's socket.
//!
//! Both rules are a function of the environment and nothing else, so each has a
//! `_from` variant taking the lookup itself: the rules can be tested without
//! mutating the process environment, and a Flatpak's paths asserted on a host
//! that has never run one.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

/// Why no config path could be resolved. The config file itself is optional —
/// Wisp runs on defaults without one — but `wisp config set` has nowhere to
/// write, so it says which variable the user has to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// `XDG_CONFIG_HOME` is set, and is not an absolute path.
    NoConfigHome,
    /// `XDG_CONFIG_HOME` was not set at all, and neither was `HOME`.
    NoHome,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::NoConfigHome => {
                write!(f, "XDG_CONFIG_HOME is not an absolute path and HOME is not set")
            }
            PathError::NoHome => write!(f, "HOME is not set"),
        }
    }
}

impl std::error::Error for PathError {}

/// `$XDG_CONFIG_HOME/wisp/config`, else `$HOME/.config/wisp/config`.
pub fn config_path() -> Result<PathBuf, PathError> {
    // A closure rather than `&std::env::var_os`: that function is generic over
    // its argument, so a reference to it fixes one lifetime and cannot be the
    // higher-ranked `dyn Fn(&str)` the `_from` variants take.
    config_path_from(&|name| std::env::var_os(name))
}

/// [`config_path`] against a caller's own environment lookup.
///
/// `XDG_CONFIG_HOME` counts only when it is absolute, as `XDG_DATA_HOME` does in
/// `wispd`'s duration store. Unlike that function there is no current-directory
/// fallback: a relative config path would quietly become a different file for
/// every directory the binary happened to start from.
pub fn config_path_from(env: &dyn Fn(&str) -> Option<OsString>) -> Result<PathBuf, PathError> {
    let xdg_config_home = env("XDG_CONFIG_HOME").map(PathBuf::from);
    let base = match &xdg_config_home {
        Some(p) if p.is_absolute() => p.clone(),
        _ => match env("HOME") {
            Some(home) => PathBuf::from(home).join(".config"),
            // Name the variable the user actually offered, if they offered one.
            None => {
                return Err(if xdg_config_home.is_some() {
                    PathError::NoConfigHome
                } else {
                    PathError::NoHome
                })
            }
        },
    };
    Ok(base.join("wisp").join("config"))
}

/// `$XDG_RUNTIME_DIR/wisp/wispd.sock`, else `/run/user/<uid>/wisp/wispd.sock`.
pub fn socket_path() -> PathBuf {
    socket_path_from(&|name| std::env::var_os(name))
}

/// [`socket_path`] against a caller's own environment lookup.
///
/// Under Flatpak the socket goes in `$XDG_RUNTIME_DIR/app/$FLATPAK_ID/` instead:
/// that directory is shared between instances of the same app and visible on the
/// host, so `wisp status` in a second `flatpak run` sees the daemon the first
/// one started. Flatpak always sets both variables, and `FLATPAK_ID` wins
/// whichever way round they arrived.
pub fn socket_path_from(env: &dyn Fn(&str) -> Option<OsString>) -> PathBuf {
    let base = env("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // SAFETY: getuid() takes no arguments and cannot fail.
            let uid = unsafe { getuid() };
            PathBuf::from(format!("/run/user/{}", uid))
        });
    match env("FLATPAK_ID") {
        Some(id) => base.join("app").join(id).join("wispd.sock"),
        None => base.join("wisp").join("wispd.sock"),
    }
}

extern "C" {
    fn getuid() -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// A fixed environment, so every rule is testable without mutating the
    /// process environment the rest of the test binary depends on.
    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn config_path_uses_an_absolute_xdg_config_home() {
        let got =
            config_path_from(&env(&[("XDG_CONFIG_HOME", "/etc/xdg"), ("HOME", "/home/u")])).unwrap();
        assert_eq!(got, PathBuf::from("/etc/xdg/wisp/config"));
    }

    #[test]
    fn config_path_ignores_a_relative_xdg_config_home() {
        // A relative XDG_CONFIG_HOME would resolve against whatever directory
        // the binary happened to start in, so it does not count.
        let got =
            config_path_from(&env(&[("XDG_CONFIG_HOME", ".config"), ("HOME", "/home/u")])).unwrap();
        assert_eq!(got, PathBuf::from("/home/u/.config/wisp/config"));
    }

    #[test]
    fn config_path_falls_back_to_home() {
        let got = config_path_from(&env(&[("HOME", "/home/u")])).unwrap();
        assert_eq!(got, PathBuf::from("/home/u/.config/wisp/config"));
    }

    #[test]
    fn config_path_errors_when_neither_variable_is_set() {
        let got = config_path_from(&env(&[]));
        assert_eq!(got, Err(PathError::NoHome));
        // An error rather than a path: there is no current-directory fallback
        // anywhere in this crate, unlike `DurationStore::default_path()`.
        assert!(got.is_err(), "a missing HOME must not yield ./wisp/config");
        let msg = got.unwrap_err().to_string();
        assert!(msg.contains("HOME"), "{msg}");
    }

    #[test]
    fn config_path_errors_on_a_relative_xdg_config_home_without_home() {
        // XDG_CONFIG_HOME was offered and cannot be used, so it is the variable
        // the message names.
        let got = config_path_from(&env(&[("XDG_CONFIG_HOME", "relative/config")]));
        assert_eq!(got, Err(PathError::NoConfigHome));
        let msg = got.unwrap_err().to_string();
        assert!(msg.contains("XDG_CONFIG_HOME"), "{msg}");
    }

    #[test]
    fn config_path_resolves_the_flatpak_location() {
        // Inside the sandbox XDG_CONFIG_HOME is already the app's own directory,
        // so the Flatpak location needs no branch of its own.
        let got = config_path_from(&env(&[(
            "XDG_CONFIG_HOME",
            "/home/u/.var/app/io.github.jds300.Wisp/config",
        )]))
        .unwrap();
        assert_eq!(
            got,
            PathBuf::from("/home/u/.var/app/io.github.jds300.Wisp/config/wisp/config")
        );
    }

    #[test]
    fn socket_path_uses_xdg_runtime_dir() {
        let got = socket_path_from(&env(&[("XDG_RUNTIME_DIR", "/run/user/1000")]));
        assert_eq!(got, PathBuf::from("/run/user/1000/wisp/wispd.sock"));
    }

    #[test]
    fn socket_path_without_xdg_runtime_dir_is_under_run_user() {
        let got = socket_path_from(&env(&[]));
        // The uid is the real one; a test cannot set it.
        let s = got.to_str().expect("/run/user/<uid>/wisp/wispd.sock is UTF-8");
        assert!(s.starts_with("/run/user/"), "{s}");
        assert!(s.ends_with("/wisp/wispd.sock"), "{s}");
    }

    #[test]
    fn socket_path_uses_the_flatpak_app_dir_when_flatpak_id_is_set() {
        // Flatpak shares $XDG_RUNTIME_DIR/app/$FLATPAK_ID between instances of
        // the same app, so `wisp status` in a second `flatpak run` sees the
        // daemon the first one started.
        let got = socket_path_from(&env(&[
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("FLATPAK_ID", "io.github.jds300.Wisp"),
        ]));
        assert_eq!(
            got,
            PathBuf::from("/run/user/1000/app/io.github.jds300.Wisp/wispd.sock")
        );
    }
}
