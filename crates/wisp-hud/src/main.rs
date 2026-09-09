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

    // Size the window from the renderer: the kill line plus MAX_ROWS timer
    // rows at their widest, padded, so the HUD never clips at this scale.
    const PAD: u32 = 8;
    let widest = std::iter::once(text::Line { text: "999999 kills".to_string(), rgb: WHITE })
        .chain((0..MAX_ROWS).map(|_| text::Line { text: format_row("W".repeat(TARGET_COLS).as_str(), &"W".repeat(SPELL_COLS), 9999), rgb: WHITE }))
        .collect::<Vec<_>>();
    let probe = renderer.render_lines(&widest);
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
                let frame = renderer.render_lines(&hud_lines(&snap));
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

use wisp_proto::{Confidence, Snapshot, Timer};

/// Wisp's own presentation thresholds. Not derived from anything.
const WARNING_SECS: i64 = 10;
const CRITICAL_SECS: i64 = 5;
const MAX_ROWS: usize = 8;
const TARGET_COLS: usize = 20;
const SPELL_COLS: usize = 18;

const WHITE: [u8; 3] = [255, 255, 255];
const DIM: [u8; 3] = [170, 170, 170];
const WARNING: [u8; 3] = [255, 200, 0];
const CRITICAL: [u8; 3] = [255, 70, 70];

fn roman(rank: u8) -> &'static str {
    ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"]
        .get(rank as usize)
        .copied()
        .unwrap_or("")
}

/// Truncate to `cols` characters, padding on the right so columns line up
/// in the monospace face.
fn fit(s: &str, cols: usize) -> String {
    let mut out: String = s.chars().take(cols).collect();
    while out.chars().count() < cols {
        out.push(' ');
    }
    out
}

fn format_row(target: &str, spell: &str, secs: i64) -> String {
    format!("{} {} {:>4}", fit(target, TARGET_COLS), fit(spell, SPELL_COLS), secs)
}

fn row_colour(t: &Timer) -> [u8; 3] {
    let secs = t.remaining_ms.div_euclid(1000);
    if secs <= CRITICAL_SECS {
        CRITICAL
    } else if secs <= WARNING_SECS {
        WARNING
    } else if t.confidence == Confidence::Estimated {
        DIM
    } else {
        WHITE
    }
}

/// The kill count, then at most MAX_ROWS timers as the daemon ordered them.
fn hud_lines(snap: &Snapshot) -> Vec<text::Line> {
    let mut lines = vec![text::Line { text: format!("{} kills", snap.session_kills), rgb: WHITE }];
    for t in snap.timers.iter().take(MAX_ROWS) {
        let spell = if t.rank == 0 { t.spell.clone() } else { format!("{} {}", t.spell, roman(t.rank)) };
        // Clamped to what the `{:>4}` column (and the startup probe's width)
        // can hold: a freshly seeded timer for one of the longer-capped
        // spells can seed above 9999 s.
        let secs = t.remaining_ms.div_euclid(1000).clamp(0, 9999);
        lines.push(text::Line { text: format_row(&t.target, &spell, secs), rgb: row_colour(t) });
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_proto::TimerKind;

    fn timer(remaining_ms: i64, confidence: Confidence) -> Timer {
        Timer {
            target: "a jeering gargoyle".to_string(),
            spell: "Mesmerization".to_string(),
            rank: 6,
            kind: TimerKind::Mez,
            remaining_ms,
            duration_ms: 38_000,
            confidence,
        }
    }

    #[test]
    fn colour_follows_the_thresholds_and_confidence() {
        assert_eq!(row_colour(&timer(30_000, Confidence::Measured)), WHITE);
        assert_eq!(row_colour(&timer(30_000, Confidence::Estimated)), DIM);
        assert_eq!(row_colour(&timer(10_000, Confidence::Measured)), WARNING);
        assert_eq!(row_colour(&timer(5_999, Confidence::Measured)), CRITICAL);
        assert_eq!(row_colour(&timer(-2_000, Confidence::Measured)), CRITICAL);
    }

    #[test]
    fn rows_are_fixed_width_and_the_rank_is_roman() {
        let snap = Snapshot {
            v: 2, seq: 1, ts: String::new(), lines_ingested: 0, session_kills: 7,
            timers: vec![timer(11_800, Confidence::Measured)],
        };
        let lines = hud_lines(&snap);
        assert_eq!(lines[0].text, "7 kills");
        assert_eq!(lines[1].text, format!("{} {} {:>4}", fit("a jeering gargoyle", 20), fit("Mesmerization VI", 18), 11));
        assert_eq!(fit("a very long mob name indeed", 20).chars().count(), 20);

        // A timer seeded far above the four-digit column (512 eligible
        // spells seed above 9999 s) still renders as "9999", not a wider
        // number that would break the probe's fixed width.
        let snap = Snapshot {
            v: 2, seq: 1, ts: String::new(), lines_ingested: 0, session_kills: 7,
            timers: vec![timer(100_000_000, Confidence::Measured)],
        };
        let lines = hud_lines(&snap);
        assert_eq!(lines[1].text, format!("{} {} {:>4}", fit("a jeering gargoyle", 20), fit("Mesmerization VI", 18), 9999));
    }

    #[test]
    fn at_most_eight_rows_are_drawn() {
        let snap = Snapshot {
            v: 2, seq: 1, ts: String::new(), lines_ingested: 0, session_kills: 0,
            timers: (0..12).map(|i| timer(1000 * i, Confidence::Measured)).collect(),
        };
        assert_eq!(hud_lines(&snap).len(), 1 + MAX_ROWS);
    }
}
