// SPDX-License-Identifier: MIT
//! Which overlay mechanism this session offers.
//!
//! Detection is a crate of its own rather than a feature of `wisp-config`
//! because Cargo unifies the features of a shared dependency across workspace
//! members in a single invocation, so `cargo build --workspace` would have
//! linked `x11rb` and `wayland-client` into `wispd` anyway. A crate boundary
//! is the only split Cargo respects.
//!
//! Both probes are pure reads that return an empty list when there is no
//! display; that is a valid answer for selection, not an error. `choose` is a
//! pure function over the two lists, and `detect` keeps everything that led to
//! the choice so a caller can explain itself without knowing the rules.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;

use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_output, wl_registry},
    Connection as WaylandConnection, Dispatch, QueueHandle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    GamescopeX11,
    WlrLayerShell,
    PlainWindow,
}

impl BackendKind {
    /// The backend a name asks for, or `None` for a name that is not one.
    ///
    /// One mapping for every reader -- `wisp-hud`'s `--backend` and the config
    /// file's `backend` key today, `wisp doctor` later -- so a backend cannot be
    /// spelled one way in one place and another way in the next. The names are
    /// the three the HUD has always accepted and nothing else; an unknown name
    /// is the reader's to refuse, because only the reader knows whether it came
    /// from a flag or from the config file.
    pub fn parse(name: &str) -> Option<BackendKind> {
        match name {
            "gamescope" => Some(BackendKind::GamescopeX11),
            "layer-shell" => Some(BackendKind::WlrLayerShell),
            "plain" => Some(BackendKind::PlainWindow),
            _ => None,
        }
    }
}

/// The prefix rule, in the only place it is written down.
fn is_gamescope_atom(name: &str) -> bool {
    name.starts_with("GAMESCOPE_")
}

/// The protocol global the wlr layer-shell extension is advertised under.
fn is_layer_shell_global(name: &str) -> bool {
    name == "zwlr_layer_shell_v1"
}

/// Gamescope wins whenever its root properties are visible, even if the outer
/// session also offers layer-shell: the game is inside gamescope, so that is
/// where the overlay has to be.
pub fn choose(x_root_atom_names: &[String], wayland_globals: &[String]) -> BackendKind {
    if x_root_atom_names
        .iter()
        .map(String::as_str)
        .any(is_gamescope_atom)
    {
        return BackendKind::GamescopeX11;
    }
    if wayland_globals
        .iter()
        .map(String::as_str)
        .any(is_layer_shell_global)
    {
        return BackendKind::WlrLayerShell;
    }
    BackendKind::PlainWindow
}

/// `None` when the X display could not be opened, which is not the same fact
/// as an opened root that carries no `GAMESCOPE_*` property.
fn try_root_atom_names() -> Option<Vec<String>> {
    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen_num].root;
    let reply = conn.list_properties(root).ok()?.reply().ok()?;
    Some(
        reply
            .atoms
            .iter()
            .filter_map(|&atom| {
                let name = conn.get_atom_name(atom).ok()?.reply().ok()?.name;
                String::from_utf8(name).ok()
            })
            .collect(),
    )
}

/// Names of every property on the X root window. Selection uses this to detect
/// gamescope, whose XWayland root carries around seventeen GAMESCOPE_* entries.
///
/// Core `xproto` only: no `xfixes`, which the backend needs and the probe does
/// not. Empty when there is no X display; `detect` tells "no display" apart
/// from "no gamescope property".
pub fn root_atom_names() -> Vec<String> {
    try_root_atom_names().unwrap_or_default()
}

/// `None` when there was no Wayland display to ask, which is not the same fact
/// as a compositor that answered and did not advertise layer-shell.
fn try_wayland_globals() -> Option<Vec<String>> {
    // A minimal Dispatch target that only needs the registry's global list;
    // `registry_queue_init` already does the one round-trip we need.
    struct GlobalsOnly;

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for GlobalsOnly {
        fn event(
            _state: &mut Self,
            _proxy: &wl_registry::WlRegistry,
            _event: wl_registry::Event,
            _data: &GlobalListContents,
            _conn: &WaylandConnection,
            _qh: &QueueHandle<Self>,
        ) {
            // Selection only needs the initial snapshot below.
        }
    }

    let conn = WaylandConnection::connect_to_env().ok()?;
    let (globals, _queue) = registry_queue_init::<GlobalsOnly>(&conn).ok()?;
    Some(
        globals
            .contents()
            .with_list(|list| list.iter().map(|g| g.interface.clone()).collect()),
    )
}

/// Interface names the compositor advertises. Empty if there is no Wayland
/// display, which is itself a valid answer for selection purposes; `detect`
/// tells "no display" apart from "not advertised".
pub fn wayland_globals() -> Vec<String> {
    try_wayland_globals().unwrap_or_default()
}

/// `None` when there was no Wayland display to bind an output on.
fn try_outputs() -> Option<Vec<String>> {
    // A minimal Dispatch target: the registry side is unused (the initial
    // snapshot below is all `outputs` needs), and the output side collects
    // the one event this probe cares about.
    struct OutputNames {
        names: Vec<String>,
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for OutputNames {
        fn event(
            _state: &mut Self,
            _proxy: &wl_registry::WlRegistry,
            _event: wl_registry::Event,
            _data: &GlobalListContents,
            _conn: &WaylandConnection,
            _qh: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<wl_output::WlOutput, ()> for OutputNames {
        fn event(
            state: &mut Self,
            _proxy: &wl_output::WlOutput,
            event: wl_output::Event,
            _data: &(),
            _conn: &WaylandConnection,
            _qh: &QueueHandle<Self>,
        ) {
            if let wl_output::Event::Name { name } = event {
                state.names.push(name);
            }
        }
    }

    let conn = WaylandConnection::connect_to_env().ok()?;
    let (globals, mut queue) = registry_queue_init::<OutputNames>(&conn).ok()?;
    let qh = queue.handle();

    // Every `wl_output` global, named and versioned, from the snapshot
    // `registry_queue_init` already took -- `outputs` opens no surface, so
    // nothing here needs a second look at the registry.
    let output_globals: Vec<(u32, u32)> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|global| global.interface == "wl_output")
            .map(|global| (global.name, global.version))
            .collect()
    });

    // Bound at the lowest of the global's own version and 4, the version
    // that added the `name` event this probe reads; held until the roundtrip
    // below delivers it, since a proxy dropped early gives up nothing sent
    // to the server but a compositor could still race the queue emptying.
    let bound: Vec<wl_output::WlOutput> = output_globals
        .into_iter()
        .map(|(name, version)| globals.registry().bind(name, version.min(4), &qh, ()))
        .collect();

    let mut state = OutputNames { names: Vec::new() };
    queue.roundtrip(&mut state).ok()?;
    drop(bound);
    Some(state.names)
}

/// Names of the Wayland outputs (`wl_output` names) on `$WAYLAND_DISPLAY`,
/// empty when there is no Wayland display. One connection, one roundtrip
/// beyond the registry's own, no surface.
pub fn outputs() -> Vec<String> {
    try_outputs().unwrap_or_default()
}

/// The choice and everything that led to it, so a caller can explain itself
/// without knowing the rules -- including which displays were actually
/// reached, so nobody states as fact a list that was never read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub kind: BackendKind,
    /// `$DISPLAY`, when set and non-empty.
    pub display: Option<String>,
    /// The X connection opened, so the root's properties were really read.
    pub x_connected: bool,
    /// A `GAMESCOPE_*` root property was seen.
    pub gamescope_root: bool,
    /// The Wayland connection opened, so the global list was really read.
    pub wayland_connected: bool,
    /// `zwlr_layer_shell_v1` was advertised.
    pub layer_shell: bool,
}

/// `$DISPLAY` as a doctor prints it. `var_os` + `to_string_lossy` so a
/// non-UTF-8 value still appears instead of vanishing, and an empty value
/// counts as unset so `reason` cannot print `DISPLAY  has`.
fn display_var() -> Option<String> {
    let value = std::env::var_os("DISPLAY")?.to_string_lossy().into_owned();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Opens each display once, runs both probes over it, then `choose`.
pub fn detect() -> Detection {
    let x_root_atom_names = try_root_atom_names();
    let wayland_globals = try_wayland_globals();
    let x_connected = x_root_atom_names.is_some();
    let wayland_connected = wayland_globals.is_some();
    let x_root_atom_names = x_root_atom_names.unwrap_or_default();
    let wayland_globals = wayland_globals.unwrap_or_default();
    Detection {
        kind: choose(&x_root_atom_names, &wayland_globals),
        display: display_var(),
        x_connected,
        gamescope_root: x_root_atom_names
            .iter()
            .map(String::as_str)
            .any(is_gamescope_atom),
        wayland_connected,
        layer_shell: wayland_globals
            .iter()
            .map(String::as_str)
            .any(is_layer_shell_global),
    }
}

impl Detection {
    /// The doctor's parenthetical: both facts `choose` consults, in the order
    /// it consults them, e.g.
    /// `DISPLAY :0 has no GAMESCOPE_* root property; zwlr_layer_shell_v1 advertised`.
    /// A display that could not be opened says so, rather than claiming an
    /// absence nobody observed.
    pub fn reason(&self) -> String {
        let gamescope = match (&self.display, self.x_connected, self.gamescope_root) {
            (Some(d), false, _) => format!("DISPLAY {d} could not be opened"),
            (Some(d), true, true) => format!("DISPLAY {d} has a GAMESCOPE_* root property"),
            (Some(d), true, false) => format!("DISPLAY {d} has no GAMESCOPE_* root property"),
            // Unreachable via detect() -- x11rb needs $DISPLAY -- but the fields are public.
            (None, _, true) => "a GAMESCOPE_* root property was seen".to_string(),
            (None, _, false) => "no DISPLAY, so no GAMESCOPE_* root property".to_string(),
        };
        let layer_shell = match (self.wayland_connected, self.layer_shell) {
            (false, _) => "no Wayland display".to_string(),
            (true, true) => "zwlr_layer_shell_v1 advertised".to_string(),
            (true, false) => "zwlr_layer_shell_v1 not advertised".to_string(),
        };
        format!("{gamescope}; {layer_shell}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn backend_parse_accepts_the_three_names() {
        // The names a user writes, on the command line or in the config file.
        assert_eq!(BackendKind::parse("gamescope"), Some(BackendKind::GamescopeX11));
        assert_eq!(BackendKind::parse("layer-shell"), Some(BackendKind::WlrLayerShell));
        assert_eq!(BackendKind::parse("plain"), Some(BackendKind::PlainWindow));
    }

    #[test]
    fn backend_parse_rejects_everything_else() {
        assert_eq!(BackendKind::parse(""), None);
        assert_eq!(BackendKind::parse("Gamescope"), None, "the names are case-sensitive");
        assert_eq!(BackendKind::parse("x11"), None, "a display server is not a backend name");
        assert_eq!(BackendKind::parse("wayland"), None);
    }

    #[test]
    fn backend_parse_does_not_prefix_match() {
        // `is_gamescope_atom` matches a prefix because the root properties are a
        // family; backend names are not a family, and a name that merely starts
        // with one is a typo the reader has to refuse rather than guess at.
        assert_eq!(BackendKind::parse("layer-shell-extra"), None);
        assert_eq!(BackendKind::parse("plainx"), None);
        assert_eq!(BackendKind::parse("gamescope "), None, "a trailing space is not the name");
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

    #[test]
    fn reason_names_the_display_and_the_gamescope_root_property() {
        let d = Detection {
            kind: BackendKind::GamescopeX11,
            display: Some(":0".to_string()),
            x_connected: true,
            gamescope_root: true,
            wayland_connected: true,
            layer_shell: false,
        };
        assert_eq!(
            d.reason(),
            "DISPLAY :0 has a GAMESCOPE_* root property; zwlr_layer_shell_v1 not advertised"
        );
    }

    #[test]
    fn reason_says_layer_shell_was_advertised() {
        let d = Detection {
            kind: BackendKind::WlrLayerShell,
            display: Some(":0".to_string()),
            x_connected: true,
            gamescope_root: false,
            wayland_connected: true,
            layer_shell: true,
        };
        assert_eq!(
            d.reason(),
            "DISPLAY :0 has no GAMESCOPE_* root property; zwlr_layer_shell_v1 advertised"
        );
    }

    #[test]
    fn reason_explains_the_plain_fallback() {
        // GNOME on a pure Wayland session: no X display to probe at all, but
        // the compositor answered and simply does not offer layer-shell.
        let d = Detection {
            kind: BackendKind::PlainWindow,
            display: None,
            x_connected: false,
            gamescope_root: false,
            wayland_connected: true,
            layer_shell: false,
        };
        assert_eq!(
            d.reason(),
            "no DISPLAY, so no GAMESCOPE_* root property; zwlr_layer_shell_v1 not advertised"
        );
    }

    #[test]
    fn reason_says_when_the_x_display_could_not_be_opened() {
        // $DISPLAY is set but nothing is listening on it. Report that, not an
        // absence of GAMESCOPE_* properties that was never observed.
        let d = Detection {
            kind: BackendKind::PlainWindow,
            display: Some(":0".to_string()),
            x_connected: false,
            gamescope_root: false,
            wayland_connected: true,
            layer_shell: false,
        };
        assert_eq!(
            d.reason(),
            "DISPLAY :0 could not be opened; zwlr_layer_shell_v1 not advertised"
        );
    }

    #[test]
    fn reason_says_when_there_is_no_wayland_display() {
        // An X session with no Wayland socket at all: the global list was
        // never read, so layer-shell cannot be reported as merely absent.
        let d = Detection {
            kind: BackendKind::GamescopeX11,
            display: Some(":0".to_string()),
            x_connected: true,
            gamescope_root: true,
            wayland_connected: false,
            layer_shell: false,
        };
        assert_eq!(
            d.reason(),
            "DISPLAY :0 has a GAMESCOPE_* root property; no Wayland display"
        );
    }
}
