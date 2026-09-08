// SPDX-License-Identifier: MIT
//! Overlay backends and the rule for choosing between them.

use std::fmt;

/// One frame of premultiplied-alpha RGBA, top-left origin.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub enum BackendError {
    Unavailable(String),
    Failed(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Unavailable(m) => write!(f, "backend unavailable: {m}"),
            BackendError::Failed(m) => write!(f, "backend failed: {m}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// Every implementation must produce a surface that never takes focus and
/// never receives pointer or keyboard input. See the charter invariant.
pub trait OverlayBackend {
    fn attach(&mut self) -> Result<(), BackendError>;
    fn present(&mut self, frame: &Frame);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    GamescopeX11,
    WlrLayerShell,
    PlainWindow,
}

/// Gamescope wins whenever its root properties are visible, even if the outer
/// session also offers layer-shell: the game is inside gamescope, so that is
/// where the overlay has to be.
pub fn choose(x_root_atom_names: &[String], wayland_globals: &[String]) -> BackendKind {
    if x_root_atom_names.iter().any(|a| a.starts_with("GAMESCOPE_")) {
        return BackendKind::GamescopeX11;
    }
    if wayland_globals.iter().any(|g| g == "zwlr_layer_shell_v1") {
        return BackendKind::WlrLayerShell;
    }
    BackendKind::PlainWindow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn gamescope_root_properties_select_the_gamescope_backend() {
        let atoms = s(&[
            "GAMESCOPE_XWAYLAND_SERVER_ID",
            "GAMESCOPE_FOCUSED_WINDOW",
            "WM_NAME",
        ]);
        assert_eq!(choose(&atoms, &s(&[])), BackendKind::GamescopeX11);
    }

    #[test]
    fn gamescope_wins_even_when_layer_shell_is_also_present() {
        // Nested gamescope inside a layer-shell-capable session. The game is
        // inside gamescope, so that is where the overlay must go.
        let atoms = s(&["GAMESCOPE_FOCUSED_APP"]);
        let globals = s(&["zwlr_layer_shell_v1"]);
        assert_eq!(choose(&atoms, &globals), BackendKind::GamescopeX11);
    }

    #[test]
    fn layer_shell_is_used_when_advertised_and_no_gamescope() {
        let globals = s(&["wl_compositor", "zwlr_layer_shell_v1"]);
        assert_eq!(choose(&s(&[]), &globals), BackendKind::WlrLayerShell);
    }

    #[test]
    fn plain_window_is_the_fallback() {
        // GNOME: no layer-shell, no gamescope.
        let globals = s(&["wl_compositor", "xdg_wm_base"]);
        assert_eq!(choose(&s(&[]), &globals), BackendKind::PlainWindow);
    }

    #[test]
    fn ordinary_x11_properties_do_not_trigger_gamescope() {
        let atoms = s(&["WM_NAME", "_NET_SUPPORTED", "GAMESCOPEISH_NOT_REALLY"]);
        // Prefix match is on "GAMESCOPE_" with the underscore, so this is not a hit.
        assert_eq!(choose(&atoms, &s(&[])), BackendKind::PlainWindow);
    }
}
