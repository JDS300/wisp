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
    fn present(&mut self, frame: &Frame) -> Result<(), BackendError>;
}
