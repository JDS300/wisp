// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/x11_common.rs
//! Shared X11 plumbing used by every X11-backed overlay backend: connecting,
//! picking a depth-32 ARGB visual with a colormap (or falling back to the
//! root visual's depth), creating the window, applying the empty XFixes
//! input region the invariant requires on every backend, and the pure
//! Frame -> wire pixel conversion that `present` uses.
//!
//! `gamescope_x11` and `plain_window` differ only in a handful of atoms and
//! whether the window bypasses the window manager; everything else here is
//! identical between them, so it lives in one place instead of being
//! duplicated.

use crate::backend::{BackendError, Frame};
use x11rb::connection::Connection;
use x11rb::protocol::shape;
use x11rb::protocol::xfixes::ConnectionExt as XfixesExt;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

/// Which pixel layout the window ended up with. Both give a window the
/// compositor or window manager can display; only the byte conversion in
/// `frame_to_wire` differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// A native 32-bit ARGB visual: real, working alpha.
    Argb32,
    /// No depth-32 visual was offered by this X server; fall back to the root
    /// visual at its native depth (almost always 24) with a black
    /// background. No real alpha -- the frame's alpha byte is dropped.
    Bgrx24,
}

/// An X11 window set up as a paintable, input-inert overlay surface.
pub struct X11Surface {
    pub conn: RustConnection,
    pub window: Window,
    /// The root window of the screen `window` was created on. EWMH
    /// client messages (e.g. `_NET_WM_STATE`) are addressed to this.
    pub root: Window,
    gc: Gcontext,
    /// Kept alive for the lifetime of the surface; the server frees the
    /// colormap when the connection closes, but holding the id here
    /// documents that a depth-32 surface owns one.
    #[allow(dead_code)]
    colormap: Colormap,
    /// The depth actually in use once `create` has resolved
    /// `COPY_DEPTH_FROM_PARENT` to a real number. Needed by `present`, which
    /// -- unlike `create_window` -- has no "copy from parent" shorthand.
    depth: u8,
    width: u32,
    height: u32,
    format: PixelFormat,
    /// The server's byte order for image data, read once at connect time.
    msb_first: bool,
    /// Set the first time `present` sees a frame bigger than the window, so
    /// the clip warning is printed once rather than every frame at 5 Hz.
    warned_clipped: bool,
}

impl X11Surface {
    /// Connects to the X server and creates a `width` x `height` window at
    /// the root of the default screen. `override_redirect` bypasses the
    /// window manager entirely (gamescope's XWayland); when false, the
    /// window is an ordinary WM-managed window (the plain-window fallback).
    ///
    /// The window always carries an empty XFixes input region -- the
    /// invariant that the HUD never takes input applies unconditionally.
    pub fn create(width: u32, height: u32, override_redirect: bool) -> Result<X11Surface, BackendError> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|e| BackendError::Unavailable(e.to_string()))?;

        let msb_first = conn.setup().image_byte_order == ImageOrder::MSB_FIRST;
        let screen = conn.setup().roots[screen_num].clone();
        let root = screen.root;

        // Prefer a 32-bit ARGB visual so the window carries real alpha. If
        // the server offers none, fall back to the root visual at its own
        // depth with a black background.
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
                eprintln!("wisp-hud: x11 backend: using a depth-32 ARGB visual");
                (32u8, 32u8, visual_id, PixelFormat::Argb32, Some(colormap))
            } else {
                eprintln!(
                    "wisp-hud: x11 backend: no depth-32 visual offered, \
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

        let mut values = CreateWindowAux::new()
            // Deliberately no input events, and no EXPOSURE either: nothing
            // redraws on expose, `present` repaints the whole window at
            // 5 Hz, so there is no reason to receive -- and no reason to
            // drain -- an X event queue at all.
            .event_mask(EventMask::NO_EVENT);
        if override_redirect {
            values = values.override_redirect(1u32);
        }
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
            width as u16,
            height as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &values,
        )
        .map_err(|e| BackendError::Failed(e.to_string()))?;

        // Belt and braces for the invariant: an empty input region means
        // clicks pass straight through no matter what the window manager or
        // compositor otherwise decides about focus. Applies to every X11
        // backend without exception.
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

        Ok(X11Surface {
            conn,
            window,
            root,
            gc,
            colormap: colormap.unwrap_or(0),
            depth: actual_depth,
            width,
            height,
            format,
            msb_first,
            warned_clipped: false,
        })
    }

    /// Interns `name` and sets it on the window as a single-value CARDINAL
    /// property -- the shape every `GAMESCOPE_*` atom takes.
    pub fn set_cardinal_property(&self, name: &str, value: u32) -> Result<(), BackendError> {
        let atom = intern_atom(&self.conn, name)?;
        self.conn
            .change_property32(PropMode::REPLACE, self.window, atom, AtomEnum::CARDINAL, &[value])
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        Ok(())
    }

    pub fn map(&self) -> Result<(), BackendError> {
        self.conn
            .map_window(self.window)
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        self.conn
            .flush()
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        Ok(())
    }

    pub fn present(&mut self, frame: &Frame) -> Result<(), BackendError> {
        if !self.warned_clipped && (frame.width > self.width || frame.height > self.height) {
            eprintln!(
                "wisp-hud: frame {}x{} exceeds the window {}x{}; clipping",
                frame.width, frame.height, self.width, self.height
            );
            self.warned_clipped = true;
        }
        let (wire, draw_w, draw_h) = frame_to_wire(frame, self.width, self.height, self.format, self.msb_first);
        if draw_w == 0 || draw_h == 0 {
            return Ok(());
        }

        // 0-width/height is X11 shorthand for "to the edge of the window",
        // clearing anything a shorter previous frame left behind.
        let _ = self.conn.clear_area(false, self.window, 0, 0, 0, 0);
        // `.check()` forces a round trip so a dead window (BadDrawable,
        // BadWindow -- the compositor closed us, or the window was
        // destroyed out from under us) is reported here rather than
        // silently dropped. One round trip per frame at 5 Hz is cheap.
        self.conn
            .put_image(
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
            )
            .map_err(|e| BackendError::Failed(e.to_string()))?
            .check()
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        self.conn
            .flush()
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        Ok(())
    }
}

/// Interns an atom by name and returns its id. Shared by every X11 backend
/// that needs to look up a `GAMESCOPE_*` or EWMH atom, so the four-line
/// intern-then-reply dance lives in one place.
pub(crate) fn intern_atom(conn: &RustConnection, name: &str) -> Result<Atom, BackendError> {
    conn.intern_atom(false, name.as_bytes())
        .map_err(|e| BackendError::Failed(e.to_string()))?
        .reply()
        .map_err(|e| BackendError::Failed(e.to_string()))
        .map(|reply| reply.atom)
}

/// Converts a `Frame` (premultiplied RGBA, top-left origin) into the wire
/// bytes for `put_image`, clipped to `window_width` x `window_height` if the
/// frame is bigger than the window. Never resizes the window.
///
/// Returns the wire bytes and the clipped width and height actually drawn.
pub fn frame_to_wire(
    frame: &Frame,
    window_width: u32,
    window_height: u32,
    format: PixelFormat,
    msb_first: bool,
) -> (Vec<u8>, u32, u32) {
    if frame.width == 0 || frame.height == 0 {
        return (Vec::new(), 0, 0);
    }

    let draw_w = frame.width.min(window_width);
    let draw_h = frame.height.min(window_height);

    let mut wire = Vec::with_capacity((draw_w * draw_h * 4) as usize);
    for y in 0..draw_h {
        for x in 0..draw_w {
            let o = ((y * frame.width + x) * 4) as usize;
            let r = frame.rgba[o];
            let g = frame.rgba[o + 1];
            let b = frame.rgba[o + 2];
            let a = frame.rgba[o + 3];
            let alpha = match format {
                PixelFormat::Argb32 => a,
                PixelFormat::Bgrx24 => 0,
            };
            if msb_first {
                wire.extend_from_slice(&[alpha, r, g, b]);
            } else {
                wire.extend_from_slice(&[b, g, r, alpha]);
            }
        }
    }

    (wire, draw_w, draw_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_to_wire_converts_2x1_frame_for_both_pixel_formats() {
        let frame = Frame {
            width: 2,
            height: 1,
            rgba: vec![
                10, 20, 30, 40, // pixel 0: r, g, b, a
                50, 60, 70, 80, // pixel 1
            ],
        };

        // ARGB32, LSB-first server: bytes go B, G, R, A per pixel.
        let (wire, w, h) = frame_to_wire(&frame, 10, 10, PixelFormat::Argb32, false);
        assert_eq!((w, h), (2, 1));
        assert_eq!(wire, vec![30, 20, 10, 40, 70, 60, 50, 80]);

        // ARGB32, MSB-first server: bytes go A, R, G, B per pixel.
        let (wire, _, _) = frame_to_wire(&frame, 10, 10, PixelFormat::Argb32, true);
        assert_eq!(wire, vec![40, 10, 20, 30, 80, 50, 60, 70]);

        // BGRX24 drops alpha to 0 regardless of byte order.
        let (wire, _, _) = frame_to_wire(&frame, 10, 10, PixelFormat::Bgrx24, false);
        assert_eq!(wire, vec![30, 20, 10, 0, 70, 60, 50, 0]);
    }

    #[test]
    fn frame_to_wire_clips_to_the_window_size() {
        // A frame wider and taller than the window, 4x3, clipped to a 2x2
        // window. Every pixel gets a distinct, coordinate-derived colour (R =
        // x, G = y, B = 0xAB, A = 0xFF) so a bug that reads with the clipped
        // width as the row stride -- rather than the frame's own width -- is
        // caught: it would pull the wrong bytes for every row after the
        // first, not just produce the right byte count.
        let width = 4u32;
        let height = 3u32;
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let o = ((y * width + x) * 4) as usize;
                rgba[o] = x as u8;
                rgba[o + 1] = y as u8;
                rgba[o + 2] = 0xAB;
                rgba[o + 3] = 0xFF;
            }
        }
        let frame = Frame { width, height, rgba };

        let (wire, draw_w, draw_h) = frame_to_wire(&frame, 2, 2, PixelFormat::Argb32, true);
        assert_eq!((draw_w, draw_h), (2, 2));
        assert_eq!(wire.len(), (draw_w * draw_h * 4) as usize);

        // ARGB32, MSB-first: bytes go A, R, G, B per pixel.
        let pixel = |x: u8, y: u8| [0xFF, x, y, 0xAB];

        // First pixel of row 0: source (0, 0).
        assert_eq!(&wire[0..4], &pixel(0, 0));
        // Last kept pixel of row 0: source (1, 0) -- not (3, 0), which is
        // what an unclipped-width stride would wrongly read.
        assert_eq!(&wire[4..8], &pixel(1, 0));
        // First pixel of the last kept row: source (0, 1) -- not (0, 2) or
        // some offset derived from the frame's full width.
        assert_eq!(&wire[8..12], &pixel(0, 1));
    }
}
