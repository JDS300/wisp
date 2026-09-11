// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/backend/layer_shell.rs
//! A zwlr_layer_shell_v1 surface on the overlay layer, for desktop Wayland
//! sessions not running under gamescope: KDE, Sway, Hyprland, river.

use crate::backend::{BackendError, Frame, KeyEvent, OverlayBackend, Rect};
use crate::evdev::ChordTracker;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_dispatch2, delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::collections::VecDeque;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface},
    Connection, EventQueue, QueueHandle, WEnum,
};

/// Dispatch target for the live connection. Holds only what event handling
/// needs; the pool and the layer surface handle live directly on
/// `LayerShellBackend` since `present` does not dispatch events to reach them.
struct AppState {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    /// Set by every `configure`, to the size it carried. `attach` waits for
    /// one where both dimensions are non-zero.
    new_size: Option<(u32, u32)>,
    closed: bool,
    /// The seat's keyboard, once `new_capability` has seen one. `None` until
    /// then, and again once `remove_capability` takes it away.
    keyboard: Option<wl_keyboard::WlKeyboard>,
    /// Key events collected from `wl_keyboard`, in order, waiting for
    /// `drain_keys` to hand them to the caller.
    keys: VecDeque<KeyEvent>,
    /// The evdev-to-`Key` state machine: modifiers, the chord, and what is
    /// currently reported as held.
    chord: ChordTracker,
    /// Whether `wl_keyboard.enter` has arrived since the last `leave` (or
    /// since the keyboard was bound). `take_keyboard`'s handshake reads this.
    entered: bool,
}

/// User data for the HUD's `wl_keyboard`. A type of its own rather than
/// `()`, because `delegate_dispatch2!(AppState)` is a blanket
/// `Dispatch<I, U> for AppState where U: Dispatch2<I, AppState>`, and a
/// hand-written `Dispatch<WlKeyboard, ()>` would conflict with it.
struct KeyboardData;

impl Dispatch2<wl_keyboard::WlKeyboard, AppState> for KeyboardData {
    fn event(
        &self,
        state: &mut AppState,
        _keyboard: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _conn: &Connection,
        _qh: &QueueHandle<AppState>,
    ) {
        match event {
            wl_keyboard::Event::Enter { keys, .. } => {
                state.entered = true;
                let codes = crate::evdev::codes_from_enter(&keys);
                state.keys.extend(state.chord.sync_from_enter(&codes));
            }
            wl_keyboard::Event::Leave { .. } => {
                state.entered = false;
                // No more releases are coming, so everything held is
                // released here or the caller's set never empties.
                let released = state.chord.release_all();
                state.keys.extend(released);
            }
            wl_keyboard::Event::Key { key, state: key_state, .. } => {
                // `Repeated` (wl_seat v10) is a press; SCTK binds at most
                // version 7 here, so it should never arrive, and treating it
                // as a press is the harmless answer if it ever does — the
                // key is already in the caller's held set and `Edges` owns
                // the repeat cadence.
                let pressed = matches!(
                    key_state,
                    WEnum::Value(wl_keyboard::KeyState::Pressed)
                        | WEnum::Value(wl_keyboard::KeyState::Repeated)
                );
                if let Some(event) = state.chord.feed(key, pressed) {
                    state.keys.push_back(event);
                }
            }
            // Keymap, Modifiers and RepeatInfo are all deliberately ignored:
            // the mapping is by position (§4.5) and the repeat cadence is
            // `keys::Edges`', so nothing here needs xkb.
            _ => {}
        }
    }
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

impl SeatHandler for AppState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        // `SeatState::get_keyboard` is behind sctk's `xkbcommon` feature,
        // which binds the C library; `wl_seat.get_keyboard` from
        // wayland-client is the same request without it.
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(seat.get_keyboard(qh, KeyboardData));
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
            self.entered = false;
            let released = self.chord.release_all();
            self.keys.extend(released);
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}
}

impl ProvidesRegistryState for AppState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
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
    /// Once a compositor has failed to give the HUD the keyboard, it is not
    /// asked again for the rest of the run.
    latch: KeyboardLatch,
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
            latch: KeyboardLatch::default(),
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
            seat_state: SeatState::new(&globals, &qh),
            shm,
            new_size: None,
            closed: false,
            keyboard: None,
            keys: VecDeque::new(),
            chord: ChordTracker::new(),
            entered: false,
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

    fn take_keyboard(&mut self, exclusive: bool) -> bool {
        let (Some(layer), Some(queue), Some(state)) =
            (self.layer.as_ref(), self.event_queue.as_mut(), self.state.as_mut())
        else {
            return false;
        };

        if !exclusive {
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.commit();
            let _ = queue.roundtrip(state);
            // The compositor moves focus back to whatever had it; from here on
            // no release events arrive, so everything held is released now.
            let released = state.chord.release_all();
            state.keys.extend(released);
            state.entered = false;
            return false;
        }

        if !self.latch.may_ask() {
            return false;
        }

        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.commit();

        // Bounded: §4.5's 500 ms. A compositor that ignores the switch must not
        // hang the frame the chord was pressed on.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            let _ = queue.roundtrip(state);
            match handshake(state.entered, std::time::Instant::now() >= deadline) {
                Handshake::Granted => return true,
                Handshake::Denied => break,
                // A roundtrip returns as soon as the compositor answers the sync,
                // which is at once; without this the loop would spin for 500 ms.
                Handshake::KeepWaiting => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }

        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        let _ = queue.roundtrip(state);
        eprintln!(
            "wisp-hud: the compositor did not give the HUD the keyboard; HUD-mode keys will also reach the game"
        );
        self.latch.deny();
        false
    }

    fn drain_keys(&mut self) -> Vec<KeyEvent> {
        let (Some(queue), Some(state)) = (self.event_queue.as_mut(), self.state.as_mut()) else {
            return Vec::new();
        };
        // The render loop only redraws when something changed, so `present`'s
        // own roundtrip cannot be relied on to read the socket: this is the read
        // that delivers key events. It is called only inside HUD mode (main.rs),
        // so outside it the connection is as quiet as it was in v0.2.0.
        let _ = queue.roundtrip(state);
        state.keys.drain(..).collect()
    }
}

/// The exclusive-keyboard handshake as a decision, so the 500 ms rule is a
/// unit test and not a compositor.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Handshake {
    Granted,
    KeepWaiting,
    Denied,
}

pub(crate) fn handshake(entered: bool, deadline_passed: bool) -> Handshake {
    if entered {
        Handshake::Granted
    } else if deadline_passed {
        Handshake::Denied
    } else {
        Handshake::KeepWaiting
    }
}

/// One latch: once a compositor has failed to give the HUD the keyboard, it
/// is not asked again for the rest of the run.
#[derive(Debug, Default)]
pub(crate) struct KeyboardLatch {
    denied: bool,
}

impl KeyboardLatch {
    pub(crate) fn may_ask(&self) -> bool {
        !self.denied
    }

    pub(crate) fn deny(&mut self) {
        self.denied = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handshake_waits_then_gives_up() {
        assert_eq!(handshake(true, false), Handshake::Granted);
        assert_eq!(handshake(true, true), Handshake::Granted, "an enter that arrived at the last moment still counts");
        assert_eq!(handshake(false, false), Handshake::KeepWaiting);
        assert_eq!(handshake(false, true), Handshake::Denied);
    }

    #[test]
    fn a_compositor_that_refused_once_is_not_asked_again() {
        let mut latch = KeyboardLatch::default();
        assert!(latch.may_ask());
        latch.deny();
        assert!(!latch.may_ask());
        latch.deny();
        assert!(!latch.may_ask(), "and it stays denied for the rest of the run");
    }
}
