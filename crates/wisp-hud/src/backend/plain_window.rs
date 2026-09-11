// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/plain_window.rs
//! Fallback for sessions with neither gamescope nor layer-shell -- notably
//! GNOME, which implements no layer-shell.
//!
//! An ordinary always-on-top window. This will not reliably composite above a
//! fullscreen game; that limitation is inherent to the approach and is stated
//! in the README rather than hidden.

use crate::backend::x11_common::{intern_atom, X11Surface};
use crate::backend::{BackendError, Frame, OverlayBackend, Rect};
use x11rb::connection::Connection;
use x11rb::properties::WmHints;
use x11rb::protocol::xproto::*;
use x11rb::wrapper::ConnectionExt as _;

pub struct PlainWindowBackend {
    surface: Option<X11Surface>,
}

impl PlainWindowBackend {
    /// `output` is ignored, for the same reason it is on the gamescope
    /// backend: this is plain X11, sized to the default screen's root
    /// window.
    pub fn new(_output: Option<&str>) -> Self {
        PlainWindowBackend { surface: None }
    }
}

impl OverlayBackend for PlainWindowBackend {
    fn attach(&mut self) -> Result<(u32, u32), BackendError> {
        // override_redirect = false: this is an ordinary WM-managed window,
        // not a gamescope overlay plane.
        let surface = X11Surface::create(false)?;
        let conn = &surface.conn;

        // So the readback test (and anyone else inspecting the window tree)
        // can find this window by name.
        let net_wm_name_atom = intern_atom(conn, "_NET_WM_NAME")?;
        let utf8_string_atom = intern_atom(conn, "UTF8_STRING")?;
        conn.change_property8(
            PropMode::REPLACE,
            surface.window,
            net_wm_name_atom,
            utf8_string_atom,
            b"wisp-hud",
        )
        .map_err(|e| BackendError::Failed(e.to_string()))?;

        // REQUIRED for the invariant: _NET_WM_WINDOW_TYPE_DOCK. Per the EWMH
        // spec a dock is undecorated and kept above by definition -- unlike
        // UTILITY, which KWin (and others) decorate with a titlebar and
        // frame, giving the window manager's frame its own clickable,
        // draggable, resizable, closable surface that the XFixes input
        // region on the client window cannot reach.
        let window_type_atom = intern_atom(conn, "_NET_WM_WINDOW_TYPE")?;
        let dock_atom = intern_atom(conn, "_NET_WM_WINDOW_TYPE_DOCK")?;
        conn.change_property32(
            PropMode::REPLACE,
            surface.window,
            window_type_atom,
            AtomEnum::ATOM,
            &[dock_atom],
        )
        .map_err(|e| BackendError::Failed(e.to_string()))?;

        // Belt and braces for window managers that decorate DOCK anyway:
        // _MOTIF_WM_HINTS with MWM_HINTS_DECORATIONS set and decorations = 0
        // asks explicitly for no frame. Format 32, 5 values: flags,
        // functions, decorations, input_mode, status. The property's type is
        // the _MOTIF_WM_HINTS atom itself, not ATOM or CARDINAL.
        const MWM_HINTS_DECORATIONS: u32 = 1 << 1;
        let motif_wm_hints_atom = intern_atom(conn, "_MOTIF_WM_HINTS")?;
        conn.change_property32(
            PropMode::REPLACE,
            surface.window,
            motif_wm_hints_atom,
            motif_wm_hints_atom,
            &[MWM_HINTS_DECORATIONS, 0, 0, 0, 0],
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

        // Belt and braces: set _NET_WM_STATE to ABOVE + SKIP_TASKBAR +
        // SKIP_PAGER before mapping, since most window managers honour this
        // property at map time.
        let net_wm_state_atom = intern_atom(conn, "_NET_WM_STATE")?;
        let above_atom = intern_atom(conn, "_NET_WM_STATE_ABOVE")?;
        let skip_taskbar_atom = intern_atom(conn, "_NET_WM_STATE_SKIP_TASKBAR")?;
        let skip_pager_atom = intern_atom(conn, "_NET_WM_STATE_SKIP_PAGER")?;
        conn.change_property32(
            PropMode::REPLACE,
            surface.window,
            net_wm_state_atom,
            AtomEnum::ATOM,
            &[above_atom, skip_taskbar_atom, skip_pager_atom],
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
