// SPDX-License-Identifier: MIT
//! Overlay backends. The rule for choosing between them lives in `wisp-probe`.

use std::fmt;

pub mod gamescope_x11;
pub mod layer_shell;
pub mod plain_window;
mod x11_common;

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
    /// The display server is there and working, but cannot host the surface
    /// this HUD is: a translucent, click-through overlay the size of the
    /// output. Its own exit code (2, like every other "what you asked for
    /// cannot be done" in `wisp-hud`) rather than the 1 a broken display
    /// earns, because the fix is a different display server or a different
    /// backend, not a retry.
    Unsupported(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Unavailable(m) => write!(f, "backend unavailable: {m}"),
            BackendError::Failed(m) => write!(f, "backend failed: {m}"),
            BackendError::Unsupported(m) => write!(f, "backend unsupported: {m}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// One key transition from a backend that *receives* key events rather than
/// polling for them. `pressed` is false for a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: crate::keys::Key,
    pub pressed: bool,
}

/// Every implementation must produce a surface that never takes focus and
/// never receives pointer or keyboard input. See the charter invariant.
pub trait OverlayBackend {
    /// Creates and maps the surface at the output's full size and returns
    /// that size.
    fn attach(&mut self) -> Result<(u32, u32), BackendError>;
    /// Uploads `dirty` sub-rectangles of `frame` (which is output-sized). An
    /// empty `dirty` uploads nothing and returns `Ok`.
    fn present(&mut self, frame: &Frame, dirty: &[Rect]) -> Result<(), BackendError>;

    /// Ask for, or give back, the keyboard.
    ///
    /// `true` means this backend now receives key events and the caller must
    /// read them with `drain_keys` instead of polling. The default is a
    /// no-op returning `false`, which is the right answer on every X11
    /// backend -- Spec 5 §3.1 stands there unchanged, and `XQueryKeymap` on
    /// the HUD's own connection keeps working because the game still holds
    /// the keyboard.
    // Unused until Task 6b's layer-shell backend overrides these and
    // main.rs calls them; T6b removes this attribute.
    #[allow(dead_code)]
    fn take_keyboard(&mut self, _exclusive: bool) -> bool {
        false
    }

    /// Key events since the last call, in order. Empty on X11 backends, and
    /// only worth calling when `take_keyboard(true)` returned `true`.
    #[allow(dead_code)]
    fn drain_keys(&mut self) -> Vec<KeyEvent> {
        Vec::new()
    }
}

/// An axis-aligned pixel rectangle, top-left origin. `x`/`y` are signed so a
/// rect can be positioned or intersected off-canvas without special-casing;
/// `w`/`h` are unsigned because a negative size is not a rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    /// The whole frame, as a rect: `(0, 0, frame.width, frame.height)`.
    pub fn full(frame: &Frame) -> Rect {
        Rect { x: 0, y: 0, w: frame.width, h: frame.height }
    }

    /// The smallest rect containing both `self` and `other`.
    pub fn union(self, other: Rect) -> Rect {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.w as i32).max(other.x + other.w as i32);
        let y1 = (self.y + self.h as i32).max(other.y + other.h as i32);
        Rect { x: x0, y: y0, w: (x1 - x0) as u32, h: (y1 - y0) as u32 }
    }

    /// The overlapping region, or `None` when the two rects don't overlap
    /// (touching edges with zero-area overlap count as not overlapping).
    pub fn intersect(self, other: Rect) -> Option<Rect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w as i32).min(other.x + other.w as i32);
        let y1 = (self.y + self.h as i32).min(other.y + other.h as i32);
        if x1 > x0 && y1 > y0 {
            Some(Rect { x: x0, y: y0, w: (x1 - x0) as u32, h: (y1 - y0) as u32 })
        } else {
            None
        }
    }

    /// Whether `(x, y)` falls inside `self`; the right and bottom edges are
    /// exclusive, so a rect never contains its own far corner.
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w as i32 && y >= self.y && y < self.y + self.h as i32
    }

    /// `self` shrunk by `px` on every side (a negative `px` grows it
    /// instead). Never goes negative: a rect inset past its own middle
    /// collapses to a single point rather than flipping inside out.
    pub fn inset(self, px: i32) -> Rect {
        let w = (self.w as i32 - 2 * px).max(0) as u32;
        let h = (self.h as i32 - 2 * px).max(0) as u32;
        Rect { x: self.x + px, y: self.y + px, w, h }
    }
}
