// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/main.rs
mod backend;
mod client;
mod text;

use backend::{BackendKind, OverlayBackend};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::OsStr;

    // args_os, not args: OsStr-clean throughout, no String round-trip. Only
    // --backend and --scale need to become &str at all (one is matched
    // against fixed literals, the other is parsed as a float); a non-UTF-8
    // value for either is refused with a clear error rather than panicking.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let forced = args
        .iter()
        .position(|a| a == OsStr::new("--backend"))
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_str().ok_or("--backend value is not valid UTF-8"))
        .transpose()?;
    let scale: f32 = args
        .iter()
        .position(|a| a == OsStr::new("--scale"))
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.to_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(48.0);

    let kind = match forced {
        Some("gamescope") => BackendKind::GamescopeX11,
        Some("layer-shell") => BackendKind::WlrLayerShell,
        Some("plain") => BackendKind::PlainWindow,
        Some(other) => return Err(format!("unknown backend: {other}").into()),
        None => backend::choose(
            &backend::gamescope_x11::root_atom_names(),
            &backend::layer_shell::wayland_globals(),
        ),
    };
    eprintln!("wisp-hud: backend {kind:?}, scale {scale}px");

    let mut surface: Box<dyn OverlayBackend> = match kind {
        BackendKind::GamescopeX11 => {
            Box::new(backend::gamescope_x11::GamescopeX11Backend::new(400, 80))
        }
        BackendKind::WlrLayerShell => Box::new(backend::layer_shell::LayerShellBackend::new(400, 80)),
        BackendKind::PlainWindow => {
            Box::new(backend::plain_window::PlainWindowBackend::new(400, 80))
        }
    };
    surface.attach()?;

    let renderer = text::Renderer::new(scale);
    let path = client::socket_path();
    let mut stream = client::connect(&path)?;
    eprintln!("wisp-hud: connected to {}", path.display());

    while let Some(item) = stream.next_snapshot() {
        match item {
            Ok(snap) => {
                let frame = renderer.render(&format!("{} kills", snap.session_kills));
                if let Err(e) = surface.present(&frame) {
                    eprintln!("wisp-hud: {e}");
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("wisp-hud: {e}");
                std::process::exit(1);
            }
        }
    }
    // next_snapshot already logged a read error, if that's why the loop
    // ended; a `None` on a clean EOF is otherwise just the daemon going away.
    eprintln!("wisp-hud: daemon closed the connection");
    Ok(())
}
