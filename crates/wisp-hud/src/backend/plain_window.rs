// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/plain_window.rs
//! Fallback for sessions with neither gamescope nor layer-shell -- notably
//! GNOME, which implements no layer-shell.
//!
//! An ordinary always-on-top window. This will not reliably composite above a
//! fullscreen game; that limitation is inherent to the approach and is stated
//! in the README rather than hidden.

use crate::backend::x11_common::{intern_atom, X11Surface};
use crate::backend::{BackendError, Frame, OverlayBackend};
use x11rb::connection::Connection;
use x11rb::properties::WmHints;
use x11rb::protocol::xproto::*;
use x11rb::wrapper::ConnectionExt as _;

pub struct PlainWindowBackend {
    width: u32,
    height: u32,
    surface: Option<X11Surface>,
}

impl PlainWindowBackend {
    pub fn new(width: u32, height: u32) -> Self {
        PlainWindowBackend {
            width,
            height,
            surface: None,
        }
    }
}

impl OverlayBackend for PlainWindowBackend {
    fn attach(&mut self) -> Result<(), BackendError> {
        // override_redirect = false: this is an ordinary WM-managed window,
        // not a gamescope overlay plane.
        let surface = X11Surface::create(self.width, self.height, false)?;
        let conn = &surface.conn;

        // Optional but helpful: tell the window manager what kind of window
        // this is, so it does not e.g. give it a taskbar entry.
        let window_type_atom = intern_atom(conn, "_NET_WM_WINDOW_TYPE")?;
        let utility_atom = intern_atom(conn, "_NET_WM_WINDOW_TYPE_UTILITY")?;
        conn.change_property32(
            PropMode::REPLACE,
            surface.window,
            window_type_atom,
            AtomEnum::ATOM,
            &[utility_atom],
        )
        .map_err(|e| BackendError::Failed(e.to_string()))?;

        // REQUIRED for the invariant: tell the window manager it may never
        // give this window input focus. Belt and braces alongside the empty
        // XFixes input region that `X11Surface::create` already applied.
        let hints = WmHints {
            input: Some(false),
            ..Default::default()
        };
        hints
            .set(conn, surface.window)
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        // Belt and braces: set _NET_WM_STATE to ABOVE before mapping, since
        // most window managers honour this property at map time.
        let net_wm_state_atom = intern_atom(conn, "_NET_WM_STATE")?;
        let above_atom = intern_atom(conn, "_NET_WM_STATE_ABOVE")?;
        conn.change_property32(
            PropMode::REPLACE,
            surface.window,
            net_wm_state_atom,
            AtomEnum::ATOM,
            &[above_atom],
        )
        .map_err(|e| BackendError::Failed(e.to_string()))?;

        surface.map()?;

        // The EWMH way to ask an already-mapped window to go above: a
        // _NET_WM_STATE client message sent to the root window with
        // _NET_WM_STATE_ADD.
        const NET_WM_STATE_ADD: u32 = 1;
        const SOURCE_INDICATION_NORMAL_APPLICATION: u32 = 1;
        let event = ClientMessageEvent::new(
            32,
            surface.window,
            net_wm_state_atom,
            [
                NET_WM_STATE_ADD,
                above_atom,
                0,
                SOURCE_INDICATION_NORMAL_APPLICATION,
                0,
            ],
        );
        conn.send_event(
            false,
            surface.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.flush()
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        self.surface = Some(surface);
        Ok(())
    }

    fn present(&mut self, frame: &Frame) {
        if let Some(surface) = &self.surface {
            surface.present(frame);
        }
    }
}
