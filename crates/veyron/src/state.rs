//! The VEYRON compositor state and protocol handlers.
//!
//! This module is the only place where Wayland/Smithay meets the Veyron core.
//! It translates protocol events into core inputs (`NewWindow`, `Action`) and
//! turns the resulting [`veyron_core::Snapshot`] into configures, focus and
//! rendering.

use std::{collections::HashMap, error::Error, path::PathBuf, time::Instant};

use smithay::{
    backend::{
        input::{
            AbsolutePositionEvent, Axis, ButtonState, InputEvent, KeyState, KeyboardKeyEvent,
            PointerAxisEvent, PointerButtonEvent,
        },
        renderer::{
            gles::GlesRenderer,
            utils::{draw_render_elements, on_commit_buffer_handler},
            Color32F, Frame, Renderer,
        },
        winit::{WinitEvent, WinitGraphicsBackend, WinitInput},
    },
    desktop::{PopupKind, PopupManager, Space, Window},
    input::{
        keyboard::{xkb, FilterResult, KeyboardHandle, KeysymHandle},
        pointer::{AxisFrame, ButtonEvent, CursorImageStatus, MotionEvent, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason},
        protocol::{wl_buffer, wl_output, wl_seat, wl_shm, wl_surface::WlSurface},
        Client, DisplayHandle,
    },
    utils::{Logical, Point, Rectangle, Serial, Transform, SERIAL_COUNTER},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            with_surface_tree_downward, CompositorClientState, CompositorHandler, CompositorState,
            SurfaceAttributes, TraversalAction,
        },
        output::OutputManagerState,
        selection::{
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
            SelectionHandler,
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
    },
};
use veyron_core::{Action, Config, Effects, Modifiers, NewWindow, Size as VSize, WindowId, Wm};
use veyron_ipc::{Response, StateReport, WindowReport};

use crate::ipc::Job;

/// Pixel offset applied per wheel notch when the backend reports discrete steps.
const AXIS_STEP_PIXELS: f64 = 15.0;

/// Per-client protocol state (required by `smithay`'s compositor handler).
#[derive(Debug, Default)]
pub struct ClientState {
    compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

/// The compositor.
pub struct Veyron {
    start: Instant,
    config_path: PathBuf,

    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    shm_state: ShmState,
    seat_state: SeatState<Self>,
    data_device_state: DataDeviceState,
    /// Kept alive so the `wl_output`/`zxdg_output` globals stay registered.
    _output_manager_state: OutputManagerState,
    popup_manager: PopupManager,

    /// Kept alive so the seat and its globals stay registered.
    _seat: Seat<Self>,
    keyboard: KeyboardHandle<Self>,
    pointer: PointerHandle<Self>,

    output: Output,
    space: Space<Window>,
    /// `None` in headless mode: the compositor then runs the full protocol
    /// stack and window model without drawing anything.
    backend: Option<WinitGraphicsBackend<GlesRenderer>>,

    wm: Wm,
    windows: HashMap<WindowId, Window>,
    /// Last location a window was mapped at, so the scene graph is only touched
    /// when the layout really moved it (avoiding pointless z-order churn).
    mapped: HashMap<WindowId, (i32, i32)>,
    /// Last size sent to each client, so configures are not flooded.
    configured: HashMap<WindowId, (i32, i32)>,

    pointer_location: Point<f64, Logical>,
    needs_redraw: bool,
    /// Set when the compositor should exit.
    pub exit: bool,
}

impl Veyron {
    /// Build the compositor around an already initialized nested backend.
    pub fn new(
        config_path: PathBuf,
        config: Config,
        backend: WinitGraphicsBackend<GlesRenderer>,
        display_handle: &DisplayHandle,
    ) -> Result<Self, Box<dyn Error>> {
        let window_size = backend.window_size();
        let size = VSize::new(window_size.w, window_size.h);
        Self::build(config_path, config, size, Some(backend), display_handle)
    }

    /// Build a headless compositor: the real `Display`, socket, protocol
    /// handlers and window model, with rendering disabled.
    ///
    /// This path needs no display server, GPU or DRM device, which is what makes
    /// the protocol layer testable in CI and on machines without graphics.
    pub fn new_headless(
        config_path: PathBuf,
        config: Config,
        size: VSize,
        display_handle: &DisplayHandle,
    ) -> Result<Self, Box<dyn Error>> {
        Self::build(config_path, config, size, None, display_handle)
    }

    fn build(
        config_path: PathBuf,
        config: Config,
        size: VSize,
        backend: Option<WinitGraphicsBackend<GlesRenderer>>,
        display_handle: &DisplayHandle,
    ) -> Result<Self, Box<dyn Error>> {
        let dh = display_handle.clone();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(
            &dh,
            vec![wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888],
        );

        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "veyron");
        let keyboard = seat.add_keyboard(Default::default(), 200, 25)?;
        let pointer = seat.add_pointer();

        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let popup_manager = PopupManager::default();

        let output = Output::new(
            "VEYRON-1".to_string(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Veyron".to_string(),
                model: "Nested".to_string(),
            },
        );
        let mode = Mode {
            size: (size.w, size.h).into(),
            refresh: 60_000,
        };
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            None,
            Some((0, 0).into()),
        );
        output.set_preferred(mode);

        let mut space = Space::default();
        space.map_output(&output, (0, 0));

        let startup = config.startup.clone();
        let wm = Wm::new(config, "VEYRON-1", size);

        let state = Self {
            start: Instant::now(),
            config_path,
            compositor_state,
            xdg_shell_state,
            shm_state,
            seat_state,
            data_device_state,
            _output_manager_state: output_manager_state,
            popup_manager,
            _seat: seat,
            keyboard,
            pointer,
            output,
            space,
            backend,
            wm,
            windows: HashMap::new(),
            mapped: HashMap::new(),
            configured: HashMap::new(),
            pointer_location: (0.0, 0.0).into(),
            needs_redraw: true,
            exit: false,
        };

        state.output.create_global::<Self>(&dh);

        for command in startup {
            spawn(&command);
        }

        Ok(state)
    }

    /// Redraw if anything changed and push frame callbacks. Called once per
    /// event-loop iteration.
    pub fn tick(&mut self) -> Result<(), Box<dyn Error>> {
        if self.needs_redraw {
            self.needs_redraw = false;
            self.sync_layout();
            self.render()?;
            self.send_frame_callbacks();
        }
        Ok(())
    }

    /// Handle an event from the nested winit backend.
    pub fn on_winit_event(&mut self, event: WinitEvent) {
        match event {
            WinitEvent::Resized { size, .. } => {
                self.wm.set_output_size(VSize::new(size.w, size.h));
                self.update_output();
                self.needs_redraw = true;
            }
            WinitEvent::CloseRequested => self.exit = true,
            WinitEvent::Redraw => self.needs_redraw = true,
            WinitEvent::Input(event) => match event {
                InputEvent::Keyboard { event } => self.on_keyboard(event),
                InputEvent::PointerMotionAbsolute { event } => self.on_pointer_motion(&event),
                InputEvent::PointerButton { event } => self.on_pointer_button(&event),
                InputEvent::PointerAxis { event } => self.on_pointer_axis(&event),
                _ => {}
            },
            _ => {}
        }
    }

    fn on_keyboard<E>(&mut self, event: E)
    where
        E: KeyboardKeyEvent<WinitInput>,
    {
        let serial = SERIAL_COUNTER.next_serial();
        let time = event.time_msec();
        let key_state = event.state();
        let keyboard = self.keyboard.clone();

        let action = keyboard.input::<Action, _>(
            self,
            event.key_code(),
            key_state,
            serial,
            time,
            |data, modifiers, keysym| {
                // Only presses trigger bindings; releases are always forwarded
                // so clients see matching key-up events.
                if key_state != KeyState::Pressed {
                    return FilterResult::Forward;
                }
                let name = keysym_name(&keysym);
                let modifiers = Modifiers {
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                    shift: modifiers.shift,
                    logo: modifiers.logo,
                };
                match data.wm.config().lookup(modifiers, &name) {
                    Some(action) => FilterResult::Intercept(action.clone()),
                    None => FilterResult::Forward,
                }
            },
        );

        if let Some(action) = action {
            self.apply_action(action);
        }
    }

    fn on_pointer_motion<E>(&mut self, event: &E)
    where
        E: AbsolutePositionEvent<WinitInput>,
    {
        let surface_position = event.position();
        let scale = self.scale_factor().max(f64::EPSILON);
        let location: Point<f64, Logical> =
            Point::from((surface_position.x / scale, surface_position.y / scale));
        self.pointer_location = location;

        let target = self.surface_under(location);
        let serial = SERIAL_COUNTER.next_serial();
        // The handle is cloned so the immutable borrow of the field does not
        // collide with the `&mut self` the protocol callbacks require.
        let pointer = self.pointer.clone();
        pointer.motion(
            self,
            target,
            &MotionEvent {
                location,
                serial,
                time: event.time_msec(),
            },
        );
        pointer.frame(self);
    }

    fn on_pointer_button<E>(&mut self, event: &E)
    where
        E: PointerButtonEvent<WinitInput>,
    {
        if event.state() == ButtonState::Pressed {
            let window = self
                .space
                .element_under(self.pointer_location)
                .map(|(window, _)| window.clone());
            if let Some(window) = window {
                if let Some(id) = self.window_id(&window) {
                    if self.wm.focus_window(id) {
                        self.sync_focus();
                    }
                    if self.wm.snapshot().is_floating(id) {
                        self.space.raise_element(&window, false);
                    }
                    self.needs_redraw = true;
                }
            }
        }

        let serial = SERIAL_COUNTER.next_serial();
        let pointer = self.pointer.clone();
        pointer.button(
            self,
            &ButtonEvent {
                serial,
                time: event.time_msec(),
                button: event.button_code(),
                state: event.state(),
            },
        );
        pointer.frame(self);
    }

    fn on_pointer_axis<E>(&mut self, event: &E)
    where
        E: PointerAxisEvent<WinitInput>,
    {
        let mut frame = AxisFrame::new(event.time_msec()).source(event.source());
        for axis in [Axis::Horizontal, Axis::Vertical] {
            if let Some(steps) = event.amount_v120(axis) {
                frame = frame
                    .v120(axis, (steps / 120.0).round() as i32)
                    .value(axis, steps / 120.0 * AXIS_STEP_PIXELS);
            } else if let Some(amount) = event.amount(axis) {
                frame = frame.value(axis, amount);
            }
        }
        let pointer = self.pointer.clone();
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    /// Dispatch an action and carry out its effects.
    pub fn apply_action(&mut self, action: Action) {
        let effects = self.wm.dispatch(action);
        self.apply_effects(effects);
    }

    fn apply_effects(&mut self, effects: Effects) {
        if let Some(id) = effects.close_requested {
            if let Some(toplevel) = self.windows.get(&id).and_then(Window::toplevel) {
                toplevel.send_close();
            }
        }

        for command in &effects.spawn {
            spawn(command);
        }

        if effects.reload_config {
            self.reload_config();
        }

        if effects.layout_changed || effects.workspace_changed || effects.focus_changed {
            self.sync_layout();
            self.sync_focus();
        }

        if effects.needs_redraw() {
            self.needs_redraw = true;
        }

        if effects.quit {
            self.exit = true;
        }
    }

    /// Handle one control-protocol request on the compositor thread.
    pub fn handle_ipc(&mut self, job: Job) {
        let response = match job.request {
            veyron_ipc::Request::Ping => Response::Pong,
            veyron_ipc::Request::State => Response::State(self.state_report()),
            veyron_ipc::Request::Reload => {
                self.apply_action(Action::ReloadConfig);
                Response::Ok
            }
            veyron_ipc::Request::Quit => {
                self.apply_action(Action::Quit);
                Response::Ok
            }
            veyron_ipc::Request::Action { action } => match Action::parse(&action) {
                Ok(action) => {
                    self.apply_action(action);
                    Response::Ok
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            },
        };
        let _ = job.reply.send(response);
    }

    fn state_report(&self) -> StateReport {
        let snapshot = self.wm.snapshot();
        StateReport {
            workspace: snapshot.workspace.clone(),
            workspace_index: snapshot.workspace_index,
            workspace_count: snapshot.workspace_count,
            windows: snapshot
                .placements
                .iter()
                .map(|placement| WindowReport {
                    id: placement.window.0,
                    rect: [
                        placement.rect.x,
                        placement.rect.y,
                        placement.rect.w,
                        placement.rect.h,
                    ],
                    floating: placement.floating,
                    fullscreen: placement.fullscreen,
                })
                .collect(),
            focused: snapshot.focused.map(|id| id.0),
            scroll: snapshot.scroll,
        }
    }

    fn reload_config(&mut self) {
        match Config::load_or_default(&self.config_path) {
            Ok(config) => {
                tracing::info!(path = %self.config_path.display(), "configuration reloaded");
                self.wm.set_config(config);
            }
            Err(error) => {
                tracing::error!(%error, path = %self.config_path.display(), "could not reload configuration");
            }
        }
        self.needs_redraw = true;
    }

    /// Scale factor of the nested window; headless outputs are unscaled.
    fn scale_factor(&self) -> f64 {
        self.backend
            .as_ref()
            .map_or(1.0, |backend| backend.scale_factor())
    }

    fn update_output(&mut self) {
        let Some(window_size) = self.backend.as_ref().map(|backend| backend.window_size()) else {
            // Headless outputs keep the mode they were created with.
            return;
        };
        let mode = Mode {
            size: window_size,
            refresh: 60_000,
        };
        self.output
            .change_current_state(Some(mode), None, None, None);
        self.output.set_preferred(mode);
        self.space.map_output(&self.output, (0, 0));
    }

    /// Match the scene graph to the layout snapshot and configure clients.
    fn sync_layout(&mut self) {
        let snapshot = self.wm.snapshot();
        let mut visible = Vec::with_capacity(snapshot.placements.len());

        for placement in &snapshot.placements {
            let Some(window) = self.windows.get(&placement.window) else {
                continue;
            };
            let location = (placement.rect.x, placement.rect.y);
            if self.mapped.get(&placement.window) != Some(&location) {
                self.space.map_element(window.clone(), location, false);
                self.mapped.insert(placement.window, location);
            }
            visible.push(placement.window);

            // Floating windows keep the size their client chose.
            if placement.floating {
                continue;
            }

            let target = (placement.rect.w, placement.rect.h);
            if self.configured.get(&placement.window) == Some(&target) {
                continue;
            }
            if let Some(toplevel) = window.toplevel() {
                // Only the size is set here: activation is applied uniformly
                // by `Window::set_activated` in `sync_focus`, so a configure
                // can never mark every tiled window as active.
                toplevel.with_pending_state(|state| {
                    state.size = Some((target.0, target.1).into());
                });
                toplevel.send_configure();
                self.configured.insert(placement.window, target);
            }
        }

        let stale: Vec<WindowId> = self
            .mapped
            .keys()
            .filter(|id| !visible.contains(id))
            .copied()
            .collect();
        for id in stale {
            if let Some(window) = self.windows.get(&id) {
                self.space.unmap_elem(window);
            }
            self.mapped.remove(&id);
        }
    }

    /// Move keyboard focus to the focused window of the active workspace.
    fn sync_focus(&mut self) {
        let snapshot = self.wm.snapshot();
        let focused_surface = snapshot
            .focused
            .and_then(|id| self.windows.get(&id))
            .and_then(Window::toplevel)
            .map(|toplevel| toplevel.wl_surface().clone());

        let serial = SERIAL_COUNTER.next_serial();
        let keyboard = self.keyboard.clone();
        keyboard.set_focus(self, focused_surface, serial);

        for (id, window) in &self.windows {
            // `set_activated` only stages the xdg state; the configure that
            // tells the client about it has to be sent explicitly, otherwise
            // clients never observe activation changes.
            if window.set_activated(snapshot.focused == Some(*id)) {
                if let Some(toplevel) = window.toplevel() {
                    toplevel.send_pending_configure();
                }
            }
        }

        if let Some(id) = snapshot.focused {
            if snapshot.is_floating(id) {
                if let Some(window) = self.windows.get(&id) {
                    self.space.raise_element(window, false);
                }
            }
        }

        self.needs_redraw = true;
    }

    fn surface_under(
        &self,
        location: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let (window, _) = self.space.element_under(location)?;
        let origin = self.space.element_location(window)?;
        let toplevel = window.toplevel()?;
        let surface_location =
            Point::from((location.x - origin.x as f64, location.y - origin.y as f64));
        Some((toplevel.wl_surface().clone(), surface_location))
    }

    fn window_id(&self, window: &Window) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, candidate)| *candidate == window)
            .map(|(id, _)| *id)
    }

    fn render(&mut self) -> Result<(), Box<dyn Error>> {
        // Headless mode has no renderer; protocol state and layout still run.
        let Some(backend) = self.backend.as_mut() else {
            return Ok(());
        };
        let size = backend.window_size();
        let damage = Rectangle::from_size(size);

        // Scope the frame: `finish` consumes it and the framebuffer borrow must
        // end before the buffers are submitted.
        {
            let (renderer, mut framebuffer) = backend.bind()?;

            let elements = self
                .space
                .render_elements_for_output(renderer, &self.output, 1.0)?;

            let background = self.wm.config().appearance.background_rgba();
            let mut frame = renderer.render(&mut framebuffer, size, Transform::Flipped180)?;
            frame.clear(
                Color32F::new(background[0], background[1], background[2], background[3]),
                &[damage],
            )?;
            draw_render_elements(&mut frame, 1.0, &elements, &[damage])?;
            let _ = frame.finish()?;
        }

        backend.submit(Some(&[damage]))?;
        Ok(())
    }

    fn send_frame_callbacks(&self) {
        let time = self.start.elapsed().as_millis() as u32;
        for window in self.windows.values() {
            if let Some(toplevel) = window.toplevel() {
                send_frames_surface_tree(toplevel.wl_surface(), time);
            }
        }
    }
}

fn keysym_name(handle: &KeysymHandle<'_>) -> String {
    xkb::keysym_get_name(handle.modified_sym()).to_ascii_lowercase()
}

fn spawn(command: &str) {
    match std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .spawn()
    {
        Ok(_) => tracing::info!(command, "spawned command"),
        Err(error) => tracing::error!(%error, command, "could not spawn command"),
    }
}

/// Deliver `wl_surface.frame` callbacks for a surface tree.
fn send_frames_surface_tree(surface: &WlSurface, time: u32) {
    with_surface_tree_downward(
        surface,
        (),
        |_, _, &()| TraversalAction::DoChildren(()),
        |_surface, states, &()| {
            for callback in states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .frame_callbacks
                .drain(..)
            {
                callback.done(time);
            }
        },
        |_, _, &()| true,
    );
}

impl CompositorHandler for Veyron {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        let state = client
            .get_data::<ClientState>()
            .expect("client was created by Veyron");
        &state.compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.popup_manager.commit(surface);

        for window in self.windows.values() {
            if window
                .toplevel()
                .map(|toplevel| toplevel.wl_surface() == surface)
                .unwrap_or(false)
            {
                window.on_commit();
            }
        }

        self.needs_redraw = true;
    }
}

impl smithay::wayland::output::OutputHandler for Veyron {
    fn output_bound(&mut self, _output: Output, _wl_output: wl_output::WlOutput) {}
}

impl BufferHandler for Veyron {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for Veyron {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl SelectionHandler for Veyron {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Veyron {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for Veyron {}

impl ServerDndGrabHandler for Veyron {
    fn send(&mut self, _mime_type: String, _fd: std::os::unix::io::OwnedFd, _seat: Seat<Self>) {}
}

impl SeatHandler for Veyron {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}
}

impl XdgShellHandler for Veyron {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        let id = self.wm.create_window(NewWindow::default());
        self.windows.insert(id, window);
        self.sync_layout();
        self.sync_focus();
        self.needs_redraw = true;
        tracing::debug!(window = id.0, "mapped a new toplevel");
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        if self
            .popup_manager
            .track_popup(PopupKind::Xdg(surface))
            .is_err()
        {
            tracing::warn!("could not track a popup surface");
        }
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Interactive popup grabs are not implemented in this phase. Popups are
        // still mapped and rendered; the client closes them itself.
    }

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
        tracing::debug!("popup reposition request ignored");
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let id = self
            .windows
            .iter()
            .find(|(_, window)| window.toplevel() == Some(&surface))
            .map(|(id, _)| *id);

        if let Some(id) = id {
            if let Some(window) = self.windows.remove(&id) {
                self.space.unmap_elem(&window);
            }
            self.mapped.remove(&id);
            self.configured.remove(&id);
            self.wm.remove_window(id);
            self.sync_layout();
            self.sync_focus();
            self.needs_redraw = true;
            tracing::debug!(window = id.0, "window destroyed");
        }
    }

    fn popup_destroyed(&mut self, _surface: PopupSurface) {
        self.popup_manager.cleanup();
        self.needs_redraw = true;
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<wl_output::WlOutput>,
    ) {
        // Fullscreen is compositor policy in Veyron: acknowledge the request
        // without changing the layout.
        surface.send_configure();
    }

    fn unfullscreen_request(&mut self, _surface: ToplevelSurface) {}
}

smithay::delegate_compositor!(Veyron);
smithay::delegate_xdg_shell!(Veyron);
smithay::delegate_shm!(Veyron);
smithay::delegate_seat!(Veyron);
smithay::delegate_data_device!(Veyron);
smithay::delegate_output!(Veyron);
