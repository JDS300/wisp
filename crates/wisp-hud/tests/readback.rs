// SPDX-License-Identifier: MIT
//! Readback: does the plain-window backend actually paint a window a real X
//! server can read back?
//!
//! Gated behind `WISP_HUD_READBACK=1` and a live `DISPLAY` -- this is not run
//! by a bare `cargo test`. It spawns `wispd --stub` and `wisp-hud --backend
//! plain`, both against a scratch environment (the pattern in
//! `crates/wisp/tests/cli.rs`, copied rather than shared: a different crate,
//! and not worth a dev-dependency on the other package's test helpers), waits
//! for the HUD to draw at least one frame, then connects to the X server
//! itself and reads the window's own pixels back with `get_image`.
//!
//! On a real desktop this opens one small, undecorated, click-through window
//! for about two seconds and closes it -- the only window this suite is
//! allowed to open. CI runs it under `xvfb-run` with `WISP_HUD_READBACK=1` set
//! (see `.github/workflows/ci.yml`).
//!
//! Until Task 7 replaces the renderer, the old HUD still draws a small frame
//! at (0, 0) of the now-output-sized window, so this only checks that some
//! pixel came out non-transparent -- Task 7 tightens it to the panel colour
//! at a specific pixel.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, ConnectionExt, ImageFormat, ImageOrder, Window};
use x11rb::rust_connection::RustConnection;

const TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn plain_window_backend_paints_a_window_a_real_x_server_can_read_back() {
    if std::env::var_os("WISP_HUD_READBACK").is_none() {
        println!("SKIP plain_window_backend_paints_a_window_a_real_x_server_can_read_back: WISP_HUD_READBACK is not set");
        return;
    }
    if std::env::var_os("DISPLAY").is_none() {
        println!("SKIP plain_window_backend_paints_a_window_a_real_x_server_can_read_back: DISPLAY is not set");
        return;
    }

    let scratch = Scratch::new("readback");
    let _wispd = scratch.spawn(&bin("wispd"), &["--stub"]);
    wait_for_socket(&scratch.socket());

    // [hud] is not yet a section this config parser understands (Spec 5
    // Task 3 adds that); on this base commit a `[hud]` line has no `=` and is
    // silently skipped, so the flat `scale = 1.0` right after it is read
    // exactly as it would be without the header.
    scratch.write_config("[hud]\nscale = 1.0\n");
    let _hud = scratch.spawn(&bin("wisp-hud"), &["--backend", "plain"]);

    // Long enough for the HUD to connect, attach its window, and draw at
    // least one frame; short enough that a hung child fails this one test
    // rather than the suite.
    std::thread::sleep(Duration::from_millis(1500));

    let (conn, screen_num) = x11rb::connect(None).expect("connect to the X server at $DISPLAY");
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let root_size = (screen.width_in_pixels as u32, screen.height_in_pixels as u32);

    let window = find_hud_window(&conn, root)
        .unwrap_or_else(|| panic!("no window named wisp-hud found under the root window within {TIMEOUT:?}"));

    let geom = conn.get_geometry(window).unwrap().reply().unwrap();
    assert_eq!(
        (geom.width as u32, geom.height as u32),
        root_size,
        "the HUD window is sized to the root window"
    );

    let image = conn
        .get_image(ImageFormat::Z_PIXMAP, window, 0, 0, geom.width, geom.height, !0)
        .unwrap()
        .reply()
        .unwrap();

    let msb_first = conn.setup().image_byte_order == ImageOrder::MSB_FIRST;
    let alpha_offset = if msb_first { 0 } else { 3 };
    let any_opaque = image.data.chunks_exact(4).any(|px| px[alpha_offset] != 0);
    assert!(any_opaque, "expected at least one non-transparent pixel in the HUD window");
}

// ---------------------------------------------------------------------------
// Finding the window
// ---------------------------------------------------------------------------

/// Breadth-first search of the window tree under `root` for a window whose
/// `_NET_WM_NAME` is exactly `wisp-hud` (the name `plain_window.rs` sets in
/// `attach`). A window manager commonly reparents a top-level window under a
/// frame of its own, so this walks the whole tree rather than just root's
/// direct children.
fn find_hud_window(conn: &RustConnection, root: Window) -> Option<Window> {
    let name_atom = intern(conn, "_NET_WM_NAME")?;
    let utf8_atom = intern(conn, "UTF8_STRING")?;

    let deadline = Instant::now() + TIMEOUT;
    loop {
        let mut queue = vec![root];
        while let Some(win) = queue.pop() {
            let Ok(tree) = conn.query_tree(win).ok()?.reply() else { continue };
            for child in tree.children {
                if window_name(conn, child, name_atom, utf8_atom).as_deref() == Some("wisp-hud") {
                    return Some(child);
                }
                queue.push(child);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        sleep_ms(50);
    }
}

fn window_name(conn: &RustConnection, window: Window, name_atom: Atom, utf8_atom: Atom) -> Option<String> {
    let reply = conn.get_property(false, window, name_atom, utf8_atom, 0, 1024).ok()?.reply().ok()?;
    if reply.value.is_empty() {
        return None;
    }
    String::from_utf8(reply.value).ok()
}

fn intern(conn: &RustConnection, name: &str) -> Option<Atom> {
    conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|reply| reply.atom)
}

// ---------------------------------------------------------------------------
// The scratch tree (the pattern in crates/wisp/tests/cli.rs, copied rather
// than shared -- this is a different crate, and neither is worth a
// dev-dependency to reach the other's test helpers)
// ---------------------------------------------------------------------------

struct Scratch {
    root: PathBuf,
    runtime: PathBuf,
    config_home: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("wisp-hud-readback-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let runtime = root.join("runtime");
        let config_home = root.join("config");
        for dir in [&runtime, &config_home] {
            fs::create_dir_all(dir).unwrap();
        }
        Scratch { root, runtime, config_home }
    }

    fn socket(&self) -> PathBuf {
        self.runtime.join("wisp").join("wispd.sock")
    }

    fn write_config(&self, text: &str) {
        let path = self.config_home.join("wisp").join("config");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
    }

    fn spawn(&self, program: &Path, args: &[&str]) -> Running {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_CONFIG_HOME", &self.config_home)
            .env_remove("FLATPAK_ID")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        Running::spawn(cmd)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A spawned child and its captured stderr. `Drop` kills it, so a failed
/// assertion cannot leave a daemon or a window behind for the next run.
struct Running {
    child: Child,
    #[allow(dead_code)] // kept so a future assertion can inspect it; not read today.
    stderr: Arc<Mutex<String>>,
}

impl Running {
    fn spawn(mut cmd: Command) -> Running {
        let mut child = cmd.spawn().expect("the binary spawns");
        let pipe = child.stderr.take().expect("stderr is piped");
        let stderr: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&stderr);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => sink.lock().unwrap().push_str(&line),
                }
            }
        });
        Running { child, stderr }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_for_socket(path: &Path) {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return;
        }
        sleep_ms(20);
    }
    panic!("nothing listened on {} within {TIMEOUT:?}", path.display());
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

/// `wisp-hud`'s own binary, in this build's target directory.
fn wisp_hud() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_wisp-hud"))
}

/// One of `wisp-hud`'s siblings in the same target directory: `wispd` is a
/// different workspace member, so there is no `CARGO_BIN_EXE_wispd` here, the
/// way `crates/wisp/tests/cli.rs` finds its own siblings.
fn bin(name: &str) -> PathBuf {
    let dir = wisp_hud().parent().expect("the wisp-hud binary is in a directory").to_path_buf();
    let path = dir.join(name);
    assert!(
        path.is_file(),
        "{} is missing: run `cargo build --workspace` before `cargo test -p wisp-hud`",
        path.display()
    );
    path
}
