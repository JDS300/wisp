// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/gamescope_x11.rs
//! An ordinary X11 window inside gamescope's XWayland, marked as the overlay
//! plane. This is the same mechanism `mangoapp` uses -- a separate process
//! linking libX11 and libGL, setting two atoms -- and gamescope's own help
//! recommends it over drawing inside the game.
//!
//! No injection, no LD_PRELOAD, no Vulkan layer.

use crate::backend::x11_common::X11Surface;
use crate::backend::{BackendError, Frame, OverlayBackend};

pub struct GamescopeX11Backend {
    width: u32,
    height: u32,
    surface: Option<X11Surface>,
}

impl GamescopeX11Backend {
    pub fn new(width: u32, height: u32) -> Self {
        GamescopeX11Backend {
            width,
            height,
            surface: None,
        }
    }
}

impl OverlayBackend for GamescopeX11Backend {
    fn attach(&mut self) -> Result<(), BackendError> {
        // override_redirect = true: bypass the window manager entirely,
        // gamescope composites us directly.
        let surface = X11Surface::create(self.width, self.height, true)?;

        // The two atoms that make gamescope treat this as the overlay plane.
        for (name, value) in [
            ("GAMESCOPE_EXTERNAL_OVERLAY", 1u32),
            ("GAMESCOPE_NO_FOCUS", 1u32),
        ] {
            surface.set_cardinal_property(name, value)?;
        }

        surface.map()?;
        self.surface = Some(surface);
        Ok(())
    }

    fn present(&mut self, frame: &Frame) -> Result<(), BackendError> {
        match &mut self.surface {
            Some(surface) => surface.present(frame),
            None => Ok(()),
        }
    }
}
