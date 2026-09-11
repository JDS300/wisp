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
//! On a real desktop this opens one undecorated, click-through window the
//! size of the screen (Task 6 sized the surface to the output) for about two
//! seconds and closes it -- the only window this suite is allowed to open. CI
//! runs it under `xvfb-run` with `WISP_HUD_READBACK=1` set (see
//! `.github/workflows/ci.yml`).
//!
//! The check itself is one pixel: (24, 141) in the top-left meter block
//! (`Layout::default_layout`'s block at offset `[20, 120]`, scale 1.0) --
//! past the panel's 1 px border, past the header strip's own 20 px band (its
//! left padding puts any text at x >= 28, well clear of x = 24), and above
//! the first row -- so it is `theme.panel` over transparent and nothing
//! else. The test still waits for the stub's fight to be active (its first
//! 45 s in every 90 s cycle) before reading it, matching the pixel the brief
//! names, even though this particular one does not move with the fight.
//!
//! On an X server that offers no depth-32 visual the HUD refuses to start at
//! all (`x11_common.rs`: a screen-sized window with no alpha channel is an
//! opaque screen), so there is no window to read back. That is not a skip --
//! the test asserts the refusal instead, exit 2 and a line naming the missing
//! visual -- so this file says something true under either server and CI
//! cannot go quietly green on a server that cannot run the HUD.

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

    // `[hud] scale = 1.0` is the layout's own scale (Spec 5 Task 3), which is
    // what the pixel this test reads back is positioned for.
    scratch.write_config("[hud]\nscale = 1.0\n");
    let mut hud = scratch.spawn(&bin("wisp-hud"), &["--backend", "plain"]);

    let (conn, screen_num) = x11rb::connect(None).expect("connect to the X server at $DISPLAY");
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let root_size = (screen.width_in_pixels as u32, screen.height_in_pixels as u32);
    let has_depth_32 = screen.allowed_depths.iter().any(|d| d.depth == 32 && !d.visuals.is_empty());
    // So a CI log says which of the two outcomes this server produced.
    println!("server root depth {}, depth-32 visual: {has_depth_32}", screen.root_depth);

    if !has_depth_32 {
        // No alpha channel to be had, so the HUD refuses rather than map an
        // opaque window the size of the screen. There is no window to read;
        // what is asserted instead is that the refusal happened, said why,
        // and exited 2.
        let (status, stderr) = hud.wait_with_output();
        assert_eq!(status.code(), Some(2), "stderr: {stderr}");
        assert!(stderr.contains("32-bit"), "the refusal names the missing visual: {stderr}");
        assert!(
            find_hud_window(&conn, root).is_none(),
            "the HUD refused and still mapped a window"
        );
        return;
    }

    // Long enough for the HUD to connect, attach its window, and draw at
    // least one frame; short enough that a hung child fails this one test
    // rather than the suite.
    std::thread::sleep(Duration::from_millis(1500));

    let window = find_hud_window(&conn, root)
        .unwrap_or_else(|| panic!("no window named wisp-hud found under the root window within {TIMEOUT:?}"));

    let geom = conn.get_geometry(window).unwrap().reply().unwrap();
    assert_eq!(
        (geom.width as u32, geom.height as u32),
        root_size,
        "the HUD window is sized to the root window"
    );
    assert_eq!(geom.depth, 32, "the window carries real alpha or the HUD does not start");

    let image = conn
        .get_image(ImageFormat::Z_PIXMAP, window, 0, 0, geom.width, geom.height, !0)
        .unwrap()
        .reply()
        .unwrap();

    // `x11_common.rs::frame_to_wire_rect` writes premultiplied RGB with the
    // frame's own alpha, ordered per the server's `image_byte_order`, exactly
    // as read back here.
    let msb_first = conn.setup().image_byte_order == ImageOrder::MSB_FIRST;
    let stride = image.data.len() / geom.height as usize;
    let offset = 141 * stride + 24 * 4;
    let px = &image.data[offset..offset + 4];
    let (r, g, b, a) = if msb_first { (px[1], px[2], px[3], px[0]) } else { (px[2], px[1], px[0], px[3]) };

    // `theme.panel` (`rgba(0x080a0e, 0.74)`) over transparent, as
    // `paint.rs`'s own `paints_the_panel_and_the_top_rows_bar` test checks
    // the same colour with the same tolerance (its 8-bit premultiply/
    // unpremultiply round trip loses at most 1 per channel).
    let close = |got: u8, want: u8| got.abs_diff(want) <= 1;
    // Real alpha: un-premultiply the way `Canvas::pixel` does, to the
    // straight-alpha colour the theme was written in.
    let (sr, sg, sb) = if a == 0 {
        (0, 0, 0)
    } else {
        ((r as u32 * 255 / a as u32) as u8, (g as u32 * 255 / a as u32) as u8, (b as u32 * 255 / a as u32) as u8)
    };
    assert!(
        close(sr, 8) && close(sg, 10) && close(sb, 14) && close(a, 189),
        "straight-alpha [{sr}, {sg}, {sb}, {a}], expected [8, 10, 14, 189] ± 1"
    );

    // And a pixel no block covers is transparent, not the window's own
    // background: with the window the size of the root and `present`
    // uploading only the dirty rects, an opaque background would black out
    // the whole screen and every assertion above would still pass.
    let corner = (geom.height as usize - 1) * stride + (geom.width as usize - 1) * 4;
    let px = &image.data[corner..corner + 4];
    let corner_alpha = if msb_first { px[0] } else { px[3] };
    assert_eq!(corner_alpha, 0, "the bottom-right corner covers no block and must be transparent");
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

impl Running {
    /// Waits for the child to exit (within [`TIMEOUT`]) and returns its status
    /// and everything it printed on stderr.
    fn wait_with_output(&mut self) -> (std::process::ExitStatus, String) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    // The reader thread may still be draining the pipe.
                    sleep_ms(100);
                    return (status, self.stderr.lock().unwrap().clone());
                }
                Ok(None) if Instant::now() < deadline => sleep_ms(20),
                Ok(None) => panic!("the child was still running after {TIMEOUT:?}"),
                Err(e) => panic!("waiting for the child: {e}"),
            }
        }
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
