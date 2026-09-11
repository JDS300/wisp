// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/layer_shell.rs
//! A zwlr_layer_shell_v1 surface on the overlay layer, for desktop Wayland
//! sessions not running under gamescope: KDE, Sway, Hyprland, river.

use crate::backend::{BackendError, Frame, OverlayBackend, Rect};

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
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, EventQueue, QueueHandle,
};

/// Dispatch target for the live connection. Holds only what event handling
/// needs; the pool and the layer surface handle live directly on
/// `LayerShellBackend` since `present` does not dispatch events to reach them.
struct AppState {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    /// Set by every `configure`, to the size it carried. `attach` waits for
    /// one where both dimensions are non-zero.
    new_size: Option<(u32, u32)>,
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
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // Stored rather than acted on here: `attach` is the one place that
        // decides whether a given size is usable (non-zero) and when to stop
        // waiting.
        self.new_size = Some(configure.new_size);
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
    /// The `wl_output` name (e.g. `DP-1`) to place the surface on, or `None`
    /// to let the compositor choose. Set once at construction; `attach`
    /// resolves it against the live output list.
    output: Option<String>,
    width: u32,
    height: u32,
    conn: Option<Connection>,
    event_queue: Option<EventQueue<AppState>>,
    state: Option<AppState>,
    pool: Option<SlotPool>,
    layer: Option<LayerSurface>,
}

impl LayerShellBackend {
    pub fn new(output: Option<&str>) -> Self {
        LayerShellBackend {
            output: output.map(str::to_string),
            width: 0,
            height: 0,
            conn: None,
            event_queue: None,
            state: None,
            pool: None,
            layer: None,
        }
    }
}

impl OverlayBackend for LayerShellBackend {
    fn attach(&mut self) -> Result<(u32, u32), BackendError> {
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

        let mut state = AppState {
            registry_state: RegistryState::new(&globals),
            output_state: OutputState::new(&globals, &qh),
            shm,
            new_size: None,
            closed: false,
        };

        // One roundtrip so every output already known from
        // `registry_queue_init` has delivered its name (and the rest of its
        // `wl_output` info) before `self.output` is looked up against it.
        event_queue
            .roundtrip(&mut state)
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        let output = self.output.as_deref().and_then(|name| {
            let found = state.output_state.outputs().find(|o| {
                state.output_state.info(o).and_then(|info| info.name).as_deref() == Some(name)
            });
            if found.is_none() {
                let list: Vec<String> = state
                    .output_state
                    .outputs()
                    .filter_map(|o| state.output_state.info(&o).and_then(|info| info.name))
                    .collect();
                eprintln!("wisp-hud: output {name} not found; outputs: {}", list.join(", "));
            }
            found
        });

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
            output.as_ref(),
        );
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(0);
        // Anchored to all four edges with a zero requested size: this asks
        // the compositor for the whole output, the same "screen-sized
        // surface" contract every other backend gives.
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_margin(0, 0, 0, 0);
        layer.set_size(0, 0);
        layer.commit();

        // Bounded, not an unconditional loop: a compositor that never sends
        // a configure (misbehaving, or a layer-shell version that silently
        // rejects the surface) must not hang attach() forever.
        let configure_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let (width, height) = loop {
            if let Some((w, h)) = state.new_size {
                if w != 0 && h != 0 {
                    break (w, h);
                }
            }
            if state.closed {
                return Err(BackendError::Failed(
                    "layer surface was closed before it was configured".to_string(),
                ));
            }
            if std::time::Instant::now() >= configure_deadline {
                return Err(BackendError::Failed(
                    "timed out waiting for the compositor to configure the layer surface"
                        .to_string(),
                ));
            }
            event_queue
                .blocking_dispatch(&mut state)
                .map_err(|e| BackendError::Failed(e.to_string()))?;
        };

        let pool = SlotPool::new((width * height * 4).max(1) as usize, &state.shm)
            .map_err(|e| BackendError::Failed(e.to_string()))?;

        self.width = width;
        self.height = height;
        self.conn = Some(conn);
        self.event_queue = Some(event_queue);
        self.state = Some(state);
        self.pool = Some(pool);
        self.layer = Some(layer);
        Ok((width, height))
    }

    fn present(&mut self, frame: &Frame, dirty: &[Rect]) -> Result<(), BackendError> {
        if dirty.is_empty() {
            return Ok(());
        }
        let (Some(event_queue), Some(state), Some(pool), Some(layer)) = (
            self.event_queue.as_mut(),
            self.state.as_mut(),
            self.pool.as_mut(),
            self.layer.as_ref(),
        ) else {
            return Ok(());
        };

        let width = self.width;
        let height = self.height;
        let stride = width as i32 * 4;
        let Ok((buffer, canvas)) =
            pool.create_buffer(width as i32, height as i32, stride, wl_shm::Format::Argb8888)
        else {
            return Ok(());
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
        // One damage_buffer per dirty rect, clipped to the buffer, instead of
        // one for the whole surface: the compositor is told exactly which
        // pixels changed rather than repainting everything downstream.
        let bounds = Rect::new(0, 0, width, height);
        for &rect in dirty {
            if let Some(clipped) = rect.intersect(bounds) {
                wl_surface.damage_buffer(clipped.x, clipped.y, clipped.w as i32, clipped.h as i32);
            }
        }
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
        //
        // The roundtrip also dispatches `closed`, so a compositor that tore
        // down the layer surface between frames is caught here rather than
        // presenting silently into a dead surface forever.
        if let Err(e) = event_queue.roundtrip(state) {
            return Err(BackendError::Failed(format!("roundtrip failed: {e}")));
        }
        if state.closed {
            return Err(BackendError::Failed(
                "compositor closed the layer surface".to_string(),
            ));
        }
        Ok(())
    }
}
