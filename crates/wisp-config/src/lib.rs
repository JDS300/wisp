// SPDX-License-Identifier: MIT
//! Where Wisp's own files live, and how the game's log is found.
//!
//! Three binaries would otherwise each carry a copy of these rules, and the
//! copies would drift: `wispd` already had one `socket_path` and `wisp-hud`
//! another. Everything here is a pure rule over an environment, a directory
//! listing or a block of text, so it is testable without a display, a daemon
//! or the game — and this crate depends on nothing that could open one.

pub mod config;
pub mod discover;
pub mod layout;
pub mod paths;
pub mod source;
pub mod spells;
pub mod write;
