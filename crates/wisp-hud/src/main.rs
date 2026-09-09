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
    // 48.0 is the default only when --scale is absent. A present-but-bad
    // value (non-UTF-8, or not a float) is refused with a clear error and
    // exit 2, not silently swapped for the default.
    let scale: f32 = match args
        .iter()
        .position(|a| a == OsStr::new("--scale"))
        .and_then(|i| args.get(i + 1))
    {
        Some(os) => {
            let s = os.to_str().ok_or("--scale value is not valid UTF-8")?;
            match s.parse::<f32>() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("wisp-hud: invalid --scale value: {s:?}");
                    std::process::exit(2);
                }
            }
        }
        None => 48.0,
    };

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

    let renderer = text::Renderer::new(scale);

    // Size the window from the renderer instead of a hardcoded guess: render
    // a worst-case probe string once and pad it, so the window is exactly as
    // big as the HUD can ever need to be at this scale and no bigger.
    const PAD: u32 = 8;
    let probe = renderer.render("999999 kills");
    let (w, h) = (probe.width + 2 * PAD, probe.height + 2 * PAD);

    let mut surface: Box<dyn OverlayBackend> = match kind {
        BackendKind::GamescopeX11 => {
            Box::new(backend::gamescope_x11::GamescopeX11Backend::new(w, h))
        }
        BackendKind::WlrLayerShell => Box::new(backend::layer_shell::LayerShellBackend::new(w, h)),
        BackendKind::PlainWindow => Box::new(backend::plain_window::PlainWindowBackend::new(w, h)),
    };
    surface.attach()?;

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
    // ended; only print "closed the connection" for the other case, a clean
    // EOF, so a read failure is not followed by a second, misleading line.
    if !stream.had_error() {
        eprintln!("wisp-hud: daemon closed the connection");
    }
    Ok(())
}
