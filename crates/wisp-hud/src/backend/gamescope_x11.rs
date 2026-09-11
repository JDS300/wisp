// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/gamescope_x11.rs
//! An ordinary X11 window inside gamescope's XWayland, marked as the overlay
//! plane. This is the same mechanism `mangoapp` uses -- a separate process
//! linking libX11 and libGL, setting two atoms -- and gamescope's own help
//! recommends it over drawing inside the game.
//!
//! No injection, no LD_PRELOAD, no Vulkan layer.

use crate::backend::x11_common::X11Surface;
use crate::backend::{BackendError, Frame, OverlayBackend, Rect};

pub struct GamescopeX11Backend {
    surface: Option<X11Surface>,
}

impl GamescopeX11Backend {
    /// `output` is ignored: an X11 window has no notion of which Wayland
    /// output it lives on, and `X11Surface::create` always sizes itself to
    /// the default screen's root window.
    pub fn new(_output: Option<&str>) -> Self {
        GamescopeX11Backend { surface: None }
    }
}

impl OverlayBackend for GamescopeX11Backend {
    fn attach(&mut self) -> Result<(u32, u32), BackendError> {
        // override_redirect = true: bypass the window manager entirely,
        // gamescope composites us directly.
        let surface = X11Surface::create(true)?;

        // The two atoms that make gamescope treat this as the overlay plane.
        for (name, value) in [
            ("GAMESCOPE_EXTERNAL_OVERLAY", 1u32),
            ("GAMESCOPE_NO_FOCUS", 1u32),
        ] {
            surface.set_cardinal_property(name, value)?;
        }

        surface.map()?;
        let size = surface.size();
        self.surface = Some(surface);
        Ok(size)
    }

    fn present(&mut self, frame: &Frame, dirty: &[Rect]) -> Result<(), BackendError> {
        match &mut self.surface {
            Some(surface) => surface.present(frame, dirty),
            None => Ok(()),
        }
    }
}
