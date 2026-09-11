// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/x11_common.rs
//! Shared X11 plumbing used by every X11-backed overlay backend: connecting,
//! taking a depth-32 ARGB visual with a colormap, creating the window,
//! applying the empty XFixes input region the invariant requires on every
//! backend, and the pure Frame -> wire pixel conversion that `present` uses.
//!
//! The depth-32 visual is a requirement, not a preference. There used to be a
//! fallback to the root visual's own depth with a black `background_pixel`,
//! which was harmless while the window was a few hundred pixels across; Task
//! 6 made the window the whole root window and made `present` upload only the
//! dirty rects, so every pixel no block covers keeps the window's background
//! -- opaque black across the entire screen, with the HUD floating on it.
//! Depth 24 has no alpha channel, so no background setting makes that window
//! translucent; the only honest answers are to refuse or to map an opaque
//! screen-sized window, and this one refuses.
//!
//! `gamescope_x11` and `plain_window` differ only in a handful of atoms and
//! whether the window bypasses the window manager; everything else here is
//! identical between them, so it lives in one place instead of being
//! duplicated.

use crate::backend::{BackendError, Frame, Rect};
use x11rb::connection::Connection;
use x11rb::protocol::shape;
use x11rb::protocol::xfixes::ConnectionExt as XfixesExt;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// The one window depth this HUD will use. See the module header: a surface
/// with no alpha channel cannot be a translucent overlay, and one the size of
/// the screen with no alpha is an opaque screen.
const DEPTH_32: u8 = 32;

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
    /// The window's depth, which is always 32 -- `create` refuses anything
    /// else. Carried rather than spelled at the `put_image` call site so the
    /// two cannot drift.
    depth: u8,
    width: u32,
    height: u32,
    /// The server's byte order for image data, read once at connect time.
    msb_first: bool,
}

impl X11Surface {
    /// Connects to the X server and creates a window the size of the default
    /// screen's root window, at (0, 0). `override_redirect` bypasses the
    /// window manager entirely (gamescope's XWayland); when false, the
    /// window is an ordinary WM-managed window (the plain-window fallback).
    ///
    /// The window always carries an empty XFixes input region -- the
    /// invariant that the HUD never takes input applies unconditionally.
    ///
    /// Refuses with [`BackendError::Unsupported`] when the server offers no
    /// depth-32 visual: see this module's own header for why there is no
    /// fallback.
    pub fn create(override_redirect: bool) -> Result<X11Surface, BackendError> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|e| BackendError::Unavailable(e.to_string()))?;

        let msb_first = conn.setup().image_byte_order == ImageOrder::MSB_FIRST;
        let screen = conn.setup().roots[screen_num].clone();
        let root = screen.root;
        let width = screen.width_in_pixels as u32;
        let height = screen.height_in_pixels as u32;

        // A 32-bit ARGB visual, or nothing. Without one the window has no
        // alpha channel at all, and a screen-sized window with no alpha is an
        // opaque screen-sized window -- which is the one thing an overlay must
        // never be, whatever it paints inside it.
        let Some(visual) = screen
            .allowed_depths
            .iter()
            .find(|d| d.depth == 32)
            .and_then(|d| d.visuals.first())
            .map(|v| v.visual_id)
        else {
            return Err(BackendError::Unsupported(format!(
                "this X server offers no 32-bit (depth-32 ARGB) visual, only depth {}; \
                 the HUD is a translucent overlay the size of the screen, and refuses \
                 to map an opaque one instead",
                screen.root_depth
            )));
        };

        let window = conn
            .generate_id()
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        let colormap = conn
            .generate_id()
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.create_colormap(ColormapAlloc::NONE, colormap, root, visual)
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        eprintln!("wisp-hud: x11 backend: using a depth-32 ARGB visual");

        let mut values = CreateWindowAux::new()
            // Deliberately no input events, and no EXPOSURE either: nothing
            // redraws on expose, `present` repaints the whole window at
            // 5 Hz, so there is no reason to receive -- and no reason to
            // drain -- an X event queue at all.
            .event_mask(EventMask::NO_EVENT);
        if override_redirect {
            values = values.override_redirect(1u32);
        }
        // A non-default-depth window needs all three of these, or the server
        // rejects window creation with BadMatch. `background_pixel(0)` is a
        // fully transparent background, which is what every pixel no block
        // covers must be.
        let values = values
            .colormap(colormap)
            .border_pixel(0u32)
            .background_pixel(0u32);

        conn.create_window(
            DEPTH_32,
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
            colormap,
            depth: DEPTH_32,
            width,
            height,
            msb_first,
        })
    }

    /// The window's size: the root window's, per [`X11Surface::create`].
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
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

    /// Uploads `dirty` sub-rectangles of `frame` (output-sized, same as the
    /// window) with one `put_image` per rect, clipped to the window and
    /// positioned at the rect's own origin. No `clear_area`: the frame is
    /// transparent where nothing is drawn, and a rect that became empty
    /// arrives already carrying transparent pixels via the canvas's own
    /// `clear`, so re-uploading it is enough to erase what was there.
    pub fn present(&mut self, frame: &Frame, dirty: &[Rect]) -> Result<(), BackendError> {
        if dirty.is_empty() {
            return Ok(());
        }
        let window = Rect::new(0, 0, self.width, self.height);
        let mut draws = Vec::with_capacity(dirty.len());
        for &rect in dirty {
            let Some(clipped) = rect.intersect(window) else { continue };
            let (wire, w, h) = frame_to_wire_rect(frame, clipped, self.msb_first);
            if w == 0 || h == 0 {
                continue;
            }
            draws.push((clipped.x, clipped.y, wire, w, h));
        }
        if draws.is_empty() {
            return Ok(());
        }

        // `.check()` forces a round trip so a dead window (BadDrawable,
        // BadWindow -- the compositor closed us, or the window was
        // destroyed out from under us) is reported here rather than
        // silently dropped. Only the last request in the batch is checked:
        // one round trip per frame at 5 Hz is cheap, one per dirty rect is
        // not.
        let last = draws.len() - 1;
        for (i, (x, y, wire, w, h)) in draws.iter().enumerate() {
            let cookie = self
                .conn
                .put_image(
                    ImageFormat::Z_PIXMAP,
                    self.window,
                    self.gc,
                    *w as u16,
                    *h as u16,
                    *x as i16,
                    *y as i16,
                    0,
                    self.depth,
                    wire,
                )
                .map_err(|e| BackendError::Failed(e.to_string()))?;
            if i == last {
                cookie.check().map_err(|e| BackendError::Failed(e.to_string()))?;
            }
        }
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

/// Converts one sub-rectangle of a `Frame` (premultiplied RGBA, top-left
/// origin) into the wire bytes for `put_image`: the bytes of `rect ∩ frame`,
/// clipped to the frame's own bounds so a rect that runs off the frame's edge
/// -- or entirely misses it -- never indexes out of bounds.
///
/// Returns the wire bytes and the clipped width and height actually drawn;
/// either is 0 when `rect` does not overlap the frame at all.
pub fn frame_to_wire_rect(frame: &Frame, rect: Rect, msb_first: bool) -> (Vec<u8>, u32, u32) {
    let Some(clipped) = rect.intersect(Rect::full(frame)) else {
        return (Vec::new(), 0, 0);
    };

    let mut wire = Vec::with_capacity((clipped.w * clipped.h * 4) as usize);
    for row in 0..clipped.h {
        for col in 0..clipped.w {
            let x = (clipped.x + col as i32) as u32;
            let y = (clipped.y + row as i32) as u32;
            let o = ((y * frame.width + x) * 4) as usize;
            let r = frame.rgba[o];
            let g = frame.rgba[o + 1];
            let b = frame.rgba[o + 2];
            let a = frame.rgba[o + 3];
            if msb_first {
                wire.extend_from_slice(&[a, r, g, b]);
            } else {
                wire.extend_from_slice(&[b, g, r, a]);
            }
        }
    }

    (wire, clipped.w, clipped.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_to_wire_rect_converts_a_2x1_frame_in_either_byte_order() {
        let frame = Frame {
            width: 2,
            height: 1,
            rgba: vec![
                10, 20, 30, 40, // pixel 0: r, g, b, a
                50, 60, 70, 80, // pixel 1
            ],
        };
        let full = Rect::full(&frame);

        // LSB-first server: bytes go B, G, R, A per pixel.
        let (wire, w, h) = frame_to_wire_rect(&frame, full, false);
        assert_eq!((w, h), (2, 1));
        assert_eq!(wire, vec![30, 20, 10, 40, 70, 60, 50, 80]);

        // MSB-first server: bytes go A, R, G, B per pixel.
        let (wire, _, _) = frame_to_wire_rect(&frame, full, true);
        assert_eq!(wire, vec![40, 10, 20, 30, 80, 50, 60, 70]);

        // The alpha byte is always the frame's own: the window is depth 32 or
        // `create` refused it, so there is no format that drops alpha.
        assert_eq!(wire[0], 40, "pixel 0 keeps its alpha");
    }

    #[test]
    fn frame_to_wire_rect_clips_using_the_frames_own_stride() {
        // A rect smaller than the 4x3 frame it's drawn from, clipped to 2x2.
        // Every pixel gets a distinct, coordinate-derived colour (R = x, G =
        // y, B = 0xAB, A = 0xFF) so a bug that reads with the clipped width
        // as the row stride -- rather than the frame's own width -- is
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

        let (wire, draw_w, draw_h) =
            frame_to_wire_rect(&frame, Rect::new(0, 0, 2, 2), true);
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

    #[test]
    fn frame_to_wire_rect_extracts_the_clipped_sub_rectangle() {
        let frame = Frame { width: 3, height: 2, rgba: (0..24).collect() }; // pixel (x,y) starts at (y*3+x)*4
        let (bytes, w, h) = frame_to_wire_rect(&frame, Rect::new(1, 0, 5, 5), false);
        assert_eq!((w, h), (2, 2));
        assert_eq!(&bytes[0..4], &[6, 5, 4, 7], "pixel (1,0) as BGRA");
        // Row-major, row outer / column inner -- the same order the older
        // `frame_to_wire` established and this file's stride test still
        // checks: index 2 of a 2-wide output is (row 1, col 0), i.e. pixel
        // (1, 1), not index 3 (which is (row 1, col 1), pixel (2, 1)).
        assert_eq!(&bytes[8..12], &[18, 17, 16, 19], "pixel (1,1)");
        assert_eq!(frame_to_wire_rect(&frame, Rect::new(10, 10, 1, 1), false).0.len(), 0);
    }
}
