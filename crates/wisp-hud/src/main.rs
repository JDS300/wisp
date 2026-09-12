// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/main.rs
mod backend;
mod draw;
mod evdev;
mod hud_mode;
mod keys;
mod model;
mod paint;
mod reload;
mod theme;
mod tray;

use backend::OverlayBackend;
use hud_mode::{Action, HudMode};
use keys::{Chord, Edges, Key, Keyboard};
use paint::HudModeView;
use reload::ConfigWatch;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};
use theme::Theme;
use wisp_config::config::{Config, Key as ConfigKey};
use wisp_config::layout::Layout;
use wisp_config::write::write_atomic;
use wisp_probe::BackendKind;
use wisp_proto::client::StreamEnd;
use wisp_proto::{Snapshot, PROTOCOL_VERSION};

/// A snapshot as `model::build` sees the world before the daemon has sent one
/// yet: no kills, no timers, no fight. HUD mode can toggle before the first
/// real snapshot arrives, and the layout still has to paint something.
fn empty_snapshot() -> Snapshot {
    Snapshot { v: PROTOCOL_VERSION, seq: 0, ts: String::new(), log: None, lines_ingested: 0, session_kills: 0, timers: Vec::new(), encounter: None }
}

/// HUD mode's arrow step, unscaled -- the theme's `nudge`/`shift_nudge` are
/// already multiplied by the render scale, which is the wrong number here:
/// moving a block by scaled pixels at scale 2.0 would jump twice as far per
/// key press as at scale 1.0, when the point of the step is to feel the same
/// regardless of how big everything is drawn.
const NUDGE: i32 = 4;
const SHIFT_NUDGE: i32 = 24;

/// How long `next_snapshot_within` may block before the loop comes back
/// around to poll keys and the config watch. Small enough that holding an
/// arrow key still feels immediate, large enough not to spin.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // args_os, not args: OsStr-clean throughout, no String round-trip. Only
    // --backend and --scale need to become text at all, one being a backend
    // name and the other parsed as a float.
    let args: Vec<OsString> = std::env::args_os().collect();
    let backend_flag = or_refuse(flag_value(&args, "backend"));
    let scale_flag = or_refuse(flag_value(&args, "scale"));

    // The config path is resolved once, here, so the HUD's own save (HUD
    // mode's Esc/chord) writes to the same file it read -- `startup_config`
    // does its own lookup and does not hand the path back.
    let config_path = wisp_config::paths::config_path().ok();

    // The config is read once, and everything it has to say about itself is
    // said once here, before any value from it is acted on.
    let (mut config, warnings) = startup_config();
    for warning in &warnings {
        eprintln!("{warning}");
    }
    print_layout_notices(&config);

    // DEFAULT_SCALE is the default only when neither the flag nor the file
    // says otherwise. A present-but-bad value is refused with a clear error and
    // exit 2, not silently swapped for the default -- and the message names
    // whichever of the two it came from.
    let scale = or_refuse(scale_of(resolve(scale_flag.as_deref(), config.get(ConfigKey::Scale))));

    // Neither the flag nor the file said, so detection decides, as before.
    let kind = or_refuse(backend_of(resolve(backend_flag.as_deref(), config.get(ConfigKey::Backend))))
        .unwrap_or_else(|| wisp_probe::detect().kind);
    eprintln!("wisp-hud: backend {kind:?}, scale {scale:.2}");

    let mut layout = config.layout().clone();
    let mut theme = Theme::at(scale);

    // Every backend sizes its own surface to the output; none of them take a
    // width or height (Task 6). `hud.output` names which output that is:
    // layer-shell resolves the name against the live output list and says
    // what it saw on a miss, and the two X11 backends ignore it (an X screen
    // is not a Wayland output). `None` -- what the config does not say -- is
    // the compositor's own choice, as before.
    let output = layout.hud.output.as_deref();
    let mut surface: Box<dyn OverlayBackend> = match kind {
        BackendKind::GamescopeX11 => {
            Box::new(backend::gamescope_x11::GamescopeX11Backend::new(output))
        }
        BackendKind::WlrLayerShell => Box::new(backend::layer_shell::LayerShellBackend::new(output)),
        BackendKind::PlainWindow => Box::new(backend::plain_window::PlainWindowBackend::new(output)),
    };
    // An `Unsupported` attach is exit 2, not the 1 a `?` would give: the
    // display server is working, it just cannot host a translucent
    // screen-sized overlay, and that is the same class of answer as a bad
    // `--backend` or a bad scale -- something written down has to change.
    // Every other attach failure is still an error to report and unwind.
    let screen = match surface.attach() {
        Ok(size) => size,
        Err(e @ backend::BackendError::Unsupported(_)) => {
            eprintln!("wisp-hud: {e}");
            std::process::exit(2);
        }
        Err(e) => return Err(Box::new(e)),
    };
    let mut canvas = draw::Canvas::new(screen.0, screen.1);
    let fonts = draw::Fonts::embedded();
    // The real font metrics, not `Theme::at`'s fonts-less fallback: this is
    // what makes `model::block_height`'s row budget and `paint::draw_panel`'s
    // actual advance the same number (beta.5 fix B).
    theme = theme.with_fonts(&fonts);

    // After the backend is up and before the first frame, so a tray that
    // cannot register has said so before anything is drawn.
    let tray_state = std::sync::Arc::new(std::sync::Mutex::new(tray::tray_state(
        env!("CARGO_PKG_VERSION"),
        &empty_snapshot(),
        false,
        config.layout_error().is_some(),
    )));
    let (tray_events, tray_inbox) = std::sync::mpsc::channel::<tray::TrayEvent>();
    let mut tray = tray::spawn_tray(std::sync::Arc::clone(&tray_state), tray_events);
    // `xdg-open` children, reaped once a frame so a user who keeps clicking
    // "Open config" does not leave a row of zombies behind.
    let mut openers: Vec<std::process::Child> = Vec::new();

    let chord = or_refuse(chord_of(&layout.hud.chord));
    let mut keyboard = Keyboard::open(&chord);
    if keyboard.is_none() {
        eprintln!("wisp-hud: no X display to poll; HUD mode unavailable (use wisp hud or edit the config)");
    }
    let mut edges = Edges::new(Duration::from_millis(400), Duration::from_millis(100));
    let mut hud_mode = HudMode::default();
    let mut watch = config_path.clone().map(ConfigWatch::new);

    let path = wisp_config::paths::socket_path();
    let mut stream = wisp_proto::client::connect(&path)?;
    eprintln!("wisp-hud: connected to {}", path.display());

    let mut session = model::Session::default();
    let mut last_snapshot = empty_snapshot();
    let mut previous_rects: Vec<backend::Rect> = Vec::new();
    // The last keyboard-poll error text already printed, so a poll that
    // keeps failing the same way says so once rather than every 50 ms; a
    // later poll that fails differently -- or succeeds, then fails again --
    // still gets its own line.
    let mut last_keyboard_error: Option<String> = None;
    // The keys currently down, as the layer-shell backend reports them.
    // The polled path builds its own set every tick from `XQueryKeymap`;
    // this one is maintained by events, because that is all there is once
    // the compositor has moved focus to the HUD and polling has gone blind.
    let mut held: std::collections::HashSet<Key> = std::collections::HashSet::new();
    let mut keyboard_taken = false;

    loop {
        let mut redraw = false;

        match stream.next_snapshot_within(POLL_INTERVAL) {
            Ok(Some(snap)) => {
                session.observe(snap.encounter.as_ref());
                last_snapshot = snap;
                redraw = true;
            }
            Ok(None) => {}
            Err(StreamEnd::Closed) => {
                eprintln!("wisp-hud: daemon closed the connection");
                return Ok(());
            }
            Err(e) => {
                eprintln!("wisp-hud: {e}");
                std::process::exit(1);
            }
        }

        let now = Instant::now();

        // Drained once per frame, where the keys are handled, so a menu click
        // and a key press take exactly the same path through HUD mode.
        let mut pending_keys: Vec<Key> = Vec::new();
        for event in tray_inbox.try_iter() {
            match tray_action(event) {
                TrayAction::Key(key) => pending_keys.push(key),
                TrayAction::OpenConfig => openers.extend(open_config(config_path.as_deref())),
                TrayAction::Stop => {
                    if !stop_the_daemon(&path) {
                        // The daemon is already gone; leaving is what the
                        // launcher needs to see to bring the rest down.
                        return Ok(());
                    }
                }
            }
        }
        openers.retain_mut(|child| child.try_wait().ok().flatten().is_none());

        if let Some(watch) = &mut watch {
            match watch.poll(now) {
                Some(Ok(cfg)) => {
                    print_layout_notices(&cfg);
                    if !hud_mode.active {
                        layout = cfg.layout().clone();
                        config = cfg;
                        redraw = true;
                    } else {
                        // A reload during placement would fight the keys, so
                        // the live `layout` HUD mode is editing is left
                        // alone. It is not queued to apply once HUD mode
                        // exits either: exiting always saves (`Chord` and
                        // `Escape` both end in `SaveAndExit`), which writes
                        // the in-progress edit and calls `mark_saved` --
                        // replaying this now-stale `cfg` afterwards would
                        // silently revert what the user just placed. The
                        // notice above is still worth printing, though: it
                        // says once that an edit landed, even though this
                        // particular one did not take.
                    }
                }
                Some(Err(e)) => eprintln!("wisp-hud: config reload failed: {e}"),
                None => {}
            }
        }

        let mut shift_held = false;
        if keyboard_taken {
            // The compositor has the poller's own connection blind (Spec 6
            // §4.5): every key, including the one that leaves the mode,
            // arrives as a `wl_keyboard` event instead.
            for event in surface.drain_keys() {
                if event.pressed {
                    held.insert(event.key);
                } else {
                    held.remove(&event.key);
                }
            }
            shift_held = held.contains(&Key::Shift);
            pending_keys.extend(edges.update(now, &held));
        } else if let Some(kb) = &mut keyboard {
            match kb.poll() {
                Ok(down) => {
                    // A poll that starts working again is worth reporting on
                    // if it fails again later -- a fresh occurrence, not a
                    // continuation of the one already printed.
                    last_keyboard_error = None;
                    shift_held = down.contains(&Key::Shift);
                    pending_keys.extend(edges.update(now, &down));
                }
                Err(e) => {
                    if last_keyboard_error.as_deref() != Some(e.as_str()) {
                        eprintln!("wisp-hud: keyboard poll failed: {e}; HUD mode keys unavailable");
                        last_keyboard_error = Some(e);
                    }
                }
            }
        }

        for key in pending_keys {
            // The one key the loaded config can veto. `Edges` fires `Chord`
            // exactly once per physical press (T7), so this is one line per
            // press; the tray's `ToggleHudMode` reaches here the same way, so
            // it earns the same refusal.
            if let Some(line) = hud_mode_refusal(&hud_mode, key, config.layout_error()) {
                eprintln!("{line}");
                continue;
            }
            let was_active = hud_mode.active;
            let action = hud_mode.handle(key, shift_held, &mut layout, NUDGE, SHIFT_NUDGE);
            if !was_active && hud_mode.active {
                // Entering: ask for the keyboard. `false` on every X11
                // backend and on a compositor that would not give it, and
                // the poller keeps driving the keys exactly as in v0.2.0.
                keyboard_taken = surface.take_keyboard(true);
                held.clear();
            } else if was_active && !hud_mode.active && keyboard_taken {
                // Leaving: give it back, and drop whatever the compositor
                // told us on the way out.
                surface.take_keyboard(false);
                let _ = surface.drain_keys();
                keyboard_taken = false;
                held.clear();
            }
            match action {
                Action::Nothing => {}
                Action::Redraw => redraw = true,
                Action::SaveAndExit => {
                    redraw = true;
                    match &config_path {
                        Some(path) => match save_layout(path, &config, &layout) {
                            Ok(fresh) => {
                                config = fresh;
                                if let Some(watch) = &mut watch {
                                    watch.mark_saved();
                                }
                            }
                            Err(e) => eprintln!("wisp-hud: failed to save config: {e}"),
                        },
                        None => eprintln!("wisp-hud: failed to save config: no config path"),
                    }
                }
            }
        }

        if redraw {
            let views = model::build(&last_snapshot, &layout, &theme, &session, screen);
            let hud_mode_view =
                if hud_mode.active { Some(HudModeView { selected: hud_mode.selected, help: paint::HELP }) } else { None };
            // `paint` hands back what it actually touched, which in HUD mode
            // is more than the block rects: the outline and halo are drawn
            // outside every block, the name tag above it, the help strip
            // along the bottom of the screen. Computing the erase set here
            // from the rects alone is what left all of that smeared on
            // screen and blending towards opaque.
            let last = std::mem::take(&mut previous_rects);
            previous_rects = paint::paint(&mut canvas, &fonts, &theme, &views, &last, hud_mode_view);

            let dirty = canvas.take_dirty();
            if let Err(e) = surface.present(canvas.frame(), &dirty) {
                eprintln!("wisp-hud: {e}");
                std::process::exit(1);
            }
        }

        if let Some(tray) = &mut tray {
            tray.publish(tray::tray_state(
                env!("CARGO_PKG_VERSION"),
                &last_snapshot,
                hud_mode.active,
                // The same `Config` and the same call the HUD-mode refusal
                // reads three hundred lines up (`hud_mode_refusal`), so the
                // icon and the refusal can never disagree, and a live reload
                // that fixes the file turns the icon green again on the next
                // frame with no restart.
                config.layout_error().is_some(),
            ));
        }
    }
}

/// The scale to render at when neither the flag nor the config file says.
/// A multiplier, not a pixel size (Spec 5): 1.0 is the console's own scale.
const DEFAULT_SCALE: f32 = 1.0;

/// Where a startup value was written down, so a bad one can be blamed
/// correctly: a mistyped flag and a typo in the config file are fixed in
/// different places, and the message has to say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Flag,
    Config,
}

/// A flag beats the file; the file beats the default. Returns the winning
/// text and where it came from, so a bad value can be blamed correctly.
///
/// `None` is "neither said", which is not a value: each reader turns it into
/// its own default, and only the reader knows what that is.
fn resolve(flag: Option<&str>, file: Option<&str>) -> Option<(String, Origin)> {
    match (flag, file) {
        (Some(value), _) => Some((value.to_string(), Origin::Flag)),
        (None, Some(value)) => Some((value.to_string(), Origin::Config)),
        (None, None) => None,
    }
}

/// A value refused at startup, before any window is opened: the line for
/// stderr and the code to exit with.
#[derive(Debug, PartialEq, Eq)]
struct Refusal {
    message: String,
    code: i32,
}

/// The two ways a refusal has to speak for a value that left nothing to print.
/// They are different mistakes: `(empty)` is a value the user wrote and left
/// blank (`--scale ""`, or `scale =` in the file), `(missing)` is a flag with
/// nothing after it at all.
const EMPTY: &str = "(empty)";
const MISSING: &str = "(missing)";

/// The refusal for a value that is present but unusable, naming its origin:
/// `--scale` for a flag, the config key for a file value. Both are exit 2, the
/// code for "you invoked me wrongly", so a config typo is as diagnosable as a
/// mistyped flag and neither is a crash.
///
/// An empty value renders as `(empty)`: interpolated raw it would leave the line
/// ending in `": "`, which reads as a message that lost its value rather than as
/// the value the user actually wrote.
fn refusal(origin: Origin, key: &str, value: &str) -> Refusal {
    let value = if value.is_empty() { EMPTY } else { value };
    let message = match origin {
        Origin::Flag => format!("wisp-hud: invalid --{key} value: {value}"),
        Origin::Config => format!("wisp-hud: invalid config {key}: {value}"),
    };
    Refusal { message, code: 2 }
}

/// Say the line and exit. Diverging, so a refusal arm stays an expression in
/// whatever `match` produces the value.
fn refuse(refused: Refusal) -> ! {
    eprintln!("{}", refused.message);
    std::process::exit(refused.code);
}

/// The value, or the refusal printed and acted on. Reads like `?` for a
/// refusal, which `?` cannot be: `main` returns a `Result`, and a refusal is
/// exit 2 with one specific line, not an error to be reported and unwound.
fn or_refuse<T>(result: Result<T, Refusal>) -> T {
    match result {
        Ok(value) => value,
        Err(refused) => refuse(refused),
    }
}

/// The value written after `--<key>`, or `None` when the flag is absent.
///
/// A flag with nothing after it and a value that is not valid UTF-8 are both
/// refusals, not an absent flag. Treating either as absent would run the default
/// and print it as chosen -- a scale or a backend the user did ask about, just
/// not one they can be given, which is worth two exit codes and a line rather
/// than a silent substitution.
fn flag_value(args: &[OsString], key: &str) -> Result<Option<String>, Refusal> {
    let flag = format!("--{key}");
    let Some(i) = args.iter().position(|a| *a == OsStr::new(&flag)) else {
        return Ok(None);
    };
    match args.get(i + 1) {
        None => Err(refusal(Origin::Flag, key, MISSING)),
        Some(value) => match value.to_str() {
            Some(text) => Ok(Some(text.to_string())),
            // Lossy so the line still shows something the user can recognise.
            // The refusal is about the value being unusable, not about its
            // encoding, and one U+FFFD per undecodable byte says that.
            None => Err(refusal(Origin::Flag, key, &value.to_string_lossy())),
        },
    }
}

/// The winning scale, or the refusal it earns. The text is parsed here rather
/// than in `wisp-config` because the message has to repeat the raw value and
/// say where it came from, and only the reader knows both.
fn scale_of(resolved: Option<(String, Origin)>) -> Result<f32, Refusal> {
    let Some((text, origin)) = resolved else {
        return Ok(DEFAULT_SCALE);
    };
    text.parse::<f32>().map_err(|_| refusal(origin, "scale", &text))
}

/// The winning backend, or the refusal it earns. `Ok(None)` is "neither said",
/// which is where `wisp_probe::detect()` comes in -- detection needs a display,
/// so it stays out of this function and out of the tests. The three names are
/// `BackendKind::parse`'s, in `wisp-probe`, so this crate never spells one and
/// `wisp doctor` cannot disagree with what the HUD is about to do.
fn backend_of(resolved: Option<(String, Origin)>) -> Result<Option<BackendKind>, Refusal> {
    let Some((text, origin)) = resolved else {
        return Ok(None);
    };
    match BackendKind::parse(&text) {
        Some(kind) => Ok(Some(kind)),
        None => Err(refusal(origin, "backend", &text)),
    }
}

/// The chord, or the refusal it earns. Always blamed on the config file --
/// `hud.chord` has no flag of its own -- so the origin is fixed rather than
/// threaded through like `scale`'s and `backend`'s.
fn chord_of(text: &str) -> Result<Chord, Refusal> {
    keys::parse_chord(text).map_err(|err| refusal(Origin::Config, "hud.chord", &err))
}

/// The lines Wisp says once about a config's layout: a legacy pixel scale
/// converted to the multiplier, and a layout that failed to parse and fell
/// back to the default. Shared between startup and every reload, since both
/// have the same thing to say about whatever `Config` they just got.
fn print_layout_notices(config: &Config) {
    if let Some((px, f)) = config.converted_scale() {
        eprintln!("wisp-hud: config scale {px} px is now hud.scale {f:.2}");
    }
    if let Some(e) = config.layout_error() {
        eprintln!("wisp-hud: config layout ignored: {e}");
    }
}

/// What one tray event asks of the render loop. `ToggleHudMode` is a
/// `Key::Chord` and nothing else: the chord's path already carries the
/// §4.7 refusal, the save on exit and the edge behaviour, and a second path
/// into HUD mode would be a second set of those rules to keep in step.
#[derive(Debug, PartialEq, Eq)]
enum TrayAction {
    Key(Key),
    OpenConfig,
    Stop,
}

fn tray_action(event: tray::TrayEvent) -> TrayAction {
    match event {
        tray::TrayEvent::ToggleHudMode => TrayAction::Key(Key::Chord),
        tray::TrayEvent::OpenConfig => TrayAction::OpenConfig,
        tray::TrayEvent::Stop => TrayAction::Stop,
    }
}

/// Ask the daemon to stop, exactly as `wisp stop` does: connect, write the
/// one line, and let go. `false` when there was nothing to write to, in
/// which case the caller exits 0 itself and the launcher takes the daemon
/// down with it.
///
/// Nothing is read back: the daemon closing this connection is the same
/// event as the daemon closing the *snapshot* connection, which the render
/// loop is already watching and already knows how to exit on.
fn stop_the_daemon(socket: &Path) -> bool {
    use std::io::Write as _;
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => stream
            .write_all(format!("{}\n", wisp_proto::STOP_LINE).as_bytes())
            .and_then(|()| stream.flush())
            .is_ok(),
        Err(_) => false,
    }
}

/// Open the config file in whatever the desktop associates with it, and do
/// not wait. In the Flatpak `xdg-open` is the portal shim and needs no extra
/// permission.
fn open_config(path: Option<&Path>) -> Option<std::process::Child> {
    use std::process::Stdio;
    let path = match path {
        Some(path) => path,
        None => {
            eprintln!("wisp-hud: cannot open the config: no config path");
            return None;
        }
    };
    match std::process::Command::new("xdg-open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => Some(child),
        Err(e) => {
            eprintln!("wisp-hud: cannot run xdg-open {}: {e}", path.display());
            None
        }
    }
}

/// The line refusing `key`, or `None` to let [`HudMode::handle`] have it.
///
/// HUD mode is unavailable while the config file's layout did not parse. What
/// the HUD is drawing then is `Layout::default_layout`, not the user's
/// layout, so every edit HUD mode offers would be an edit to blocks the file
/// never asked for -- and exiting HUD mode always saves, which would write
/// those defaults over the file the user still has a chance to fix by hand.
/// Refusing the chord is the whole of it: nothing else can enter HUD mode,
/// and once inside it nothing is refused, because a user who is already in
/// there has to be able to get out (`Escape` and `Chord` both end in
/// `SaveAndExit`).
///
/// Not sticky. The error comes from whatever `Config` the last successful
/// read produced, so a live reload that fixes the file makes HUD mode
/// available again with no restart -- and one that breaks it takes HUD mode
/// away again.
fn hud_mode_refusal(hud_mode: &HudMode, key: Key, layout_error: Option<&str>) -> Option<String> {
    if hud_mode.active || key != Key::Chord {
        return None;
    }
    let error = layout_error?;
    Some(format!("wisp-hud: HUD mode unavailable: config: {error}"))
}

/// Saves `layout` into the config file at `path`, for HUD mode's own
/// save-and-exit.
///
/// Re-reads `path` first rather than reusing whatever `Config` `main` has
/// been holding since startup or the last applied reload: a reload that
/// arrived while HUD mode was active is deliberately not applied to that
/// in-memory copy (a live placement session must not have its keys fought by
/// an incoming layout), but an edit to a key the layout does not touch --
/// `wisp config set logs_dir …`, a hand edit in a text editor -- must still
/// survive the HUD's own save rather than being silently overwritten by the
/// stale in-memory `Config`. Only `layout` itself is applied on top of
/// whatever was just read; everything else in the file is that fresh read's,
/// untouched.
///
/// A `path` that cannot be read right now -- deleted, briefly locked,
/// whatever -- falls back to `fallback` (the caller's in-memory `Config`)
/// instead of failing the save outright, with one line saying so.
///
/// Returns the `Config` that was actually written, so the caller can keep
/// its own copy in sync with what is now on disk.
fn save_layout(path: &Path, fallback: &Config, layout: &Layout) -> io::Result<Config> {
    let mut fresh = match fs::read_to_string(path) {
        Ok(text) => Config::parse(&text),
        Err(e) => {
            eprintln!("wisp-hud: could not re-read config before saving ({e}); using the last known config");
            fallback.clone()
        }
    };
    fresh.layout_mut().clone_from(layout);
    // The same post-condition `wisp config set` and `wisp hud` write under:
    // `to_toml` regenerates the whole file, so text that does not read back
    // as this config would cost the user every key it did not mean to touch.
    // A refusal here is one line on stderr and no write -- HUD mode's own
    // placement is still on screen and the file is still whatever it was.
    let text = fresh.to_toml_checked().map_err(io::Error::other)?;
    write_atomic(path, &text)?;
    Ok(fresh)
}

/// The config at `path`, and the lines to say once about it. A key Wisp does
/// not have is reported and skipped; a file that cannot be read at all -- not
/// valid UTF-8, or no permission to open it -- is reported and treated as empty.
/// Neither is fatal: the HUD runs on its defaults with no config file, so it
/// runs on them with a broken one.
fn read_config(path: &Path) -> (Config, Vec<String>) {
    match Config::load(path) {
        Ok(config) => {
            let warnings = config
                .unknown()
                .iter()
                .map(|name| format!("wisp-hud: ignoring unknown config key: {name}"))
                .collect();
            (config, warnings)
        }
        Err(e) => (
            Config::default(),
            vec![format!("wisp-hud: ignoring unreadable config {}: {e}", path.display())],
        ),
    }
}

/// [`read_config`] at the path this environment resolves to. No `HOME` and no
/// absolute `XDG_CONFIG_HOME` means there is no file to read, which is the
/// situation of a user who has never written one: an empty config, and nothing
/// said about it.
fn startup_config() -> (Config, Vec<String>) {
    match wisp_config::paths::config_path() {
        Ok(path) => read_config(&path),
        Err(_) => (Config::default(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tray_event_is_the_chord_or_one_of_the_two_commands() {
        assert_eq!(tray_action(tray::TrayEvent::ToggleHudMode), TrayAction::Key(Key::Chord));
        assert_eq!(tray_action(tray::TrayEvent::OpenConfig), TrayAction::OpenConfig);
        assert_eq!(tray_action(tray::TrayEvent::Stop), TrayAction::Stop);
    }

    #[test]
    fn stopping_writes_the_stop_line_to_a_listening_daemon_and_says_no_to_nothing() {
        let mut path = std::env::temp_dir();
        path.push(format!("wisp-hud-stop-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(stop_the_daemon(&path), "a listening daemon takes the line");
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(stream), &mut line).unwrap();
        assert_eq!(line, format!("{}\n", wisp_proto::STOP_LINE));

        drop(listener);
        std::fs::remove_file(&path).unwrap();
        assert!(!stop_the_daemon(&path), "nothing listening is a `false`, not a panic");
    }

    #[test]
    fn resolve_reports_where_the_value_came_from() {
        // The origin travels with the text: a bad value has to blame the place
        // the user actually wrote it.
        assert_eq!(resolve(Some("32"), Some("16")), Some(("32".to_string(), Origin::Flag)));
        assert_eq!(resolve(Some("32"), None), Some(("32".to_string(), Origin::Flag)));
        assert_eq!(resolve(None, Some("16")), Some(("16".to_string(), Origin::Config)));
        assert_eq!(resolve(None, None), None);
    }

    #[test]
    fn a_scale_flag_beats_the_config_file() {
        let config = Config::parse("[hud]\nscale = 16\n");
        assert_eq!(scale_of(resolve(Some("32"), config.get(ConfigKey::Scale))), Ok(32.0));
    }

    #[test]
    fn the_config_file_is_used_when_there_is_no_flag() {
        // `[hud] scale` (Spec 5's own key) is the multiplier as written, with
        // no Spec-4-pixel conversion — a bare top-level `scale` would be
        // read as that legacy pixel value and converted (see
        // `wisp_config::config`'s own tests), which is not what this test is
        // about.
        let config = Config::parse("[hud]\nscale = 16\n");
        assert_eq!(scale_of(resolve(None, config.get(ConfigKey::Scale))), Ok(16.0));
    }

    #[test]
    fn neither_gives_the_default_of_one() {
        let config = Config::parse("# no scale in here\n");
        assert_eq!(config.get(ConfigKey::Scale), None);
        assert_eq!(scale_of(resolve(None, config.get(ConfigKey::Scale))), Ok(1.0));
        assert_eq!(DEFAULT_SCALE, 1.0);
    }

    #[test]
    fn a_bad_scale_flag_exits_2_naming_the_flag() {
        let config = Config::parse("[hud]\nscale = 16\n");
        let refused = scale_of(resolve(Some("not-a-number"), config.get(ConfigKey::Scale))).unwrap_err();
        assert_eq!(refused.code, 2);
        assert_eq!(refused.message, "wisp-hud: invalid --scale value: not-a-number");
        // The flag won, so the file's usable value is not what gets blamed.
        assert!(!refused.message.contains("config"), "{}", refused.message);
    }

    #[test]
    fn a_bad_scale_in_the_config_exits_2_naming_the_key() {
        let config = Config::parse("scale = not-a-number\n");
        let refused = scale_of(resolve(None, config.get(ConfigKey::Scale))).unwrap_err();
        assert_eq!(refused.code, 2, "the same exit code a bad flag earns");
        assert_eq!(refused.message, "wisp-hud: invalid config scale: not-a-number");
    }

    #[test]
    fn a_backend_flag_beats_the_config_file() {
        let config = Config::parse("backend = plain\n");
        assert_eq!(
            backend_of(resolve(Some("gamescope"), config.get(ConfigKey::Backend))),
            Ok(Some(BackendKind::GamescopeX11))
        );
        assert_eq!(backend_of(resolve(None, config.get(ConfigKey::Backend))), Ok(Some(BackendKind::PlainWindow)));
        // Neither said, so no text wins and detection decides. `detect()` needs
        // a display, which is why it is not part of this function.
        assert_eq!(backend_of(resolve(None, None)), Ok(None));
        // Which names are accepted at all is `BackendKind::parse`'s business and
        // is tested in wisp-probe; here the only question is which source wins.
    }

    #[test]
    fn an_unknown_backend_in_the_config_is_refused_like_an_unknown_flag() {
        let config = Config::parse("backend = nonsense\n");
        let from_file = backend_of(resolve(None, config.get(ConfigKey::Backend))).unwrap_err();
        let from_flag = backend_of(resolve(Some("nonsense"), None)).unwrap_err();
        assert_eq!(from_file.code, 2);
        assert_eq!(from_flag.code, 2);
        assert_eq!(from_file.message, "wisp-hud: invalid config backend: nonsense");
        assert_eq!(from_flag.message, "wisp-hud: invalid --backend value: nonsense");
    }

    #[test]
    fn an_empty_value_is_named_rather_than_printed_as_nothing() {
        // `scale =` in the file and `--scale ""` both arrive as an empty
        // string; interpolated raw, the message would end in ": " and read as
        // a value that went missing rather than the one the user wrote.
        let config = Config::parse("scale =\n");
        assert_eq!(config.get(ConfigKey::Scale), Some(""));
        assert_eq!(
            scale_of(resolve(None, config.get(ConfigKey::Scale))).unwrap_err().message,
            "wisp-hud: invalid config scale: (empty)"
        );
        assert_eq!(
            scale_of(resolve(Some(""), None)).unwrap_err().message,
            "wisp-hud: invalid --scale value: (empty)"
        );
        let config = Config::parse("backend =\n");
        assert_eq!(config.get(ConfigKey::Backend), Some(""));
        assert_eq!(
            backend_of(resolve(None, config.get(ConfigKey::Backend))).unwrap_err().message,
            "wisp-hud: invalid config backend: (empty)"
        );
        assert_eq!(
            backend_of(resolve(Some(""), None)).unwrap_err().message,
            "wisp-hud: invalid --backend value: (empty)"
        );
    }

    #[test]
    fn a_flag_reads_the_value_after_it_and_is_absent_without_it() {
        let args = vec![OsString::from("wisp-hud"), OsString::from("--scale"), OsString::from("32")];
        assert_eq!(flag_value(&args, "scale"), Ok(Some("32".to_string())));
        assert_eq!(flag_value(&args, "backend"), Ok(None), "a flag not given is absent");
        // The value is whatever follows the flag, so the next flag's own name is
        // read as a value and refused by the reader that cannot use it.
        let args = vec![OsString::from("--scale"), OsString::from("--backend")];
        assert_eq!(flag_value(&args, "scale"), Ok(Some("--backend".to_string())));
    }

    #[test]
    fn a_scale_flag_with_nothing_after_it_is_refused_as_missing() {
        // Not `(empty)` and not an absent flag: the user asked for a scale and
        // gave no value, which is its own mistake with its own message.
        let args = vec![OsString::from("wisp-hud"), OsString::from("--scale")];
        let refused = flag_value(&args, "scale").unwrap_err();
        assert_eq!(refused.code, 2);
        assert_eq!(refused.message, "wisp-hud: invalid --scale value: (missing)");
    }

    #[test]
    fn a_backend_flag_with_nothing_after_it_is_refused_as_missing() {
        let args = vec![OsString::from("wisp-hud"), OsString::from("--backend")];
        let refused = flag_value(&args, "backend").unwrap_err();
        assert_eq!(refused.code, 2);
        assert_eq!(refused.message, "wisp-hud: invalid --backend value: (missing)");
    }

    #[test]
    fn a_non_utf8_flag_value_is_refused_with_its_lossy_text() {
        use std::os::unix::ffi::OsStringExt;
        // Undecodable bytes are still a value the user wrote, so the refusal
        // shows what survives of it -- one U+FFFD per byte that did not decode.
        let args = vec![OsString::from("--scale"), OsString::from_vec(b"\xff\xfe".to_vec())];
        let refused = flag_value(&args, "scale").unwrap_err();
        assert_eq!(refused.code, 2, "exit 2 like any other unusable value");
        assert_eq!(refused.message, "wisp-hud: invalid --scale value: \u{fffd}\u{fffd}");
        let args = vec![OsString::from("--backend"), OsString::from_vec(b"pla\xff".to_vec())];
        let refused = flag_value(&args, "backend").unwrap_err();
        assert_eq!(refused.message, "wisp-hud: invalid --backend value: pla\u{fffd}");
    }

    use std::path::PathBuf;

    /// A file of its own per test, since the harness runs them in parallel
    /// inside one process.
    fn scratch_file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wisp-hud-{}-{tag}", std::process::id()))
    }

    #[test]
    fn an_unknown_config_key_is_reported_once_and_ignored() {
        let path = scratch_file("unknown-key");
        fs::write(&path, "nonsense = 1\nscale = 32\nnonsense = 2\nalso_unknown = 3\n").unwrap();
        let (config, warnings) = read_config(&path);
        assert_eq!(
            warnings,
            [
                "wisp-hud: ignoring unknown config key: nonsense".to_string(),
                "wisp-hud: ignoring unknown config key: also_unknown".to_string(),
            ]
        );
        // A duplicate `nonsense` key makes this text invalid TOML, so it
        // reads as the legacy grammar, whose `scale` is now Spec 5's
        // converted multiplier (32 / 13), not the raw text Spec 4 read back.
        assert_eq!(
            config.get(ConfigKey::Scale),
            Some("2.46"),
            "the keys Wisp does have are still read"
        );
        assert_eq!(config.get(ConfigKey::Backend), None);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn an_unreadable_config_is_reported_once_with_the_exact_text_and_treated_as_empty() {
        let path = scratch_file("unreadable-config");
        // 0xff is not a valid UTF-8 leading byte, so `Config::load` cannot
        // return text. The values in the file are unusable, not merely absent.
        fs::write(&path, b"scale = \xff\xfe\nbackend = plain\n").unwrap();
        let err = Config::load(&path).expect_err("the file is not text");
        let (config, warnings) = read_config(&path);
        assert_eq!(
            warnings,
            vec![format!("wisp-hud: ignoring unreadable config {}: {err}", path.display())],
            "the real path and the real io::Error, once"
        );
        assert_eq!(config, Config::default());
        // Both keys fall back to their defaults: the file's `backend = plain`
        // is behind the same read that failed.
        assert_eq!(scale_of(resolve(None, config.get(ConfigKey::Scale))), Ok(DEFAULT_SCALE));
        assert_eq!(backend_of(resolve(None, config.get(ConfigKey::Backend))), Ok(None));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_bad_chord_is_named_in_the_refusal() {
        let refused = chord_of("ctrl+super").unwrap_err();
        assert_eq!(refused.code, 2);
        assert_eq!(refused.message, "wisp-hud: invalid config hud.chord: super");
    }

    #[test]
    fn a_good_chord_parses() {
        assert_eq!(chord_of("ctrl+shift+grave").unwrap(), keys::Chord { ctrl: true, shift: true, alt: false, key: "grave".to_string() });
    }

    #[test]
    fn the_chord_is_refused_while_the_config_layout_did_not_parse() {
        // What the HUD draws with a layout error is `Layout::default_layout`,
        // not the user's layout, and leaving HUD mode always saves -- so
        // entering it at all would put the user one Esc away from writing the
        // default two blocks over a file they can still fix by hand.
        let broken = Config::parse("[[block]]\nkind = \"meter\"\nanchor = \"bottm-left\"\n");
        let error = broken.layout_error().expect("a mistyped anchor is a layout error");

        let mut hud_mode = HudMode::default();
        let line = hud_mode_refusal(&hud_mode, Key::Chord, Some(error)).expect("the chord is refused");
        assert!(line.starts_with("wisp-hud: HUD mode unavailable: config: "), "{line}");
        assert!(line.contains("[[block]] 0:"), "it names the block, as the CLI writers do: {line}");
        assert!(!hud_mode.active, "the chord never reached HudMode::handle");

        // Only the chord, and only from outside: a user already inside HUD
        // mode must still be able to get out.
        assert_eq!(hud_mode_refusal(&hud_mode, Key::Escape, Some(error)), None);
        assert_eq!(hud_mode_refusal(&hud_mode, Key::H, Some(error)), None);
        let inside = HudMode { active: true, selected: 0 };
        assert_eq!(hud_mode_refusal(&inside, Key::Chord, Some(error)), None);

        // A reload that fixes the file makes HUD mode available again, with
        // no restart: the error is read from whatever `Config` main is
        // holding, and nothing here remembers.
        let fixed = Config::parse("[[block]]\nkind = \"meter\"\nanchor = \"bottom-left\"\n");
        assert_eq!(fixed.layout_error(), None);
        assert_eq!(hud_mode_refusal(&hud_mode, Key::Chord, fixed.layout_error()), None);
        let mut layout = fixed.layout().clone();
        assert_eq!(
            hud_mode.handle(Key::Chord, false, &mut layout, NUDGE, SHIFT_NUDGE),
            Action::Redraw
        );
        assert!(hud_mode.active, "the chord enters HUD mode once the layout parses");
    }

    #[test]
    fn saving_re_reads_the_file_so_a_concurrent_edit_survives() {
        let path = scratch_file("save-re-reads");
        // What `main` had in memory since startup: no `log` key at all.
        let fallback = Config::parse("logs_dir = \"/a\"\n");
        // While HUD mode was active, something else -- `wisp config set`, a
        // text editor -- wrote a `log` key the in-memory `fallback` above
        // does not have.
        fs::write(&path, "log = \"/b\"\nlogs_dir = \"/a\"\n").unwrap();

        let mut layout = Layout::default_layout();
        layout.blocks[0].offset = [99, 5];

        let written = save_layout(&path, &fallback, &layout).unwrap();
        assert_eq!(written.get(ConfigKey::Log), Some("/b"), "the concurrent edit survived the save");
        assert_eq!(written.layout().blocks[0].offset, [99, 5], "the new layout was applied");

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("log = \"/b\""), "{text}");
        assert!(text.contains("offset = [99, 5]"), "{text}");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn saving_falls_back_to_the_in_memory_config_when_the_file_cannot_be_read() {
        let path = scratch_file("save-missing");
        let _ = fs::remove_file(&path);
        let fallback = Config::parse("logs_dir = \"/a\"\n");
        let layout = Layout::default_layout();

        let written = save_layout(&path, &fallback, &layout).unwrap();
        assert_eq!(written.get(ConfigKey::LogsDir), Some("/a"), "fell back to the in-memory config");
        assert!(fs::read_to_string(&path).unwrap().contains("logs_dir = \"/a\""));

        let _ = fs::remove_file(&path);
    }
}
