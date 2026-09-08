// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/layer_shell.rs
//! A zwlr_layer_shell_v1 surface on the overlay layer, for desktop Wayland
//! sessions not running under gamescope: KDE, Sway, Hyprland, river.

use crate::backend::{BackendError, Frame, OverlayBackend};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_output, wl_registry, wl_shm, wl_surface},
    Connection, Dispatch, EventQueue, QueueHandle,
};

/// Interface names the compositor advertises. Empty if there is no Wayland
/// display, which is itself a valid answer for selection purposes.
pub fn wayland_globals() -> Vec<String> {
    // A minimal Dispatch target that only needs the registry's global list;
    // `registry_queue_init` already does the one round-trip we need.
    struct GlobalsOnly;

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for GlobalsOnly {
        fn event(
            _state: &mut Self,
            _proxy: &wl_registry::WlRegistry,
            _event: wl_registry::Event,
            _data: &GlobalListContents,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
            // Selection only needs the initial snapshot below.
        }
    }

    let Ok(conn) = Connection::connect_to_env() else {
        return Vec::new();
    };
    let Ok((globals, _queue)) = registry_queue_init::<GlobalsOnly>(&conn) else {
        return Vec::new();
    };
    globals
        .contents()
        .with_list(|list| list.iter().map(|g| g.interface.clone()).collect())
}

/// Dispatch target for the live connection. Holds only what event handling
/// needs; the pool and the layer surface handle live directly on
/// `LayerShellBackend` since `present` does not dispatch events to reach them.
struct AppState {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    configured: bool,
    closed: bool,
}

impl CompositorHandler for AppState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        // The daemon drives the presentation cadence; frame callbacks are
        // not requested and not needed.
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for AppState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: wl_output::WlOutput) {}

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for AppState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.closed = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // The size we asked for is the size we keep; a compositor-suggested
        // size is only ever a suggestion for this fixed-size HUD.
        self.configured = true;
    }
}

impl ShmHandler for AppState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for AppState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_dispatch2!(AppState);
delegate_registry!(AppState);

pub struct LayerShellBackend {
    width: u32,
    height: u32,
    conn: Option<Connection>,
    event_queue: Option<EventQueue<AppState>>,
    state: Option<AppState>,
    pool: Option<SlotPool>,
    layer: Option<LayerSurface>,
}

impl LayerShellBackend {
    pub fn new(width: u32, height: u32) -> Self {
        LayerShellBackend {
            width,
            height,
            conn: None,
            event_queue: None,
            state: None,
            pool: None,
            layer: None,
        }
    }
}

impl OverlayBackend for LayerShellBackend {
    fn attach(&mut self) -> Result<(), BackendError> {
        let conn =
            Connection::connect_to_env().map_err(|e| BackendError::Unavailable(e.to_string()))?;

        let (globals, mut event_queue) = registry_queue_init::<AppState>(&conn)
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        let qh = event_queue.handle();

        let compositor = CompositorState::bind(&globals, &qh)
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        let layer_shell = LayerShell::bind(&globals, &qh)
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        let shm =
            Shm::bind(&globals, &qh).map_err(|e| BackendError::Unavailable(e.to_string()))?;

        let surface = compositor.create_surface(&qh);

        // Belt and braces for the invariant, the Wayland equivalent of the
        // XFixes region the gamescope backend sets: an empty region means
        // this surface never receives pointer or keyboard input, whatever
        // KeyboardInteractivity::None turns out to do on its own.
        let region =
            Region::new(&compositor).map_err(|e| BackendError::Failed(e.to_string()))?;
        surface.set_input_region(Some(region.wl_region()));

        let layer = layer_shell.create_layer_surface(
            &qh,
            surface,
            Layer::Overlay,
            Some("wisp-hud"),
            None,
        );
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(0);
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_margin(16, 0, 0, 16);
        layer.set_size(self.width, self.height);
        layer.commit();

        let mut state = AppState {
            registry_state: RegistryState::new(&globals),
            output_state: OutputState::new(&globals, &qh),
            shm,
            configured: false,
            closed: false,
        };

        while !state.configured && !state.closed {
            event_queue
                .blocking_dispatch(&mut state)
                .map_err(|e| BackendError::Failed(e.to_string()))?;
        }
        if state.closed {
            return Err(BackendError::Failed(
                "layer surface was closed before it was configured".to_string(),
            ));
        }

        let pool = SlotPool::new((self.width * self.height * 4).max(1) as usize, &state.shm)
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        self.conn = Some(conn);
        self.event_queue = Some(event_queue);
        self.state = Some(state);
        self.pool = Some(pool);
        self.layer = Some(layer);
        Ok(())
    }

    fn present(&mut self, frame: &Frame) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        let (Some(event_queue), Some(state), Some(pool), Some(layer)) = (
            self.event_queue.as_mut(),
            self.state.as_mut(),
            self.pool.as_mut(),
            self.layer.as_ref(),
        ) else {
            return;
        };

        let width = self.width;
        let height = self.height;
        let stride = width as i32 * 4;
        let Ok((buffer, canvas)) =
            pool.create_buffer(width as i32, height as i32, stride, wl_shm::Format::Argb8888)
        else {
            return;
        };

        // Transparent everywhere, then copy the frame into the top-left,
        // clipped to the buffer -- the same "clip, don't resize" rule as the
        // gamescope backend.
        canvas.fill(0);
        let draw_w = frame.width.min(width);
        let draw_h = frame.height.min(height);
        for y in 0..draw_h {
            for x in 0..draw_w {
                let src = ((y * frame.width + x) * 4) as usize;
                let dst = ((y * width + x) * 4) as usize;
                let r = frame.rgba[src];
                let g = frame.rgba[src + 1];
                let b = frame.rgba[src + 2];
                let a = frame.rgba[src + 3];
                // Already premultiplied; ARGB8888 is little-endian B, G, R, A
                // in memory, same as the gamescope backend's MSB-first path.
                canvas[dst] = b;
                canvas[dst + 1] = g;
                canvas[dst + 2] = r;
                canvas[dst + 3] = a;
            }
        }

        let wl_surface = layer.wl_surface();
        wl_surface.damage_buffer(0, 0, width as i32, height as i32);
        let _ = buffer.attach_to(wl_surface);
        layer.commit();

        // A roundtrip -- not flush() + dispatch_pending() -- is what actually
        // reads the socket. flush() only writes, and dispatch_pending() only
        // replays events already buffered client-side; neither performs the
        // recv() that delivers wl_buffer.release. Without a real read here,
        // released buffers are never returned to the pool and it grows
        // without bound. A roundtrip returns as soon as the compositor
        // answers the sync request, so this does not wait for a frame
        // callback -- the daemon still drives the presentation cadence.
        if let Err(e) = event_queue.roundtrip(state) {
            eprintln!("wisp-hud: layer-shell backend: roundtrip failed: {e}");
        }
    }
}

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
