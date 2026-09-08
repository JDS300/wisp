// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/main.rs
mod backend;
mod client;
mod text;

use backend::{BackendKind, OverlayBackend};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let forced = args
        .iter()
        .position(|a| a == "--backend")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str());
    let scale: f32 = args
        .iter()
        .position(|a| a == "--scale")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(48.0);

    let kind = match forced {
        Some("gamescope") => BackendKind::GamescopeX11,
        Some("layer-shell") => BackendKind::WlrLayerShell,
        Some("plain") => BackendKind::PlainWindow,
        Some(other) => return Err(format!("unknown backend: {other}").into()),
        None => backend::choose(&backend::gamescope_x11::root_atom_names(), &[]),
    };
    eprintln!("wisp-hud: backend {kind:?}, scale {scale}px");

    let mut surface: Box<dyn OverlayBackend> = match kind {
        BackendKind::GamescopeX11 => {
            Box::new(backend::gamescope_x11::GamescopeX11Backend::new(400, 80))
        }
        other => return Err(format!("{other:?} is not implemented yet").into()),
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
                surface.present(&frame);
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
