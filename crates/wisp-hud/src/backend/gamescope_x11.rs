// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/gamescope_x11.rs
//! An ordinary X11 window inside gamescope's XWayland, marked as the overlay
//! plane. This is the same mechanism `mangoapp` uses -- a separate process
//! linking libX11 and libGL, setting two atoms -- and gamescope's own help
//! recommends it over drawing inside the game.
//!
//! No injection, no LD_PRELOAD, no Vulkan layer.

use crate::backend::{BackendError, Frame, OverlayBackend};
use x11rb::connection::Connection;
use x11rb::protocol::shape;
use x11rb::protocol::xfixes::ConnectionExt as XfixesExt;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

/// Names of every property on the X root window. Selection uses this to detect
/// gamescope, whose XWayland root carries around seventeen GAMESCOPE_* entries.
pub fn root_atom_names() -> Vec<String> {
    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        return Vec::new();
    };
    let root = conn.setup().roots[screen_num].root;
    let Ok(cookie) = conn.list_properties(root) else {
        return Vec::new();
    };
    let Ok(reply) = cookie.reply() else {
        return Vec::new();
    };
    reply
        .atoms
        .iter()
        .filter_map(|&atom| {
            let name = conn.get_atom_name(atom).ok()?.reply().ok()?.name;
            String::from_utf8(name).ok()
        })
        .collect()
}

/// Which pixel layout the window ended up with. Both give a window gamescope
/// will treat as the overlay plane; only the byte conversion in `present`
/// differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PixelFormat {
    /// A native 32-bit ARGB visual: real, working alpha.
    Argb32,
    /// No depth-32 visual was offered by this X server; fall back to the root
    /// visual at its native depth (almost always 24) with a black
    /// background. No real alpha -- the frame's alpha byte is dropped.
    Bgrx24,
}

pub struct GamescopeX11Backend {
    width: u32,
    height: u32,
    conn: Option<RustConnection>,
    window: Window,
    gc: Gcontext,
    colormap: Colormap,
    /// The depth actually in use once `attach` has resolved
    /// `COPY_DEPTH_FROM_PARENT` to a real number. Needed by `put_image`,
    /// which -- unlike `create_window` -- has no "copy from parent" shorthand.
    depth: u8,
    format: PixelFormat,
    /// The server's byte order for image data, read once at connect time.
    msb_first: bool,
}

impl GamescopeX11Backend {
    pub fn new(width: u32, height: u32) -> Self {
        GamescopeX11Backend {
            width,
            height,
            conn: None,
            window: 0,
            gc: 0,
            colormap: 0,
            depth: 0,
            format: PixelFormat::Bgrx24,
            msb_first: false,
        }
    }
}

impl OverlayBackend for GamescopeX11Backend {
    fn attach(&mut self) -> Result<(), BackendError> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|e| BackendError::Unavailable(e.to_string()))?;

        let msb_first = conn.setup().image_byte_order == ImageOrder::MSB_FIRST;
        let screen = conn.setup().roots[screen_num].clone();
        let root = screen.root;

        // Prefer a 32-bit ARGB visual so the window carries real alpha (ruling
        // B). If gamescope's XWayland offers none, fall back to the root
        // visual at its own depth with a black background.
        let depth32_visual = screen
            .allowed_depths
            .iter()
            .find(|d| d.depth == 32)
            .and_then(|d| d.visuals.first())
            .map(|v| v.visual_id);

        let window = conn
            .generate_id()
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        let (create_depth, actual_depth, visual, format, colormap) =
            if let Some(visual_id) = depth32_visual {
                let colormap = conn
                    .generate_id()
                    .map_err(|e| BackendError::Failed(e.to_string()))?;
                conn.create_colormap(ColormapAlloc::NONE, colormap, root, visual_id)
                    .map_err(|e| BackendError::Failed(e.to_string()))?;
                eprintln!("wisp-hud: gamescope backend: using a depth-32 ARGB visual");
                (32u8, 32u8, visual_id, PixelFormat::Argb32, Some(colormap))
            } else {
                eprintln!(
                    "wisp-hud: gamescope backend: no depth-32 visual offered, \
                     falling back to depth-{} BGRX",
                    screen.root_depth
                );
                (
                    COPY_DEPTH_FROM_PARENT,
                    screen.root_depth,
                    screen.root_visual,
                    PixelFormat::Bgrx24,
                    None,
                )
            };

        let values = CreateWindowAux::new()
            // Deliberately no input events. The HUD never takes input.
            .event_mask(EventMask::EXPOSURE)
            // Bypass the window manager entirely; gamescope composites us directly.
            .override_redirect(1u32);
        let values = match colormap {
            // A non-default-depth window needs all three of these, or the
            // server rejects window creation with BadMatch.
            Some(cmap) => values
                .colormap(cmap)
                .border_pixel(0u32)
                .background_pixel(0u32),
            None => values.background_pixel(screen.black_pixel),
        };

        conn.create_window(
            create_depth,
            window,
            root,
            0,
            0,
            self.width as u16,
            self.height as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &values,
        )
        .map_err(|e| BackendError::Failed(e.to_string()))?;

        // The two atoms that make gamescope treat this as the overlay plane.
        for (name, value) in [
            ("GAMESCOPE_EXTERNAL_OVERLAY", 1u32),
            ("GAMESCOPE_NO_FOCUS", 1u32),
        ] {
            let atom = conn
                .intern_atom(false, name.as_bytes())
                .map_err(|e| BackendError::Failed(e.to_string()))?
                .reply()
                .map_err(|e| BackendError::Failed(e.to_string()))?
                .atom;
            conn.change_property32(PropMode::REPLACE, window, atom, AtomEnum::CARDINAL, &[value])
                .map_err(|e| BackendError::Failed(e.to_string()))?;
        }

        // Belt and braces for the invariant: an empty input region means clicks
        // pass straight through, whatever GAMESCOPE_NO_FOCUS turns out to do.
        // The spec records NO_FOCUS as verified-accepted but not verified to
        // deliver click-through; this does not depend on it.
        conn.xfixes_query_version(5, 0)
            .map_err(|e| BackendError::Failed(e.to_string()))?
            .reply()
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        let region = conn
            .generate_id()
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.xfixes_create_region(region, &[])
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.xfixes_set_window_shape_region(window, shape::SK::INPUT, 0, 0, region)
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.xfixes_destroy_region(region)
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        let gc = conn
            .generate_id()
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.create_gc(gc, window, &CreateGCAux::new())
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        conn.map_window(window)
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.flush()
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        self.conn = Some(conn);
        self.window = window;
        self.gc = gc;
        self.colormap = colormap.unwrap_or(0);
        self.depth = actual_depth;
        self.format = format;
        self.msb_first = msb_first;
        Ok(())
    }

    fn present(&mut self, frame: &Frame) {
        let Some(conn) = self.conn.as_ref() else {
            return;
        };
        if frame.width == 0 || frame.height == 0 {
            return;
        }

        // The frame is already premultiplied; never resize the window, just
        // clip to it if the frame is bigger (ruling C).
        let draw_w = frame.width.min(self.width);
        let draw_h = frame.height.min(self.height);

        let mut wire = Vec::with_capacity((draw_w * draw_h * 4) as usize);
        for y in 0..draw_h {
            for x in 0..draw_w {
                let o = ((y * frame.width + x) * 4) as usize;
                let r = frame.rgba[o];
                let g = frame.rgba[o + 1];
                let b = frame.rgba[o + 2];
                let a = frame.rgba[o + 3];
                let alpha = match self.format {
                    PixelFormat::Argb32 => a,
                    PixelFormat::Bgrx24 => 0,
                };
                if self.msb_first {
                    wire.extend_from_slice(&[alpha, r, g, b]);
                } else {
                    wire.extend_from_slice(&[b, g, r, alpha]);
                }
            }
        }

        // 0-width/height is X11 shorthand for "to the edge of the window",
        // clearing anything a shorter previous frame left behind.
        let _ = conn.clear_area(false, self.window, 0, 0, 0, 0);
        let _ = conn.put_image(
            ImageFormat::Z_PIXMAP,
            self.window,
            self.gc,
            draw_w as u16,
            draw_h as u16,
            0,
            0,
            0,
            self.depth,
            &wire,
        );
        let _ = conn.flush();
    }
}
