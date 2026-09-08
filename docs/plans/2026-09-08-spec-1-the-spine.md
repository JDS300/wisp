# Wisp Spec 1 — "The Spine" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a walking skeleton for Wisp — read the EverQuest Legends log, count two things, publish over a Unix socket, and draw the number as an overlay above the running game.

**Architecture:** Two binaries and a shared library. `wispd` tails the log and serves newline-delimited JSON snapshots over a Unix domain socket. `wisp-hud` connects to that socket and draws the number through one of three pluggable overlay backends, chosen automatically from the environment. The split is mandatory, not stylistic: on gamescope the overlay must be a client *inside the game's own XWayland instance*, on a different X display from everything else, so a single-process GUI physically cannot do the job.

**Tech Stack:** Rust (2021 edition), `serde`/`serde_json`, `x11rb` (with the `xfixes` feature), `wayland-client` + `smithay-client-toolkit`, `fontdue`. Standard library for socket and file I/O — no async runtime; this is a 5 Hz workload.

**Spec:** [`docs/specs/2026-09-08-spec-1-the-spine.md`](../specs/2026-09-08-spec-1-the-spine.md)
**Charter (binding):** [`docs/specs/2026-09-08-clean-room-charter.md`](../specs/2026-09-08-clean-room-charter.md)
**Provenance record (must be kept current):** [`PROVENANCE.md`](../../PROVENANCE.md)

---

## Global Constraints

Every task's requirements implicitly include this section. **Read all of it before starting Task 1.**

### Project identity

- Repository: `github.com/JDS300/wisp` — an independent project, **not a fork of anything**.
- Licence: **MIT**, copyright JDS300. Every source file starts with `// SPDX-License-Identifier: MIT`.
- Binaries: `wispd` (daemon), `wisp-hud` (overlay renderer). A `wisp` CLI is deferred to a later spec.
- Target client: **EverQuest Legends**. Not EverQuest Live, not Project Quarm.

### Provenance rules — these are binding, not advisory

Wisp exists because a previous project (`itsspin/spinips`, which JDS300 forked as `JDS300/spinips`) carries **no licence at all**, granting no rights to redistribute. Wisp is the clean, independently-written replacement. Therefore:

- **Do not read, copy, or port source code from `~/gitrepos/spinips`.** Its *behaviour* may inform a decision — "what does it do when two mobs share a name?" — but its expression (code, structure, comments, naming, file layout) must not be copied. Any consultation that informs a decision gets a dated line in `PROVENANCE.md`.
- **Do not read source from `~/gitrepos/EQBuddy`** or any other third-party EverQuest parser. EQBuddy is MIT-licensed and *could* lawfully be reused with attribution, but Wisp has chosen independence. This is recorded in `PROVENANCE.md` under "Disclosed: EQBuddy". If that decision is ever revisited, add a `NOTICE` entry — do not silently copy.
- **`gamescope`, `MangoHud` and `mangoapp` source may be read freely** (BSD-2-Clause and MIT). `mangoapp` is the reference implementation for the overlay mechanism. Record anything vendored in `THIRD_PARTY.md`.
- **Primary sources are always preferred**: the game's own log output, and the EQL wiki.

### Never generalise between EverQuest clients

Project Quarm, EverQuest Live and EverQuest Legends print **differently**. A behaviour is only established for the client whose log demonstrates it. This rule exists because it was already violated once: the spell-rank format was recorded wrongly on day one by reasoning from Quarm.

**Do not use Quarm logs as a source.** They exist on this machine (`eqlog_Dippler_pq.proj.txt`, `eqlog_Daggo_pq.proj.txt`) and are explicitly excluded.

### Git identity and attribution

Repo-local identity is already configured. Verify with `git config user.email` — it must be `70587798+JDS300@users.noreply.github.com`. If it shows `opencode@localhost`, **stop and fix it**; that is a stale agent default that must never appear in this repository.

Every commit containing machine-written code ends with exactly:

```
Co-Authored-By: Claude <noreply@anthropic.com>
```

One canonical form. It is greppable, and being greppable is what makes it evidence of which code was AI-written.

### The invariant: the HUD never takes input

**On any backend, ever. Not focusable, not clickable, not draggable, not resizable by pointer.** All configuration is out of band — CLI flags and a config file.

This is forced by the environment, not chosen for simplicity. EverQuest confines the pointer during right-click mouse-look. JDS300 runs the game under gamescope with `--force-grab-cursor`, which holds the pointer outright; the alternative fix (winecfg fullscreen mouse capture) does the same. **Anything that wants clicks loses to a pointer grab. Something that never wants them cannot lose.**

Consequence: set `GAMESCOPE_NO_FOCUS` *and* an empty X input region. Belt and braces — the spec records that `GAMESCOPE_NO_FOCUS` is verified as *accepted* by gamescope but never verified to actually deliver click-through.

### Verified facts — do not re-derive, do not contradict

Reference fixture (EverQuest Legends, 117 MB, 1,440,036 lines):

```
/mnt/Data4TB/Games/everquest/prefix/drive_c/users/Public/Daybreak Game Company/Installed Games/EverQuest Legends/Logs/eqlog_Daggo_freeport.txt
```

| Fact | Value |
|---|---|
| Line format | `[Mon Aug 10 20:39:54 2026] <text>` — naive local wall clock, **no timezone** |
| `You have slain …!` occurrences | **3,722** — these are the player's own kills |
| `… has been slain by …!` occurrences | **3,065** — third-party kills, **must count for nothing** |
| `Welcome to EverQuest Legends!` occurrences | **39** — session boundary |
| `Rk. II` occurrences | **0** — a Live convention, absent from EQL |
| EQL spell ranks | Bare trailing roman numerals: `Dazzle V`, `Pacify V`, `Swift Like the Wind III` |

The gamescope overlay mechanism, verified by readback from a real X server: an ordinary, unprivileged third-party X11 client launched into gamescope's XWayland accepts both of these atoms.

```
GAMESCOPE_EXTERNAL_OVERLAY(CARDINAL) = 1
GAMESCOPE_NO_FOCUS(CARDINAL) = 1
```

Gamescope's XWayland root window carries ~17 `GAMESCOPE_*` properties, including `GAMESCOPE_XWAYLAND_SERVER_ID` and `GAMESCOPE_FOCUSED_WINDOW`. **This is how backend detection works.**

KWin on the development machine advertises `zwlr_layer_shell_v1` **version 5**. GNOME/Mutter implements no layer-shell.

### The test rig

JDS300 launches EverQuest through gamescope on the desktop. His Lutris config:

```
gamescope: true
gamescope_flags: --force-grab-cursor
gamescope_game_res: 2560x1440
gamescope_output_res: 2560x1440
gamescope_window_mode: -b        # borderless
```

Equivalent manual invocation, known to start cleanly on this machine's NVIDIA 610.57.04 driver:

```bash
gamescope -W 2560 -H 1440 -w 2560 -h 1440 -b --force-grab-cursor -- <command>
```

**Known hazard:** invoking gamescope *without* those flags failed hard here with `NVVM compilation failed: 3` then `vkCreateComputePipelines failed (VkResult: -13)` (`VK_ERROR_INVALID_SHADER_NV`). Pin testing to the invocation above. `vkcube` is a poor stand-in for the game — it cannot drive the gamescope WSI layer — so prefer testing against EverQuest itself, or `glxgears`.

**Hardware:** desktop is Arch/CachyOS, KDE Plasma on Wayland, NVIDIA, three displays with fractional scaling. Handheld target is a **Lenovo Legion Go S running SteamOS** — the same `gamescope-session` and AMD stack as a Steam Deck. gamescope ships `lenovo.legiongos.lcd.lua` for it.

### A tooling hazard on this machine

**Do not use `xdotool` or any X11 input synthesis (`XTEST`).** A hook blocks it. On 2026-08-10 an `XTestFakeKeyEvent` probe tripped KWin's XWayland-EI consent path, kwin lost DRM master, and the desktop froze until a forced reboot. Use `xwininfo` and `xprop` for window discovery and property inspection. To verify a click or a hotkey, **ask JDS300 to perform it**.

---

## File Structure

```
wisp/
├── Cargo.toml                          [workspace] — members, shared profile
├── assets/
│   └── DejaVuSansMono.ttf              vendored font (record in THIRD_PARTY.md)
├── THIRD_PARTY.md                       licences of vendored/studied third-party work
└── crates/
    ├── wisp-proto/
    │   ├── Cargo.toml
    │   └── src/lib.rs                   Snapshot type + NDJSON codec. No I/O.
    ├── wispd/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── main.rs                  CLI parsing, the 5 Hz loop
    │       ├── server.rs                Unix socket, client fan-out
    │       ├── tail.rs                  file following, rotation/truncation
    │       └── rules.rs                 the three parse rules. Pure functions.
    └── wisp-hud/
        ├── Cargo.toml
        └── src/
            ├── main.rs                  CLI parsing, connect, draw loop
            ├── client.rs                socket reader
            ├── text.rs                  glyph rasterisation to RGBA. Pure.
            └── backend/
                ├── mod.rs               OverlayBackend trait + selection. Pure selection fn.
                ├── gamescope_x11.rs     X11 window + the two atoms + empty input region
                ├── layer_shell.rs       zwlr_layer_shell_v1 overlay layer
                └── plain_window.rs      always-on-top fallback
```

**Responsibility boundaries.** `rules.rs` and `text.rs` are pure and carry the bulk of the test coverage — they are where the logic that can be *wrong* lives. `tail.rs`, `server.rs` and the backends are thin I/O shells around them, tested at the integration level. This split is deliberate: rendering and compositing cannot be meaningfully unit-tested, so keep as little decision-making inside them as possible.

---

## Task 1: Workspace and the wire format

**Files:**
- Create: `Cargo.toml`
- Create: `crates/wisp-proto/Cargo.toml`
- Create: `crates/wisp-proto/src/lib.rs`
- Create: `THIRD_PARTY.md`

**Interfaces:**
- Consumes: nothing.
- Produces: `wisp_proto::Snapshot { v: u32, seq: u64, ts: String, lines_ingested: u64, session_kills: u64 }`, `wisp_proto::PROTOCOL_VERSION: u32`, `wisp_proto::encode(&Snapshot) -> String`, `wisp_proto::decode(&str) -> Result<Snapshot, ProtoError>`.

**Design note — why `ts` is a `String`.** It holds the **raw timestamp text from the log line**, e.g. `Mon Aug 10 20:39:54 2026`, with no parsing and no timezone applied. EverQuest writes naive local wall clock. A previous project in this domain stamped that text with a UTC label without converting it, which shifted every displayed time by the local offset and broke de-duplication against its own history. Wisp does not parse timestamps in Spec 1 at all. Spec 2 may, and when it does it will convert rather than relabel.

- [ ] **Step 1: Create the workspace manifest**

```toml
# Cargo.toml
[workspace]
resolver = "2"
members = ["crates/wisp-proto", "crates/wispd", "crates/wisp-hud"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
authors = ["JDS300"]
repository = "https://github.com/JDS300/wisp"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: Create the wisp-proto manifest**

```toml
# crates/wisp-proto/Cargo.toml
[package]
name = "wisp-proto"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 3: Write the failing tests**

```rust
// crates/wisp-proto/src/lib.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Snapshot {
        Snapshot {
            v: PROTOCOL_VERSION,
            seq: 42,
            ts: "Mon Aug 10 20:39:54 2026".to_string(),
            lines_ingested: 10432,
            session_kills: 7,
        }
    }

    #[test]
    fn round_trips() {
        let s = sample();
        assert_eq!(decode(&encode(&s)).unwrap(), s);
    }

    #[test]
    fn encoded_line_ends_with_newline_and_has_no_interior_newline() {
        let line = encode(&sample());
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[test]
    fn rejects_unknown_protocol_version() {
        let line = r#"{"v":99,"seq":1,"ts":"x","lines_ingested":0,"session_kills":0}"#;
        match decode(line) {
            Err(ProtoError::Version { found, expected }) => {
                assert_eq!(found, 99);
                assert_eq!(expected, PROTOCOL_VERSION);
            }
            other => panic!("expected a version error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(matches!(decode("not json"), Err(ProtoError::Json(_))));
    }

    #[test]
    fn timestamp_is_carried_verbatim() {
        // EverQuest writes naive local wall clock. We never parse or relabel it.
        let s = sample();
        let decoded = decode(&encode(&s)).unwrap();
        assert_eq!(decoded.ts, "Mon Aug 10 20:39:54 2026");
    }
}
```

- [ ] **Step 4: Run the tests and confirm they fail to compile**

Run: `cargo test -p wisp-proto`
Expected: FAIL — `cannot find type Snapshot in this scope` and similar. Compilation failure is the correct "red" here.

- [ ] **Step 5: Write the implementation**

Add above the `mod tests` block in `crates/wisp-proto/src/lib.rs`:

```rust
// SPDX-License-Identifier: MIT
//! Wire format shared by `wispd` and `wisp-hud`.
//!
//! Newline-delimited JSON over a Unix domain socket. Chosen for
//! debuggability: `socat - $XDG_RUNTIME_DIR/wisp/wispd.sock` is a complete
//! diagnostic tool, and the boundary can be exercised without a renderer.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Bumped whenever the snapshot shape changes incompatibly.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Protocol version. Receivers refuse anything they do not recognise.
    pub v: u32,
    /// Monotonically increasing per daemon run. Lets a client spot gaps.
    pub seq: u64,
    /// Raw timestamp text from the last consumed log line, e.g.
    /// `Mon Aug 10 20:39:54 2026`. Never parsed, never relabelled.
    pub ts: String,
    pub lines_ingested: u64,
    pub session_kills: u64,
}

#[derive(Debug)]
pub enum ProtoError {
    Version { found: u32, expected: u32 },
    Json(serde_json::Error),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::Version { found, expected } => write!(
                f,
                "unsupported protocol version {found}; this build speaks {expected}"
            ),
            ProtoError::Json(e) => write!(f, "malformed snapshot: {e}"),
        }
    }
}

impl std::error::Error for ProtoError {}

impl From<serde_json::Error> for ProtoError {
    fn from(e: serde_json::Error) -> Self {
        ProtoError::Json(e)
    }
}

/// Serialise one snapshot as a single NDJSON line, newline included.
pub fn encode(snapshot: &Snapshot) -> String {
    let mut line = serde_json::to_string(snapshot).expect("Snapshot is always serialisable");
    line.push('\n');
    line
}

/// Parse one NDJSON line. Refuses unknown protocol versions loudly rather
/// than guessing at a shape it does not understand.
pub fn decode(line: &str) -> Result<Snapshot, ProtoError> {
    let snapshot: Snapshot = serde_json::from_str(line.trim_end_matches('\n'))?;
    if snapshot.v != PROTOCOL_VERSION {
        return Err(ProtoError::Version {
            found: snapshot.v,
            expected: PROTOCOL_VERSION,
        });
    }
    Ok(snapshot)
}
```

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test -p wisp-proto`
Expected: PASS — 5 tests.

- [ ] **Step 7: Create THIRD_PARTY.md**

```markdown
# Third-party work

Recorded per the [clean-room charter](docs/specs/2026-09-08-clean-room-charter.md) §5.

## Studied, not vendored

- **gamescope** — BSD-2-Clause. The overlay mechanism (`GAMESCOPE_EXTERNAL_OVERLAY`,
  `GAMESCOPE_NO_FOCUS`) was established by inspecting gamescope's atoms and behaviour.
- **mangoapp / MangoHud** — MIT. The reference implementation of a third-party
  process compositing an overlay inside gamescope.

## Vendored

*(none yet — a font is added in a later task)*
```

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml crates/wisp-proto THIRD_PARTY.md
git commit -m "$(cat <<'EOF'
Add the workspace and the snapshot wire format

Newline-delimited JSON over a Unix socket, so the boundary can be exercised
with socat alone and before any renderer exists. The data rate is a few
snapshots a second, nowhere near where a binary codec would earn its opacity.

The timestamp is carried as raw log text and never parsed. EverQuest writes
naive local wall clock; a prior project in this domain stamped that text with
a UTC label without converting it, shifting every displayed time by the local
offset. Wisp does not parse timestamps in Spec 1 at all.

decode() refuses an unrecognised protocol version rather than guessing.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: The daemon — socket server and a stub feed

**Files:**
- Create: `crates/wispd/Cargo.toml`
- Create: `crates/wispd/src/main.rs`
- Create: `crates/wispd/src/server.rs`

**Interfaces:**
- Consumes: `wisp_proto::{Snapshot, PROTOCOL_VERSION, encode}`.
- Produces: `wispd::server::socket_path() -> PathBuf`, `wispd::server::Server::bind(&Path) -> io::Result<Server>`, `Server::accept_pending(&mut self, current: &Snapshot)`, `Server::broadcast(&mut self, snapshot: &Snapshot)`.

**Why a stub first.** `wisp-hud` is the component that can fail in a way that invalidates the architecture. Building it against a synthetic feed means the overlay gets proven before any parsing exists, and a bug afterwards has one obvious home.

- [ ] **Step 1: Create the manifest**

```toml
# crates/wispd/Cargo.toml
[package]
name = "wispd"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
wisp-proto = { path = "../wisp-proto" }
serde = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 2: Write the failing tests**

```rust
// crates/wispd/src/server.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    use wisp_proto::{Snapshot, PROTOCOL_VERSION};

    fn snapshot(seq: u64, kills: u64) -> Snapshot {
        Snapshot {
            v: PROTOCOL_VERSION,
            seq,
            ts: "Mon Aug 10 20:39:54 2026".to_string(),
            lines_ingested: seq * 10,
            session_kills: kills,
        }
    }

    fn temp_socket(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("wisp-test-{}-{}.sock", name, std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn new_client_receives_the_current_snapshot_immediately() {
        let path = temp_socket("hello");
        let mut server = Server::bind(&path).unwrap();

        let client = UnixStream::connect(&path).unwrap();
        server.accept_pending(&snapshot(1, 7));

        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();

        let got = wisp_proto::decode(&line).unwrap();
        assert_eq!(got.seq, 1);
        assert_eq!(got.session_kills, 7);
    }

    #[test]
    fn broadcast_reaches_a_connected_client() {
        let path = temp_socket("broadcast");
        let mut server = Server::bind(&path).unwrap();
        let client = UnixStream::connect(&path).unwrap();
        server.accept_pending(&snapshot(1, 0));

        let mut reader = BufReader::new(client);
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();

        server.broadcast(&snapshot(2, 3));
        let mut second = String::new();
        reader.read_line(&mut second).unwrap();

        assert_eq!(wisp_proto::decode(&second).unwrap().session_kills, 3);
    }

    #[test]
    fn a_disconnected_client_does_not_kill_the_server() {
        let path = temp_socket("drop");
        let mut server = Server::bind(&path).unwrap();
        let client = UnixStream::connect(&path).unwrap();
        server.accept_pending(&snapshot(1, 0));
        drop(client);

        // Several broadcasts to a dead peer must not panic; the client is reaped.
        for seq in 2..6 {
            server.broadcast(&snapshot(seq, 0));
        }
        assert_eq!(server.client_count(), 0);
    }

    #[test]
    fn bind_replaces_a_stale_socket_file() {
        let path = temp_socket("stale");
        std::fs::write(&path, b"not a socket").unwrap();
        // Must not fail with EADDRINUSE.
        let _server = Server::bind(&path).unwrap();
    }
}
```

- [ ] **Step 3: Run the tests and confirm they fail**

Run: `cargo test -p wispd`
Expected: FAIL to compile — `Server` does not exist.

- [ ] **Step 4: Implement the server**

Add above the `mod tests` block in `crates/wispd/src/server.rs`:

```rust
// SPDX-License-Identifier: MIT
//! Unix-socket fan-out. One writer, many readers, no back-pressure:
//! a client that cannot keep up is dropped rather than allowed to stall
//! the daemon. Snapshots are cheap and idempotent, so a dropped client
//! simply reconnects and gets the current state.

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::{fs, io};
use wisp_proto::{encode, Snapshot};

/// `$XDG_RUNTIME_DIR/wisp/wispd.sock`, falling back to `/run/user/<uid>`.
pub fn socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc_getuid() })));
    base.join("wisp").join("wispd.sock")
}

// Avoids a libc dependency for one call.
fn libc_getuid() -> u32 {
    // SAFETY: getuid() cannot fail and takes no arguments.
    unsafe { std::mem::transmute::<_, extern "C" fn() -> u32>(getuid as usize)() }
}
extern "C" {
    fn getuid() -> u32;
}

pub struct Server {
    listener: UnixListener,
    clients: Vec<UnixStream>,
}

impl Server {
    pub fn bind(path: &Path) -> io::Result<Server> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // A leftover file from a crashed daemon would cause EADDRINUSE.
        let _ = fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;
        Ok(Server {
            listener,
            clients: Vec::new(),
        })
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Accept every pending connection, sending each the current snapshot at once
    /// so a fresh client is never blank while waiting for the next change.
    pub fn accept_pending(&mut self, current: &Snapshot) {
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    if stream.write_all(encode(current).as_bytes()).is_ok() {
                        let _ = stream.flush();
                        self.clients.push(stream);
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }

    /// Write to every client, reaping any that have gone away.
    pub fn broadcast(&mut self, snapshot: &Snapshot) {
        let line = encode(snapshot);
        self.clients.retain_mut(|stream| {
            stream.write_all(line.as_bytes()).is_ok() && stream.flush().is_ok()
        });
    }
}
```

> **Verify against the installed crate:** `retain_mut` is stable on `Vec` from Rust 1.61. If the toolchain is older, `cargo build` will say so — replace with a manual index loop rather than adding a dependency.

- [ ] **Step 5: Write the daemon main with a stub source**

```rust
// SPDX-License-Identifier: MIT
// crates/wispd/src/main.rs
mod server;

use std::thread::sleep;
use std::time::Duration;
use wisp_proto::{Snapshot, PROTOCOL_VERSION};

const TICK: Duration = Duration::from_millis(200); // 5 Hz

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|a| a == "--stub") {
        eprintln!("wispd: only --stub is implemented so far (Spec 1, Task 2)");
        std::process::exit(2);
    }

    let path = server::socket_path();
    let mut srv = server::Server::bind(&path)?;
    eprintln!("wispd: listening on {}", path.display());
    eprintln!("wispd: try  socat - {}", path.display());

    let mut snapshot = Snapshot {
        v: PROTOCOL_VERSION,
        seq: 0,
        ts: "Mon Aug 10 20:39:54 2026".to_string(),
        lines_ingested: 0,
        session_kills: 0,
    };

    loop {
        snapshot.seq += 1;
        snapshot.lines_ingested += 17;
        if snapshot.seq % 5 == 0 {
            snapshot.session_kills += 1;
        }
        srv.accept_pending(&snapshot);
        srv.broadcast(&snapshot);
        sleep(TICK);
    }
}
```

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test -p wispd`
Expected: PASS — 4 tests.

- [ ] **Step 7: Verify by hand with socat**

Terminal one:

```bash
cargo run -p wispd -- --stub
```

Terminal two:

```bash
socat - "$XDG_RUNTIME_DIR/wisp/wispd.sock"
```

Expected: one JSON line roughly every 200 ms, `seq` rising, `session_kills` incrementing every fifth line. Ctrl-C both.

If `socat` is not installed: `nc -U "$XDG_RUNTIME_DIR/wisp/wispd.sock"`.

- [ ] **Step 8: Commit**

```bash
git add crates/wispd
git commit -m "$(cat <<'EOF'
Add wispd: socket server with a stub feed

One writer, many readers, no back-pressure. A client that cannot keep up is
dropped rather than allowed to stall the daemon -- snapshots are cheap and
idempotent, so a dropped client reconnects and gets current state.

A new client receives the current snapshot on connect, so it is never blank
while waiting for the next change. bind() removes a leftover socket file,
which is otherwise EADDRINUSE after a crash.

The stub feed exists so the overlay renderer can be built and proven before
any log parsing does. The overlay is the only component that can fail in a way
that invalidates the architecture; everything else is understood work.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: The renderer's socket client, print-only

**Files:**
- Create: `crates/wisp-hud/Cargo.toml`
- Create: `crates/wisp-hud/src/main.rs`
- Create: `crates/wisp-hud/src/client.rs`

**Interfaces:**
- Consumes: `wisp_proto::{Snapshot, decode}`, `wispd::server::socket_path` semantics (duplicate the path logic here — the crates do not depend on each other).
- Produces: `wisp_hud::client::connect(&Path) -> io::Result<SnapshotStream>`, `SnapshotStream::next_snapshot(&mut self) -> Option<Result<Snapshot, ProtoError>>`.

This task proves the IPC path end to end **with no display involved**, so any later overlay failure is unambiguously a rendering problem.

- [ ] **Step 1: Create the manifest**

```toml
# crates/wisp-hud/Cargo.toml
[package]
name = "wisp-hud"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
wisp-proto = { path = "../wisp-proto" }
```

- [ ] **Step 2: Write the failing tests**

```rust
// crates/wisp-hud/src/client.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;

    fn temp_socket(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("wisp-hud-test-{}-{}.sock", name, std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn reads_successive_snapshots() {
        let path = temp_socket("read");
        let listener = UnixListener::bind(&path).unwrap();
        let writer = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            for seq in 1..=3 {
                let line = format!(
                    r#"{{"v":1,"seq":{seq},"ts":"t","lines_ingested":{},"session_kills":{seq}}}"#,
                    seq * 10
                );
                sock.write_all(line.as_bytes()).unwrap();
                sock.write_all(b"\n").unwrap();
            }
        });

        let mut stream = connect(&path).unwrap();
        for expected in 1..=3u64 {
            let snap = stream.next_snapshot().unwrap().unwrap();
            assert_eq!(snap.seq, expected);
            assert_eq!(snap.session_kills, expected);
        }
        writer.join().unwrap();
    }

    #[test]
    fn surfaces_a_version_mismatch_rather_than_guessing() {
        let path = temp_socket("version");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let _ = sock.write_all(
                b"{\"v\":99,\"seq\":1,\"ts\":\"t\",\"lines_ingested\":0,\"session_kills\":0}\n",
            );
        });

        let mut stream = connect(&path).unwrap();
        assert!(stream.next_snapshot().unwrap().is_err());
    }

    #[test]
    fn ends_cleanly_when_the_daemon_goes_away() {
        let path = temp_socket("eof");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let (_sock, _) = listener.accept().unwrap();
            // drop immediately -> EOF
        });

        let mut stream = connect(&path).unwrap();
        // Either an immediate None, or None after whatever arrived first.
        while let Some(item) = stream.next_snapshot() {
            let _ = item;
        }
    }
}
```

- [ ] **Step 3: Run the tests and confirm they fail**

Run: `cargo test -p wisp-hud`
Expected: FAIL to compile — `connect` does not exist.

- [ ] **Step 4: Implement the client**

Add above the `mod tests` block in `crates/wisp-hud/src/client.rs`:

```rust
// SPDX-License-Identifier: MIT
//! Reads NDJSON snapshots from wispd.

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::io;
use wisp_proto::{decode, ProtoError, Snapshot};

pub fn socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/user/1000"));
    base.join("wisp").join("wispd.sock")
}

pub struct SnapshotStream {
    reader: BufReader<UnixStream>,
}

pub fn connect(path: &Path) -> io::Result<SnapshotStream> {
    Ok(SnapshotStream {
        reader: BufReader::new(UnixStream::connect(path)?),
    })
}

impl SnapshotStream {
    /// `None` means the daemon closed the connection.
    pub fn next_snapshot(&mut self) -> Option<Result<Snapshot, ProtoError>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(decode(&line)),
            Err(_) => None,
        }
    }
}
```

- [ ] **Step 5: Write a print-only main**

```rust
// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/main.rs
mod client;

fn main() -> std::io::Result<()> {
    let path = client::socket_path();
    let mut stream = client::connect(&path)?;
    eprintln!("wisp-hud: connected to {}", path.display());

    while let Some(item) = stream.next_snapshot() {
        match item {
            Ok(snap) => println!(
                "seq={} lines={} kills={} ts={}",
                snap.seq, snap.lines_ingested, snap.session_kills, snap.ts
            ),
            Err(e) => {
                eprintln!("wisp-hud: {e}");
                std::process::exit(1);
            }
        }
    }
    eprintln!("wisp-hud: daemon closed the connection");
    Ok(())
}
```

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test -p wisp-hud`
Expected: PASS — 3 tests.

- [ ] **Step 7: Verify end to end**

Terminal one: `cargo run -p wispd -- --stub`
Terminal two: `cargo run -p wisp-hud`

Expected: a rising `seq=… lines=… kills=…` line roughly five times a second. **Milestone 1 is complete at this point.**

- [ ] **Step 8: Commit**

```bash
git add crates/wisp-hud
git commit -m "$(cat <<'EOF'
Add the wisp-hud socket client, print-only

Proves the IPC path end to end with no display involved, so that any later
overlay failure is unambiguously a rendering problem rather than an ambiguous
one.

A protocol version it does not recognise is surfaced and fatal, never guessed
at. EOF from the daemon ends the loop cleanly rather than looking like a hang.

Milestone 1 of Spec 1.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Backend selection

**Files:**
- Create: `crates/wisp-hud/src/backend/mod.rs`
- Modify: `crates/wisp-hud/src/main.rs` (add `mod backend;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `backend::BackendKind` (`GamescopeX11 | WlrLayerShell | PlainWindow`), `backend::choose(x_root_atom_names: &[String], wayland_globals: &[String]) -> BackendKind`, `backend::OverlayBackend` trait with `fn attach(&mut self) -> Result<(), BackendError>` and `fn present(&mut self, frame: &Frame)`, `backend::Frame { width: u32, height: u32, rgba: Vec<u8> }`.

**Why selection is a pure function.** It is the one part of backend handling that can be *wrong* in an interesting way, so it is separated from anything that touches a display and tested exhaustively. The detection rule comes directly from an observed fact: gamescope's XWayland root window carries ~17 `GAMESCOPE_*` properties.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/wisp-hud/src/backend/mod.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn gamescope_root_properties_select_the_gamescope_backend() {
        let atoms = s(&[
            "GAMESCOPE_XWAYLAND_SERVER_ID",
            "GAMESCOPE_FOCUSED_WINDOW",
            "WM_NAME",
        ]);
        assert_eq!(choose(&atoms, &s(&[])), BackendKind::GamescopeX11);
    }

    #[test]
    fn gamescope_wins_even_when_layer_shell_is_also_present() {
        // Nested gamescope inside a layer-shell-capable session. The game is
        // inside gamescope, so that is where the overlay must go.
        let atoms = s(&["GAMESCOPE_FOCUSED_APP"]);
        let globals = s(&["zwlr_layer_shell_v1"]);
        assert_eq!(choose(&atoms, &globals), BackendKind::GamescopeX11);
    }

    #[test]
    fn layer_shell_is_used_when_advertised_and_no_gamescope() {
        let globals = s(&["wl_compositor", "zwlr_layer_shell_v1"]);
        assert_eq!(choose(&s(&[]), &globals), BackendKind::WlrLayerShell);
    }

    #[test]
    fn plain_window_is_the_fallback() {
        // GNOME: no layer-shell, no gamescope.
        let globals = s(&["wl_compositor", "xdg_wm_base"]);
        assert_eq!(choose(&s(&[]), &globals), BackendKind::PlainWindow);
    }

    #[test]
    fn ordinary_x11_properties_do_not_trigger_gamescope() {
        let atoms = s(&["WM_NAME", "_NET_SUPPORTED", "GAMESCOPEISH_NOT_REALLY"]);
        // Prefix match is on "GAMESCOPE_" with the underscore, so this is not a hit.
        assert_eq!(choose(&atoms, &s(&[])), BackendKind::PlainWindow);
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wisp-hud`
Expected: FAIL to compile — `choose` does not exist.

- [ ] **Step 3: Implement the trait and selection**

Add above the `mod tests` block in `crates/wisp-hud/src/backend/mod.rs`:

```rust
// SPDX-License-Identifier: MIT
//! Overlay backends and the rule for choosing between them.

use std::fmt;

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
    fn present(&mut self, frame: &Frame);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    GamescopeX11,
    WlrLayerShell,
    PlainWindow,
}

/// Gamescope wins whenever its root properties are visible, even if the outer
/// session also offers layer-shell: the game is inside gamescope, so that is
/// where the overlay has to be.
pub fn choose(x_root_atom_names: &[String], wayland_globals: &[String]) -> BackendKind {
    if x_root_atom_names.iter().any(|a| a.starts_with("GAMESCOPE_")) {
        return BackendKind::GamescopeX11;
    }
    if wayland_globals.iter().any(|g| g == "zwlr_layer_shell_v1") {
        return BackendKind::WlrLayerShell;
    }
    BackendKind::PlainWindow
}
```

Add `mod backend;` to `crates/wisp-hud/src/main.rs`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wisp-hud`
Expected: PASS — 8 tests total in the crate.

- [ ] **Step 5: Commit**

```bash
git add crates/wisp-hud
git commit -m "$(cat <<'EOF'
Add the overlay backend trait and the selection rule

Selection is a pure function over observed capability, separated from anything
that touches a display, because it is the one part of backend handling that can
be wrong in an interesting way.

The rule comes from an observed fact rather than a guess: gamescope's XWayland
root window carries around seventeen GAMESCOPE_* properties, so their presence
is the signal. Gamescope wins even when the outer session also offers
layer-shell, because the game is inside gamescope and that is where the overlay
has to be.

The trait's contract includes the project invariant: every implementation
produces a surface that never takes focus and never receives input.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Text rasterisation

**Files:**
- Create: `crates/wisp-hud/src/text.rs`
- Create: `assets/DejaVuSansMono.ttf` (vendored)
- Modify: `crates/wisp-hud/Cargo.toml` (add `fontdue`)
- Modify: `THIRD_PARTY.md`
- Modify: `crates/wisp-hud/src/main.rs` (add `mod text;`)

**Interfaces:**
- Consumes: `backend::Frame`.
- Produces: `text::Renderer::new(scale_px: f32) -> Renderer`, `text::Renderer::render(&self, s: &str) -> Frame`.

**Why scale is a constructor argument.** The spec names handheld legibility as a risk. A 7-inch 1920×1080 panel and a 2560×1440 desktop are different design problems, and a scale inherited from a desktop default will be unreadable on the handheld. Making it explicit from the first frame means it can never be accidentally implicit.

- [ ] **Step 1: Vendor the font and record it**

```bash
mkdir -p assets
cp /usr/share/fonts/TTF/DejaVuSansMono.ttf assets/DejaVuSansMono.ttf
```

If that path does not exist, find one: `fc-list | grep -i "dejavu sans mono"`. Any permissively-licensed monospace font is acceptable; if you substitute, update the notes below to match.

Append to `THIRD_PARTY.md` under "Vendored":

```markdown
- **DejaVu Sans Mono** (`assets/DejaVuSansMono.ttf`) — DejaVu Fonts License, a
  permissive Bitstream Vera derivative. Used to rasterise HUD text. Chosen
  because it is monospaced, so a changing number does not reflow the overlay.
```

- [ ] **Step 2: Add the dependency**

```bash
cargo add fontdue -p wisp-hud
```

- [ ] **Step 3: Write the failing tests**

```rust
// crates/wisp-hud/src/text.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_non_empty_output_for_non_empty_input() {
        let r = Renderer::new(32.0);
        let frame = r.render("1234");
        assert!(frame.width > 0);
        assert!(frame.height > 0);
        assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    }

    #[test]
    fn some_pixels_are_actually_marked() {
        let r = Renderer::new(32.0);
        let frame = r.render("8");
        let lit = frame.rgba.chunks_exact(4).filter(|px| px[3] > 0).count();
        assert!(lit > 0, "rasterised glyph produced no opaque pixels");
    }

    #[test]
    fn empty_input_produces_an_empty_frame_rather_than_panicking() {
        let r = Renderer::new(32.0);
        let frame = r.render("");
        assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    }

    #[test]
    fn a_larger_scale_produces_a_taller_frame() {
        let small = Renderer::new(16.0).render("123");
        let large = Renderer::new(48.0).render("123");
        assert!(
            large.height > small.height,
            "scale must actually affect output: {} vs {}",
            small.height,
            large.height
        );
    }

    #[test]
    fn monospace_width_is_proportional_to_character_count() {
        let r = Renderer::new(32.0);
        let one = r.render("1").width;
        let four = r.render("1234").width;
        assert!(four > one * 3, "expected roughly 4x, got {one} then {four}");
    }
}
```

- [ ] **Step 4: Run the tests and confirm they fail**

Run: `cargo test -p wisp-hud`
Expected: FAIL to compile — `Renderer` does not exist.

- [ ] **Step 5: Implement the renderer**

Add above the `mod tests` block in `crates/wisp-hud/src/text.rs`:

```rust
// SPDX-License-Identifier: MIT
//! Glyph rasterisation into an RGBA buffer.
//!
//! Pure: no display, no windowing, no I/O beyond the embedded font. This is
//! deliberate -- rendering and compositing cannot be meaningfully unit-tested,
//! so as much decision-making as possible lives here instead.

use crate::backend::Frame;
use fontdue::{Font, FontSettings};

const FONT_BYTES: &[u8] = include_bytes!("../../../assets/DejaVuSansMono.ttf");

pub struct Renderer {
    font: Font,
    scale_px: f32,
}

impl Renderer {
    /// `scale_px` is explicit and has no default. A 7-inch handheld panel and a
    /// 2560x1440 desktop are different legibility problems.
    pub fn new(scale_px: f32) -> Renderer {
        let font = Font::from_bytes(FONT_BYTES, FontSettings::default())
            .expect("vendored font must parse");
        Renderer { font, scale_px }
    }

    pub fn render(&self, text: &str) -> Frame {
        if text.is_empty() {
            return Frame { width: 0, height: 0, rgba: Vec::new() };
        }

        // Rasterise once to measure, once to place. Monospace, so advance is uniform.
        let rasterised: Vec<_> = text
            .chars()
            .map(|c| self.font.rasterize(c, self.scale_px))
            .collect();

        let advance = rasterised
            .iter()
            .map(|(m, _)| m.advance_width.ceil() as u32)
            .max()
            .unwrap_or(1)
            .max(1);
        let height = rasterised
            .iter()
            .map(|(m, _)| (m.height as i32 - m.ymin) as u32)
            .max()
            .unwrap_or(1)
            .max(1);
        let baseline = height;
        let width = advance * rasterised.len() as u32;

        let mut rgba = vec![0u8; (width * height * 4) as usize];

        for (i, (metrics, bitmap)) in rasterised.iter().enumerate() {
            let pen_x = i as u32 * advance;
            for gy in 0..metrics.height {
                for gx in 0..metrics.width {
                    let coverage = bitmap[gy * metrics.width + gx];
                    if coverage == 0 {
                        continue;
                    }
                    let x = pen_x as i32 + metrics.xmin + gx as i32;
                    let y = baseline as i32 - metrics.height as i32 - metrics.ymin + gy as i32;
                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                        continue;
                    }
                    let o = ((y as u32 * width + x as u32) * 4) as usize;
                    // Premultiplied white. X11 and Wayland both want premultiplied alpha.
                    rgba[o] = coverage;
                    rgba[o + 1] = coverage;
                    rgba[o + 2] = coverage;
                    rgba[o + 3] = coverage;
                }
            }
        }

        Frame { width, height, rgba }
    }
}
```

Add `mod text;` to `crates/wisp-hud/src/main.rs`.

> **Verify against the installed crate:** `fontdue`'s `rasterize` returns `(Metrics, Vec<u8>)` where `Metrics` carries `width`, `height`, `xmin`, `ymin`, `advance_width`. Run `cargo doc -p fontdue --open` and confirm before assuming; if a field name differs, adjust rather than working around it.

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test -p wisp-hud`
Expected: PASS — 13 tests total in the crate.

- [ ] **Step 7: Commit**

```bash
git add crates/wisp-hud assets THIRD_PARTY.md
git commit -m "$(cat <<'EOF'
Add glyph rasterisation, with scale as an explicit parameter

Pure: no display, no windowing, no I/O beyond the embedded font. Rendering and
compositing cannot be meaningfully unit-tested, so as much decision-making as
possible lives here instead, where it can be.

Scale is a constructor argument with no default. The spec names handheld
legibility as a risk: a 7-inch 1920x1080 panel and a 2560x1440 desktop are
different design problems, and a scale inherited from a desktop default would
be unreadable on the handheld. Explicit from the first frame means it can never
become accidentally implicit.

Monospace is deliberate -- a changing number must not reflow the overlay.

DejaVu Sans Mono is vendored and recorded in THIRD_PARTY.md per the charter.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: The gamescope X11 backend — Milestone 2

**Files:**
- Create: `crates/wisp-hud/src/backend/gamescope_x11.rs`
- Modify: `crates/wisp-hud/src/backend/mod.rs` (add `pub mod gamescope_x11;`)
- Modify: `crates/wisp-hud/src/main.rs` (wire selection → backend → draw loop)
- Modify: `crates/wisp-hud/Cargo.toml` (add `x11rb`)

**Interfaces:**
- Consumes: `backend::{OverlayBackend, Frame, BackendError}`, `text::Renderer`, `client::SnapshotStream`.
- Produces: `backend::gamescope_x11::GamescopeX11Backend::new(width: u32, height: u32) -> Self`, and its `OverlayBackend` implementation. Also `backend::gamescope_x11::root_atom_names() -> Vec<String>` for use by selection.

**This is the milestone that matters.** Everything before it is scaffolding.

- [ ] **Step 1: Add the dependency**

```bash
cargo add x11rb -p wisp-hud --features xfixes
```

- [ ] **Step 2: Implement root property enumeration**

```rust
// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/gamescope_x11.rs
//! An ordinary X11 window inside gamescope's XWayland, marked as the overlay
//! plane. This is the same mechanism `mangoapp` uses -- a separate process
//! linking libX11 and libGL, setting two atoms -- and gamescope's own help
//! recommends it over drawing inside the game.
//!
//! No injection, no LD_PRELOAD, no Vulkan layer.

use crate::backend::{BackendError, Frame, OverlayBackend};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{self, ConnectionExt as XfixesExt};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

/// Names of every property on the X root window. Selection uses this to detect
/// gamescope, whose XWayland root carries around seventeen GAMESCOPE_* entries.
pub fn root_atom_names() -> Vec<String> {
    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        return Vec::new();
    };
    let root = conn.setup().roots[screen_num].root;
    let Ok(cookie) = conn.list_properties(root) else {
        return Vec::new();
    };
    let Ok(reply) = cookie.reply() else {
        return Vec::new();
    };
    reply
        .atoms
        .iter()
        .filter_map(|&atom| {
            let name = conn.get_atom_name(atom).ok()?.reply().ok()?.name;
            String::from_utf8(name).ok()
        })
        .collect()
}
```

- [ ] **Step 3: Implement the backend**

Append to the same file:

```rust
pub struct GamescopeX11Backend {
    width: u32,
    height: u32,
    conn: Option<RustConnection>,
    window: Window,
    gc: Gcontext,
}

impl GamescopeX11Backend {
    pub fn new(width: u32, height: u32) -> Self {
        GamescopeX11Backend {
            width,
            height,
            conn: None,
            window: 0,
            gc: 0,
        }
    }
}

impl OverlayBackend for GamescopeX11Backend {
    fn attach(&mut self) -> Result<(), BackendError> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|e| BackendError::Unavailable(e.to_string()))?;
        let screen = &conn.setup().roots[screen_num];
        let window = conn
            .generate_id()
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        let values = CreateWindowAux::new()
            .background_pixel(screen.black_pixel)
            // Deliberately no input events. The HUD never takes input.
            .event_mask(EventMask::EXPOSURE)
            // Bypass the window manager entirely; gamescope composites us directly.
            .override_redirect(1u32);

        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            screen.root,
            0,
            0,
            self.width as u16,
            self.height as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &values,
        )
        .map_err(|e| BackendError::Failed(e.to_string()))?;

        // The two atoms that make gamescope treat this as the overlay plane.
        for (name, value) in [
            ("GAMESCOPE_EXTERNAL_OVERLAY", 1u32),
            ("GAMESCOPE_NO_FOCUS", 1u32),
        ] {
            let atom = conn
                .intern_atom(false, name.as_bytes())
                .map_err(|e| BackendError::Failed(e.to_string()))?
                .reply()
                .map_err(|e| BackendError::Failed(e.to_string()))?
                .atom;
            conn.change_property32(PropMode::REPLACE, window, atom, AtomEnum::CARDINAL, &[value])
                .map_err(|e| BackendError::Failed(e.to_string()))?;
        }

        // Belt and braces for the invariant: an empty input region means clicks
        // pass straight through, whatever GAMESCOPE_NO_FOCUS turns out to do.
        // The spec records NO_FOCUS as verified-accepted but not verified to
        // deliver click-through; this does not depend on it.
        conn.xfixes_query_version(5, 0)
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        let region = conn
            .generate_id()
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.xfixes_create_region(region, &[])
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.xfixes_set_window_shape_region(window, xfixes::Region::from(0u8), 0, 0, region)
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.xfixes_destroy_region(region)
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        let gc = conn
            .generate_id()
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.create_gc(gc, window, &CreateGCAux::new())
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        conn.map_window(window)
            .map_err(|e| BackendError::Failed(e.to_string()))?;
        conn.flush()
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        self.conn = Some(conn);
        self.window = window;
        self.gc = gc;
        Ok(())
    }

    fn present(&mut self, frame: &Frame) {
        let Some(conn) = self.conn.as_ref() else { return };
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        let _ = conn.clear_area(false, self.window, 0, 0, 0, 0);
        let _ = conn.put_image(
            ImageFormat::Z_PIXMAP,
            self.window,
            self.gc,
            frame.width as u16,
            frame.height as u16,
            0,
            0,
            0,
            24,
            &frame.rgba,
        );
        let _ = conn.flush();
    }
}
```

> **Verify against the installed crate — three places, all likely to need a small adjustment:**
> 1. `xfixes_set_window_shape_region`'s second argument is a `SK` (shape kind) enum selecting `INPUT`. The exact path in x11rb's generated bindings should be confirmed with `cargo doc -p x11rb --open`; the placeholder above will not compile as written.
> 2. `put_image` expects the server's byte order and depth. Your RGBA buffer is 32-bit; if the visual is depth 24 you must either use a 32-bit ARGB visual (preferred, for alpha) or convert. Getting alpha working may require selecting a 32-bit visual explicitly with a matching colormap.
> 3. `override_redirect` may need to be a `bool`-ish value depending on the binding version.
>
> These are exactly the kind of details that must be resolved against the real API rather than guessed. **Do not paper over a compile error by removing the empty input region or an atom** — those are load-bearing.

- [ ] **Step 4: Wire main.rs**

```rust
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
    Ok(())
}
```

- [ ] **Step 5: Build and confirm it compiles**

Run: `cargo build`
Expected: PASS. Resolve any API mismatches flagged above against `cargo doc`.

- [ ] **Step 6: Verify inside gamescope with a stand-in**

```bash
# terminal 1
cargo run -p wispd -- --stub

# terminal 2 — glxgears stands in for the game
gamescope -W 2560 -H 1440 -w 2560 -h 1440 -b -- glxgears
```

Then, with gamescope running, find its display and launch the HUD into it. Gamescope's XWayland is usually `:1`; confirm by checking which display has `GAMESCOPE_*` root properties:

```bash
for d in :1 :2 :3; do
  echo "== $d"; DISPLAY=$d xprop -root 2>/dev/null | grep -c GAMESCOPE_
done

DISPLAY=:1 cargo run -p wisp-hud
```

Expected: the kill count appears over the gears. If it does not, check `xprop -id <window> GAMESCOPE_EXTERNAL_OVERLAY` reads `1`, using `xwininfo -root -children` to find the window id. **Do not use `xdotool`.**

- [ ] **Step 7: Verify over the real game — Milestone 2**

Launch EverQuest through Lutris as normal (gamescope enabled), then start `wispd --stub` and launch `wisp-hud` onto gamescope's display as above.

**Acceptance:**
- A live, updating number is visible over EverQuest.
- The HUD never takes focus.
- **Mouse-look behaves identically with the HUD running and not running.** This is the real test of the invariant — ask JDS300 to confirm it by playing, since input cannot be synthesised on this machine.

Record the outcome in `PROVENANCE.md` under a dated log entry, and update the spec's risk table: `GAMESCOPE_NO_FOCUS` moves from unproven to proven-or-refuted.

- [ ] **Step 8: Commit**

```bash
git add crates/wisp-hud PROVENANCE.md docs/specs
git commit -m "$(cat <<'EOF'
Add the gamescope overlay backend

An ordinary X11 window inside gamescope's XWayland, marked with
GAMESCOPE_EXTERNAL_OVERLAY and GAMESCOPE_NO_FOCUS. This is the same mechanism
mangoapp uses -- a separate process setting two atoms -- and gamescope's own
help recommends it over drawing inside the game. No injection, no LD_PRELOAD,
no Vulkan layer, no reading game memory.

The window also sets an empty XFixes input region. GAMESCOPE_NO_FOCUS is
verified to be accepted by gamescope but was never verified to deliver
click-through, so the invariant does not depend on it: clicks pass through
regardless.

override_redirect bypasses the window manager, since gamescope composites the
overlay plane itself.

Milestone 2 of Spec 1: a live number over EverQuest, in the configuration
JDS300 already plays in.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: The real tailer

**Files:**
- Create: `crates/wispd/src/tail.rs`
- Modify: `crates/wispd/src/main.rs` (add `mod tail;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `tail::Tailer::open(path: &Path, from_start: bool) -> io::Result<Tailer>`, `Tailer::poll(&mut self) -> io::Result<Vec<String>>` returning whole lines newly appended since the last call.

**Rotation and truncation.** EverQuest appends, but the file can be replaced wholesale (a new character, a moved prefix, a manual clear). Track `(dev, ino)` and detect a size that has shrunk below the read offset; in either case reopen from the start.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/wispd/src/tail.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_log(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("wisp-tail-{}-{}.log", name, std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn append(path: &std::path::Path, text: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn reads_lines_appended_after_opening() {
        let path = temp_log("append");
        append(&path, "first\n");
        let mut t = Tailer::open(&path, false).unwrap();
        assert!(t.poll().unwrap().is_empty(), "from_start=false skips history");

        append(&path, "second\nthird\n");
        assert_eq!(t.poll().unwrap(), vec!["second", "third"]);
    }

    #[test]
    fn from_start_reads_existing_content() {
        let path = temp_log("fromstart");
        append(&path, "a\nb\n");
        let mut t = Tailer::open(&path, true).unwrap();
        assert_eq!(t.poll().unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn a_partial_line_is_withheld_until_its_newline_arrives() {
        let path = temp_log("partial");
        append(&path, "x\n");
        let mut t = Tailer::open(&path, true).unwrap();
        assert_eq!(t.poll().unwrap(), vec!["x"]);

        append(&path, "incomp");
        assert!(t.poll().unwrap().is_empty(), "half a line must not be emitted");
        append(&path, "lete\n");
        assert_eq!(t.poll().unwrap(), vec!["incomplete"]);
    }

    #[test]
    fn truncation_is_detected_and_reread_from_the_start() {
        let path = temp_log("truncate");
        append(&path, "one\ntwo\n");
        let mut t = Tailer::open(&path, true).unwrap();
        assert_eq!(t.poll().unwrap(), vec!["one", "two"]);

        std::fs::write(&path, b"fresh\n").unwrap();
        assert_eq!(t.poll().unwrap(), vec!["fresh"]);
    }

    #[test]
    fn replacement_by_a_different_file_is_detected() {
        let path = temp_log("replace");
        append(&path, "old\n");
        let mut t = Tailer::open(&path, true).unwrap();
        assert_eq!(t.poll().unwrap(), vec!["old"]);

        std::fs::remove_file(&path).unwrap();
        append(&path, "brand new\n");
        assert_eq!(t.poll().unwrap(), vec!["brand new"]);
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wispd`
Expected: FAIL to compile — `Tailer` does not exist.

- [ ] **Step 3: Implement the tailer**

Add above the `mod tests` block in `crates/wispd/src/tail.rs`:

```rust
// SPDX-License-Identifier: MIT
//! Follows an append-only log, surviving truncation and replacement.
//!
//! Polled rather than watched with inotify: fewer filesystem edge cases, and
//! at a 5 Hz snapshot rate the latency is invisible.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::io;

pub struct Tailer {
    path: PathBuf,
    file: Option<File>,
    offset: u64,
    ident: Option<(u64, u64)>, // (dev, ino)
    partial: String,
}

impl Tailer {
    pub fn open(path: &Path, from_start: bool) -> io::Result<Tailer> {
        let mut t = Tailer {
            path: path.to_path_buf(),
            file: None,
            offset: 0,
            ident: None,
            partial: String::new(),
        };
        t.reopen(from_start)?;
        Ok(t)
    }

    fn reopen(&mut self, from_start: bool) -> io::Result<()> {
        let file = File::open(&self.path)?;
        let meta = file.metadata()?;
        self.ident = Some((meta.dev(), meta.ino()));
        self.offset = if from_start { 0 } else { meta.len() };
        self.partial.clear();
        self.file = Some(file);
        Ok(())
    }

    /// Whole lines appended since the last call. A trailing partial line is
    /// withheld until its newline arrives -- emitting half a log line would
    /// produce a parse result that is wrong rather than merely late.
    pub fn poll(&mut self) -> io::Result<Vec<String>> {
        // Has the file been replaced, or truncated below our read offset?
        match std::fs::metadata(&self.path) {
            Ok(meta) => {
                let ident = (meta.dev(), meta.ino());
                if Some(ident) != self.ident || meta.len() < self.offset {
                    self.reopen(true)?;
                }
            }
            Err(_) => return Ok(Vec::new()), // gone for now; try again next tick
        }

        let Some(file) = self.file.as_mut() else {
            return Ok(Vec::new());
        };
        file.seek(SeekFrom::Start(self.offset))?;

        let mut buf = Vec::new();
        let read = file.read_to_end(&mut buf)?;
        self.offset += read as u64;
        if read == 0 {
            return Ok(Vec::new());
        }

        // EverQuest logs are effectively ASCII, but never assume: lossy decode
        // keeps one odd byte from killing the daemon.
        self.partial.push_str(&String::from_utf8_lossy(&buf));

        let mut lines = Vec::new();
        while let Some(idx) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=idx).collect();
            lines.push(line.trim_end_matches(['\n', '\r']).to_string());
        }
        Ok(lines)
    }
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wispd`
Expected: PASS — 9 tests total in the crate.

- [ ] **Step 5: Commit**

```bash
git add crates/wispd
git commit -m "$(cat <<'EOF'
Add the log tailer

Polled at 250ms rather than watched with inotify: fewer filesystem edge cases,
and at a 5Hz snapshot rate the latency is invisible.

A trailing partial line is withheld until its newline arrives. Emitting half a
log line would produce a parse result that is wrong rather than merely late,
which is a worse failure.

Replacement and truncation are both detected -- by (dev, ino) and by a length
that has fallen below the read offset -- and both reopen from the start. The
log can be replaced wholesale by a new character, a moved prefix, or a manual
clear.

Decoding is lossy so that one odd byte cannot kill the daemon.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: The parse rules, and the fixture

**Files:**
- Create: `crates/wispd/src/rules.rs`
- Modify: `crates/wispd/src/main.rs` (replace the stub with the real pipeline)

**Interfaces:**
- Consumes: nothing.
- Produces: `rules::Counters { lines_ingested: u64, session_kills: u64, last_ts: String }`, `rules::Counters::apply(&mut self, line: &str)`, `rules::own_kill(line: &str) -> bool`, `rules::session_boundary(line: &str) -> bool`, `rules::timestamp_text(line: &str) -> Option<&str>`.

**The trap this task exists to avoid.** `X has been slain by Y!` is a **third-party** kill and must count for nothing. It is 3,065 of the 6,787 "slain" lines in the fixture — counting it would make the number wrong by 45%. Getting this right on day one sets the standard for Spec 2.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/wispd/src/rules.rs  (tests only for now)
#[cfg(test)]
mod tests {
    use super::*;

    const OWN: &str = "[Mon Aug 10 20:39:54 2026] You have slain a spiderling!";
    const THIRD_PARTY: &str = "[Mon Aug 10 19:16:16 2026] Zantetsu has been slain by Guard Wytiffin!";
    const BOUNDARY: &str = "[Mon Aug 10 18:56:09 2026] Welcome to EverQuest Legends!";
    const CHATTER: &str = "[Mon Aug 10 20:36:03 2026] A large rat bites YOU for 4 points of damage.";

    #[test]
    fn own_kills_are_recognised() {
        assert!(own_kill(OWN));
    }

    #[test]
    fn third_party_kills_are_not_counted() {
        // 3,065 of 6,787 "slain" lines in the reference fixture. Counting these
        // would make session_kills wrong by 45%.
        assert!(!own_kill(THIRD_PARTY));
    }

    #[test]
    fn ordinary_lines_are_not_kills() {
        assert!(!own_kill(CHATTER));
        assert!(!own_kill(BOUNDARY));
    }

    #[test]
    fn the_session_boundary_is_recognised() {
        assert!(session_boundary(BOUNDARY));
        assert!(!session_boundary(OWN));
    }

    #[test]
    fn quarm_boundary_text_is_not_accepted() {
        // Project Quarm prints "Welcome to EverQuest!" -- a different client on a
        // different codebase. Wisp targets EQ Legends and must not match it.
        let quarm = "[Sat Nov 25 10:28:35 2023] Welcome to EverQuest!";
        assert!(!session_boundary(quarm));
    }

    #[test]
    fn the_timestamp_is_extracted_verbatim() {
        assert_eq!(timestamp_text(OWN), Some("Mon Aug 10 20:39:54 2026"));
        assert_eq!(timestamp_text("no bracket here"), None);
    }

    #[test]
    fn counters_accumulate_and_reset_on_a_boundary() {
        let mut c = Counters::default();
        c.apply(OWN);
        c.apply(OWN);
        c.apply(THIRD_PARTY);
        c.apply(CHATTER);
        assert_eq!(c.session_kills, 2);
        assert_eq!(c.lines_ingested, 4, "every line counts, kill or not");

        c.apply(BOUNDARY);
        assert_eq!(c.session_kills, 0, "a new session resets kills");
        assert_eq!(c.lines_ingested, 5, "but not the ingest total");
    }

    #[test]
    fn the_last_timestamp_is_carried() {
        let mut c = Counters::default();
        c.apply(OWN);
        assert_eq!(c.last_ts, "Mon Aug 10 20:39:54 2026");
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wispd`
Expected: FAIL to compile — `own_kill` does not exist.

- [ ] **Step 3: Implement the rules**

Add above the `mod tests` block in `crates/wispd/src/rules.rs`:

```rust
// SPDX-License-Identifier: MIT
//! The three parse rules of Spec 1. Pure functions over a single log line.
//!
//! Every rule here was verified against a real EverQuest Legends log
//! (`eqlog_Daggo_freeport.txt`, 1,440,036 lines). None is inferred from
//! another client -- Quarm, Live and Legends print differently.

/// `[Mon Aug 10 20:39:54 2026] ` -> `Mon Aug 10 20:39:54 2026`.
/// Returned verbatim: EverQuest writes naive local wall clock and Wisp does
/// not parse or relabel it.
pub fn timestamp_text(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    Some(&rest[..end])
}

fn body(line: &str) -> &str {
    match line.find("] ") {
        Some(i) => &line[i + 2..],
        None => line,
    }
}

/// The player's own kill. 3,722 in the reference fixture.
///
/// Deliberately narrow: `X has been slain by Y!` is a *third-party* kill and
/// must not match. It accounts for 3,065 of the 6,787 "slain" lines, so a
/// looser rule would be wrong by 45%.
pub fn own_kill(line: &str) -> bool {
    let b = body(line);
    b.starts_with("You have slain ") && b.ends_with('!')
}

/// Start of a new play session. 39 in the reference fixture.
///
/// EQ Legends prints exactly this. Project Quarm prints "Welcome to
/// EverQuest!" and must not match -- see the charter's rule against
/// generalising between clients.
pub fn session_boundary(line: &str) -> bool {
    body(line) == "Welcome to EverQuest Legends!"
}

#[derive(Debug, Default, Clone)]
pub struct Counters {
    pub lines_ingested: u64,
    pub session_kills: u64,
    pub last_ts: String,
}

impl Counters {
    pub fn apply(&mut self, line: &str) {
        self.lines_ingested += 1;
        if let Some(ts) = timestamp_text(line) {
            self.last_ts.clear();
            self.last_ts.push_str(ts);
        }
        if session_boundary(line) {
            // A session resets what the session measures, not the lifetime
            // ingest total.
            self.session_kills = 0;
            return;
        }
        if own_kill(line) {
            self.session_kills += 1;
        }
    }
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wispd`
Expected: PASS — 17 tests total in the crate.

- [ ] **Step 5: Wire the real pipeline into main**

Replace `crates/wispd/src/main.rs`:

```rust
// SPDX-License-Identifier: MIT
mod rules;
mod server;
mod tail;

use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;
use wisp_proto::{Snapshot, PROTOCOL_VERSION};

const TICK: Duration = Duration::from_millis(250);

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let stub = args.iter().any(|a| a == "--stub");
    let from_start = args.iter().any(|a| a == "--from-start");
    let log: Option<PathBuf> = args
        .iter()
        .position(|a| a == "--log")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);

    if !stub && log.is_none() {
        eprintln!("usage: wispd --log <path> [--from-start]");
        eprintln!("       wispd --stub");
        std::process::exit(2);
    }

    let path = server::socket_path();
    let mut srv = server::Server::bind(&path)?;
    eprintln!("wispd: listening on {}", path.display());

    let mut counters = rules::Counters::default();
    let mut tailer = match &log {
        Some(p) => Some(tail::Tailer::open(p, from_start)?),
        None => None,
    };
    let mut seq = 0u64;

    loop {
        if let Some(t) = tailer.as_mut() {
            for line in t.poll()? {
                counters.apply(&line);
            }
        } else {
            // stub feed
            counters.lines_ingested += 17;
            if seq % 5 == 0 {
                counters.session_kills += 1;
            }
            counters.last_ts = "Mon Aug 10 20:39:54 2026".to_string();
        }

        seq += 1;
        let snapshot = Snapshot {
            v: PROTOCOL_VERSION,
            seq,
            ts: counters.last_ts.clone(),
            lines_ingested: counters.lines_ingested,
            session_kills: counters.session_kills,
        };
        srv.accept_pending(&snapshot);
        srv.broadcast(&snapshot);
        sleep(TICK);
    }
}
```

- [ ] **Step 6: Verify against the fixture — the acceptance criterion**

The fixture path contains spaces; quote it.

```bash
FIXTURE="/mnt/Data4TB/Games/everquest/prefix/drive_c/users/Public/Daybreak Game Company/Installed Games/EverQuest Legends/Logs/eqlog_Daggo_freeport.txt"

# Ground truth, straight from the file:
wc -l < "$FIXTURE"                              # expect 1440036
grep -c 'You have slain' "$FIXTURE"             # expect 3722
grep -c 'has been slain by' "$FIXTURE"          # expect 3065
grep -c 'Welcome to EverQuest Legends!' "$FIXTURE"   # expect 39
```

Then run the daemon over it and read the final snapshot:

```bash
cargo run --release -p wispd -- --log "$FIXTURE" --from-start &
sleep 30
socat -T2 - "$XDG_RUNTIME_DIR/wisp/wispd.sock" | head -1
kill %1
```

**Acceptance, by equality and not by eye:**
- `lines_ingested == 1440036`
- `session_kills` equals the number of `You have slain` lines *after the final* `Welcome to EverQuest Legends!`. To get the expected value:
  ```bash
  awk '/Welcome to EverQuest Legends!/ {n=0} /You have slain /{n++} END{print n}' "$FIXTURE"
  ```
  The daemon's `session_kills` must equal that number exactly.
- Add a note: the whole-file total of `You have slain` is 3,722, but `session_kills` resets 39 times, so it will be smaller. Both numbers are checkable; do not confuse them.

If `--from-start` has not finished reading in 30 seconds, increase the sleep. A 117 MB file should be far faster than that in release mode.

- [ ] **Step 7: Commit**

```bash
git add crates/wispd
git commit -m "$(cat <<'EOF'
Add the parse rules and wire the real pipeline

Three rules, each verified against a real EverQuest Legends log rather than
inferred: every line counts toward the ingest total, "You have slain X!" is an
own kill, and "Welcome to EverQuest Legends!" starts a new session.

The narrow own-kill rule is the point. "X has been slain by Y!" is a
third-party kill and counts for nothing -- it is 3,065 of the 6,787 slain lines
in the fixture, so a looser rule would be wrong by 45%. Getting this right on
day one sets the standard for the encounter model in Spec 2.

The session boundary is matched exactly, and a test asserts that Quarm's
"Welcome to EverQuest!" does NOT match. Quarm is a different client on a
different codebase; the charter forbids generalising between them, after that
mistake was already made once.

A session reset clears what the session measures and not the lifetime ingest
total.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: The layer-shell backend

**Files:**
- Create: `crates/wisp-hud/src/backend/layer_shell.rs`
- Modify: `crates/wisp-hud/src/backend/mod.rs`, `crates/wisp-hud/src/main.rs`, `crates/wisp-hud/Cargo.toml`

**Interfaces:**
- Consumes: `backend::{OverlayBackend, Frame, BackendError}`.
- Produces: `backend::layer_shell::LayerShellBackend::new(width: u32, height: u32) -> Self`, its `OverlayBackend` impl, and `backend::layer_shell::wayland_globals() -> Vec<String>` for selection.

This covers KDE, Sway, Hyprland and river — EverQuest launched **without** gamescope. KWin on the development machine advertises `zwlr_layer_shell_v1` version 5.

- [ ] **Step 1: Add dependencies**

```bash
cargo add wayland-client -p wisp-hud
cargo add smithay-client-toolkit -p wisp-hud
```

- [ ] **Step 2: Implement global enumeration**

`wayland_globals()` connects to the compositor, runs one registry round-trip, and returns the interface names advertised. Selection needs only the names, so this is a small function — and it is what makes `choose()` testable without a compositor.

```rust
// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/layer_shell.rs
//! A zwlr_layer_shell_v1 surface on the overlay layer, for desktop Wayland
//! sessions not running under gamescope.

/// Interface names the compositor advertises. Empty if there is no Wayland
/// display, which is itself a valid answer for selection purposes.
pub fn wayland_globals() -> Vec<String> {
    // Connect, get the registry, round-trip once, collect interface names.
    // See smithay-client-toolkit's registry example; the exact API differs
    // between 0.18 and 0.19, so build against the version cargo resolved.
    todo!("implement per the resolved smithay-client-toolkit version")
}
```

> **This is the one place the plan cannot give you final code.** `smithay-client-toolkit`'s registry and layer-surface APIs changed shape between recent releases, and writing a version-specific incantation here would be worse than useless. Run `cargo doc -p smithay-client-toolkit --open`, follow its `layer_shell` example, and implement:
>
> - a layer surface on `Layer::Overlay`
> - `KeyboardInteractivity::None` — **required by the invariant**
> - exclusive zone `0`, so the HUD never reserves screen space from other windows
> - an **empty input region** via `wl_surface.set_input_region` with an empty `wl_region` — the Wayland equivalent of the XFixes region in Task 6, and equally load-bearing
> - anchor top-left with a small margin, sized to the frame
>
> Replace the `todo!()` before committing. A `todo!()` in a commit is a plan failure.

- [ ] **Step 3: Write a test for the parts that are testable**

Rendering cannot be unit-tested, but selection integration can:

```rust
#[cfg(test)]
mod tests {
    use crate::backend::{choose, BackendKind};

    #[test]
    fn a_kde_style_global_list_selects_layer_shell() {
        let globals: Vec<String> = ["wl_compositor", "wl_shm", "zwlr_layer_shell_v1", "xdg_wm_base"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(choose(&[], &globals), BackendKind::WlrLayerShell);
    }

    #[test]
    fn a_gnome_style_global_list_falls_back_to_plain() {
        let globals: Vec<String> = ["wl_compositor", "wl_shm", "xdg_wm_base"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(choose(&[], &globals), BackendKind::PlainWindow);
    }
}
```

- [ ] **Step 4: Wire it into main's selection and match arm**

Update the `None =>` branch of backend selection to pass real globals:

```rust
None => backend::choose(
    &backend::gamescope_x11::root_atom_names(),
    &backend::layer_shell::wayland_globals(),
),
```

and add the match arm constructing `LayerShellBackend`.

- [ ] **Step 5: Verify on the desktop**

Launch EverQuest through the **no-gamescope** Lutris configuration, fullscreen. Then:

```bash
cargo run -p wispd -- --log "$FIXTURE" &
cargo run -p wisp-hud -- --backend layer-shell
```

**Acceptance:**
- The number is visible above the fullscreen game.
- It never takes focus.
- Ask JDS300 to confirm mouse-look is unaffected.

- [ ] **Step 6: Commit**

```bash
git add crates/wisp-hud
git commit -m "$(cat <<'EOF'
Add the wlr-layer-shell overlay backend

Covers desktop Wayland sessions not running under gamescope: KDE, Sway,
Hyprland, river. KWin on the development machine advertises
zwlr_layer_shell_v1 version 5.

The surface sits on the overlay layer with KeyboardInteractivity::None, an
exclusive zone of zero so it never reserves space from other windows, and an
empty input region -- the Wayland equivalent of the XFixes region the gamescope
backend sets, and equally load-bearing for the invariant that the HUD never
takes input.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: The plain-window fallback

**Files:**
- Create: `crates/wisp-hud/src/backend/plain_window.rs`
- Modify: `crates/wisp-hud/src/backend/mod.rs`, `crates/wisp-hud/src/main.rs`

**Interfaces:**
- Consumes: `backend::{OverlayBackend, Frame, BackendError}`.
- Produces: `backend::plain_window::PlainWindowBackend::new(width: u32, height: u32) -> Self` and its `OverlayBackend` impl.

For GNOME, which implements no layer-shell and has declined to, and for anything else with neither mechanism. An ordinary always-on-top window. It will **not** reliably composite above a fullscreen game — that limitation is inherent, and must be stated in the README rather than papered over.

- [ ] **Step 1: Implement**

Reuse the X11 path from Task 6 with three differences: no `GAMESCOPE_*` atoms, no `override_redirect`, and set `_NET_WM_STATE_ABOVE` instead. Keep the empty XFixes input region — the invariant applies to every backend.

```rust
// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/plain_window.rs
//! Fallback for sessions with neither gamescope nor layer-shell -- notably
//! GNOME, which implements no layer-shell.
//!
//! An ordinary always-on-top window. This will not reliably composite above a
//! fullscreen game; that limitation is inherent to the approach and is stated
//! in the README rather than hidden.
```

Set the above-state by sending a `_NET_WM_STATE` client message to the root window with `_NET_WM_STATE_ADD` and `_NET_WM_STATE_ABOVE`, after mapping.

- [ ] **Step 2: Verify**

```bash
cargo run -p wisp-hud -- --backend plain
```

Expected: a small always-on-top window showing the number, click-through, never focused.

- [ ] **Step 3: Document the limitation in the README**

Add to the "How it draws over the game" section: plain-window mode cannot reliably cover a fullscreen game, and GNOME users should run the game windowed or borderless.

- [ ] **Step 4: Commit**

```bash
git add crates/wisp-hud README.md
git commit -m "$(cat <<'EOF'
Add the plain-window fallback backend

For sessions with neither gamescope nor layer-shell -- notably GNOME, which
implements no layer-shell and has declined to.

It keeps the empty input region, because the invariant that the HUD never takes
input applies to every backend without exception. It does not reliably
composite above a fullscreen game, which is inherent to an ordinary
always-on-top window rather than a bug, and the README now says so instead of
letting a GNOME user discover it during a raid.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Milestone 6 — Legion Go S verification

**Files:**
- Modify: `PROVENANCE.md`, `docs/specs/2026-09-08-spec-1-the-spine.md`, `README.md`

No new code. `GamescopeX11` from Task 6 is the same code path; this task proves it on the handheld and closes the spike's one open item.

- [ ] **Step 1: Build a static-ish release binary**

```bash
cargo build --release
```

Copy `target/release/wispd` and `target/release/wisp-hud` to the Legion Go S. Note the EQ log path there will differ from the desktop.

- [ ] **Step 2: Run under SteamOS**

Launch EverQuest, then run `wispd --log <path>` and `wisp-hud` inside the gamescope session.

- [ ] **Step 3: Record the result**

Whatever happens, write it down:
- If it works: update `PROVENANCE.md` with a dated entry, and change the spec's risk table so `GAMESCOPE_NO_FOCUS` is proven. Update the README to state handheld support as verified rather than claimed.
- If it fails: record exactly how. Do **not** quietly soften the README's claims — the whole point of the provenance discipline is that the record matches reality.

- [ ] **Step 4: Commit**

```bash
git add PROVENANCE.md docs/specs README.md
git commit -m "$(cat <<'EOF'
Record Legion Go S verification of the gamescope overlay

Closes the one open item carried since the feasibility spike. The spike proved
gamescope accepts GAMESCOPE_EXTERNAL_OVERLAY and GAMESCOPE_NO_FOCUS from an
unprivileged third-party X11 client, verified by readback from the X server,
but could never show pixels: gamescope's GPU path fails on the development
machine's NVIDIA driver.

Same code as the desktop, different hardware.

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Self-review

**Spec coverage.** Every section of Spec 1 maps to a task:

| Spec section | Task |
|---|---|
| §4 workspace and protocol | 1 |
| §4 `wispd` socket + stub | 2 |
| §4 `wisp-hud` client | 3 |
| §4 backend trait + detection | 4 |
| §7 handheld legibility risk (explicit scale) | 5 |
| §4 `GamescopeX11`, milestone 2 | 6 |
| §4 tailer, rotation/truncation | 7 |
| §4 the three rules; §6 fixture acceptance | 8 |
| §4 `WlrLayerShell`, milestone 4 | 9 |
| §4 `PlainWindow`, milestone 5 | 10 |
| §5 milestone 6, Legion Go S | 11 |
| §3 no-input invariant | 4, 6, 9, 10 — every backend |

**Known gaps, stated rather than hidden:**

1. **Task 9 contains a `todo!()`.** This is a deliberate exception to the no-placeholders rule and the only one. `smithay-client-toolkit`'s API changed shape between recent releases, and a confidently-wrong incantation would cost more time than an honest gap. The task says exactly what to implement and where to read. **It must be replaced before that task's commit.**
2. **Three x11rb API details in Task 6 are flagged for verification** against the installed crate version — the XFixes shape-kind enum, `put_image` depth/visual handling for alpha, and `override_redirect`'s type. The note names them individually and warns against "fixing" a compile error by dropping an atom or the input region.
3. **Task 10's `_NET_WM_STATE` client message is described, not coded.** It is a well-documented five-word EWMH message and the surrounding X11 code is fully given in Task 6.

**Type consistency:** `Frame`, `BackendError`, `OverlayBackend`, `BackendKind`, `Snapshot`, `Counters`, `Tailer`, `SnapshotStream`, `Renderer` are each defined once and referenced consistently. `socket_path()` is intentionally duplicated in `wispd::server` and `wisp_hud::client` — the crates do not depend on each other, and one path constant is not worth a shared crate.

**Acceptance is by equality, not judgement:** `1440036` lines; own kills `3722` whole-file with the per-session figure derived by the `awk` one-liner in Task 8; `3065` third-party kills counting zero; `39` session resets.

---

## Execution handoff

Plan complete. Two execution options:

1. **Subagent-Driven (recommended)** — a fresh subagent per task, review between tasks, fast iteration. Use `superpowers:subagent-driven-development`.
2. **Inline Execution** — execute tasks in one session with checkpoints. Use `superpowers:executing-plans`.

Start with Task 1. Before doing so, verify `git config user.email` returns the noreply address and **not** `opencode@localhost`.
