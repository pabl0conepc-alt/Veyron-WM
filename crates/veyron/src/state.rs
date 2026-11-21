//! The VEYRON compositor state and protocol handlers.
//!
//! This module is the only place where Wayland/Smithay meets the Veyron core.
//! It translates protocol events into core inputs (`NewWindow`, `Action`) and
//! turns the resulting [`veyron_core::Snapshot`] into configures, focus and
//! rendering.

use std::{
    collections::HashMap,
    error::Error,
    path::PathBuf,
    time::{Duration, Instant},
};

use smithay::{
    backend::{
        input::{
            AbsolutePositionEvent, Axis, ButtonState, InputEvent, KeyState, KeyboardKeyEvent,
            PointerAxisEvent, PointerButtonEvent,
        },
        renderer::{
            damage::OutputDamageTracker, gles::GlesRenderer, utils::on_commit_buffer_handler,
        },
        winit::{WinitEvent, WinitGraphicsBackend, WinitInput},
    },
    desktop::{
        layer_map_for_output, LayerSurface as DesktopLayerSurface, PopupKind, PopupManager, Space,
        Window, WindowSurfaceType,
    },
    input::{
        keyboard::{xkb, FilterResult, KeyboardHandle, KeysymHandle},
        pointer::{AxisFrame, ButtonEvent, CursorImageStatus, MotionEvent, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer, wl_output, wl_seat, wl_shm, wl_surface::WlSurface},
            Client, DisplayHandle,
        },
    },
    utils::{Logical, Point, Serial, Transform, SERIAL_COUNTER},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            with_states, with_surface_tree_downward, CompositorClientState, CompositorHandler,
            CompositorState, SurfaceAttributes, TraversalAction,
        },
        output::OutputManagerState,
        selection::{
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
            SelectionHandler,
        },
        shell::{
            wlr_layer::{
                Layer, LayerSurface as WlrLayerSurface, WlrLayerShellHandler, WlrLayerShellState,
            },
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                XdgToplevelSurfaceData,
            },
        },
        shm::{ShmHandler, ShmState},
    },
};
use veyron_core::{
    workspace_slide, Action, Config, Effects, Modifiers, NewWindow, Rect, RectAnimator, Session,
    Size as VSize, WindowId, Wm, WorkspacePosition,
};
use veyron_ipc::{
    OverviewReport, OverviewTileReport, OverviewWindowReport, Response, StateReport, WindowReport,
    MAX_LINE_BYTES,
};

use crate::ipc::Job;

/// Pixel offset applied per wheel notch when the backend reports discrete steps.
const AXIS_STEP_PIXELS: f64 = 15.0;

/// Size of one workspace tile in the `veyronctl overview` grid.
const OVERVIEW_TILE: VSize = VSize::new(320, 200);
/// Space between overview tiles.
const OVERVIEW_GAP: i32 = 16;

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
    /// Serves `zwlr_layer_shell_v1`: panels, notification daemons, wallpapers
    /// and OSDs are ordinary clients of this protocol.
    layer_shell_state: WlrLayerShellState,
    popup_manager: PopupManager,

    /// Kept alive so the seat and its globals stay registered.
    _seat: Seat<Self>,
    keyboard: KeyboardHandle<Self>,
    pointer: PointerHandle<Self>,

    output: Output,
    space: Space<Window>,
    damage_tracker: OutputDamageTracker,
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
    /// Last fullscreen state sent to each client.
    configured_fullscreen: HashMap<WindowId, bool>,
    /// In-flight window rectangles; the scene graph draws `current()`.
    animations: HashMap<WindowId, RectAnimator>,
    last_animation_frame: Instant,
    /// Workspace drawn last, so a switch can slide in from the right side.
    last_workspace_position: WorkspacePosition,

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
        let state = Self::build(config_path, config, size, Some(backend), display_handle)?;
        state.update_workspace_indicator();
        Ok(state)
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
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
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
        let damage_tracker = OutputDamageTracker::new((size.w, size.h), 1.0, Transform::Flipped180);

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
            layer_shell_state,
            popup_manager,
            _seat: seat,
            keyboard,
            pointer,
            output,
            space,
            damage_tracker,
            backend,
            wm,
            windows: HashMap::new(),
            mapped: HashMap::new(),
            configured: HashMap::new(),
            configured_fullscreen: HashMap::new(),
            animations: HashMap::new(),
            last_animation_frame: Instant::now(),
            last_workspace_position: WorkspacePosition::default(),
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
        let now = Instant::now();
        let dt = now.duration_since(self.last_animation_frame).as_secs_f64();
        self.last_animation_frame = now;

        // Draw the frame that finishes a transition too, so the last drawn
        // rectangle is the exact target and not one frame short of it.
        let mut animating = false;
        for animator in self.animations.values_mut() {
            if animator.is_running() {
                animating = true;
                animator.advance(dt);
            }
        }
        if animating {
            self.needs_redraw = true;
        }

        if self.needs_redraw {
            self.needs_redraw = false;
            self.sync_layout();
            self.render()?;
            self.send_frame_callbacks();
        }
        Ok(())
    }

    /// Bound the event-loop sleep while an animation needs another frame.
    pub fn dispatch_wait(&self, idle_wait: Option<Duration>) -> Option<Duration> {
        const FRAME_INTERVAL: Duration = Duration::from_millis(16);
        if self.animations.values().any(RectAnimator::is_running) {
            Some(idle_wait.map_or(FRAME_INTERVAL, |wait| wait.min(FRAME_INTERVAL)))
        } else {
            idle_wait
        }
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

        if effects.workspace_changed {
            self.update_workspace_indicator();
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
            veyron_ipc::Request::Overview => Response::Overview(self.overview_report()),
            veyron_ipc::Request::CaptureSession => match self.wm.capture_session().to_toml() {
                Ok(toml) => {
                    let response = Response::Session { toml };
                    match serde_json::to_vec(&response) {
                        Ok(encoded) if encoded.len() < MAX_LINE_BYTES => response,
                        Ok(_) => Response::Error {
                            message: format!(
                                "captured session exceeds the {MAX_LINE_BYTES} byte IPC message limit"
                            ),
                        },
                        Err(error) => Response::Error {
                            message: format!("could not encode captured session: {error}"),
                        },
                    }
                }
                Err(message) => Response::Error { message },
            },
            veyron_ipc::Request::RestoreSession { toml } => match Session::from_toml(&toml) {
                Ok(session) => {
                    self.wm.restore_session(&session);
                    self.sync_layout();
                    self.sync_focus();
                    self.needs_redraw = true;
                    Response::Ok
                }
                Err(message) => Response::Error { message },
            },
            veyron_ipc::Request::Launch { query } => match crate::apps::launch(&query) {
                Ok(entry) => Response::Launched {
                    command: entry.command(),
                    name: entry.name,
                },
                Err(message) => Response::Error { message },
            },
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
            workspace_position: [snapshot.workspace_position.x, snapshot.workspace_position.y],
            layout: snapshot.layout.to_string(),
            gap: snapshot.gap,
            windows: snapshot
                .placements
                .iter()
                .map(|placement| {
                    let window = self.wm.window(placement.window);
                    WindowReport {
                        id: placement.window.0,
                        rect: [
                            placement.rect.x,
                            placement.rect.y,
                            placement.rect.w,
                            placement.rect.h,
                        ],
                        floating: placement.floating,
                        fullscreen: placement.fullscreen,
                        app_id: window
                            .map(|window| window.app_id().to_string())
                            .unwrap_or_default(),
                        title: window
                            .map(|window| window.title().to_string())
                            .unwrap_or_default(),
                        opacity: placement.opacity,
                    }
                })
                .collect(),
            focused: snapshot.focused.map(|id| id.0),
            scroll: snapshot.scroll,
        }
    }

    fn overview_report(&self) -> OverviewReport {
        let overview = self.wm.overview(OVERVIEW_TILE, OVERVIEW_GAP);
        let rect = |rect: Rect| [rect.x, rect.y, rect.w, rect.h];
        OverviewReport {
            bounds: rect(overview.bounds),
            viewport: rect(overview.viewport),
            tiles: overview
                .tiles
                .iter()
                .map(|tile| OverviewTileReport {
                    workspace: tile.workspace.clone(),
                    index: tile.index,
                    position: [tile.position.x, tile.position.y],
                    active: tile.active,
                    rect: rect(tile.rect),
                    windows: tile
                        .windows
                        .iter()
                        .map(|window| OverviewWindowReport {
                            id: window.window,
                            rect: rect(window.rect),
                            floating: window.floating,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Report the identity a client announced for its toplevel to the core, so
    /// window rules, `veyronctl state` and Veyron Recall see the real app id
    /// and title. Rules are applied by the core on the first announcement.
    fn sync_identity(&mut self, id: WindowId) {
        let Some(toplevel) = self.windows.get(&id).and_then(Window::toplevel) else {
            return;
        };
        let (app_id, title) = with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .map(|attributes| {
                    let attributes = attributes
                        .lock()
                        .expect("toplevel attributes are not poisoned");
                    (
                        attributes.app_id.clone().unwrap_or_default(),
                        attributes.title.clone().unwrap_or_default(),
                    )
                })
                .unwrap_or_default()
        });
        let effects = self.wm.set_window_identity(id, &app_id, &title);
        self.apply_effects(effects);
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

    /// Show the active spatial coordinate in the nested window title and host
    /// desktop decorations.
    fn update_workspace_indicator(&self) {
        let Some(backend) = self.backend.as_ref() else {
            return;
        };
        let position = self.wm.snapshot().workspace_position;
        backend.window().set_title(&format!(
            "VEYRON WM — Workspace ({}, {})",
            position.x, position.y
        ));
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
        self.damage_tracker = OutputDamageTracker::new(window_size, 1.0, Transform::Flipped180);

        // Panels and notification surfaces get exclusive zones and sizes
        // recomputed, then a configure for their new geometry.
        let layers: Vec<_> = {
            let mut map = layer_map_for_output(&self.output);
            map.arrange();
            map.layers().cloned().collect()
        };
        for layer in layers {
            layer.layer_surface().send_pending_configure();
        }
        self.sync_layer_work_area();
    }

    /// Mirror Smithay's current layer-shell reservation into the window model.
    /// The physical output rectangle remains unchanged for rendering.
    fn sync_layer_work_area(&mut self) {
        let area = {
            let zone = layer_map_for_output(&self.output).non_exclusive_zone();
            Rect::new(zone.loc.x, zone.loc.y, zone.size.w, zone.size.h)
        };
        if self.wm.set_output_work_area(0, area) {
            self.sync_layout();
            self.needs_redraw = true;
        }
    }

    /// Match the scene graph to the layout snapshot and configure clients.
    fn sync_layout(&mut self) {
        let snapshot = self.wm.snapshot();
        let motion = self.wm.config().motion;
        let slide = workspace_slide(
            self.last_workspace_position,
            snapshot.workspace_position,
            snapshot.viewport,
        );
        self.last_workspace_position = snapshot.workspace_position;

        for placement in &snapshot.placements {
            let Some(window) = self.windows.get(&placement.window) else {
                continue;
            };
            let target = placement.rect;
            let animator = self.animations.entry(placement.window).or_insert_with(|| {
                let drawn = match (
                    self.mapped.get(&placement.window),
                    self.configured.get(&placement.window),
                ) {
                    (Some((x, y)), Some((w, h))) => Rect::new(*x, *y, *w, *h),
                    // A workspace that just became active slides in from the
                    // direction the user travelled.
                    _ if placement.animate && slide != (0, 0) => {
                        Rect::new(target.x + slide.0, target.y + slide.1, target.w, target.h)
                    }
                    _ => target,
                };
                RectAnimator::new(drawn, target, &motion)
            });
            if animator.target() != target {
                // Restarting from what is on screen keeps a retile continuous;
                // a window whose rules disable animation snaps instead.
                let from = if placement.animate {
                    animator.current()
                } else {
                    target
                };
                *animator = RectAnimator::new(from, target, &motion);
            }
            let animated = animator.current();
            let location = (animated.x, animated.y);
            if self.mapped.get(&placement.window) != Some(&location) {
                self.space.map_element(window.clone(), location, false);
                self.mapped.insert(placement.window, location);
            }
            let target_size = (target.w, target.h);
            let previous_fullscreen = self.configured_fullscreen.get(&placement.window);
            let fullscreen_changed = previous_fullscreen != Some(&placement.fullscreen);
            // Floating windows keep their client-chosen size except while
            // entering or leaving fullscreen, when they need the target size.
            let should_configure_size =
                !placement.floating || placement.fullscreen || previous_fullscreen == Some(&true);
            let size_changed = should_configure_size
                && self.configured.get(&placement.window) != Some(&target_size);
            if !fullscreen_changed && !size_changed {
                continue;
            }
            if let Some(toplevel) = window.toplevel() {
                toplevel.with_pending_state(|state| {
                    if size_changed {
                        state.size = Some((target_size.0, target_size.1).into());
                    }
                    if placement.fullscreen {
                        state.states.set(xdg_toplevel::State::Fullscreen);
                    } else {
                        state.states.unset(xdg_toplevel::State::Fullscreen);
                    }
                });
                toplevel.send_configure();
                if size_changed {
                    self.configured.insert(placement.window, target_size);
                }
                self.configured_fullscreen
                    .insert(placement.window, placement.fullscreen);
            }
        }

        let stale: Vec<WindowId> = self
            .mapped
            .keys()
            .filter(|id| {
                !snapshot
                    .placements
                    .iter()
                    .any(|placement| placement.window == **id)
            })
            .copied()
            .collect();
        for id in stale {
            if let Some(window) = self.windows.get(&id) {
                self.space.unmap_elem(window);
            }
            self.mapped.remove(&id);
        }
    }

    fn handle_fullscreen_request(&mut self, surface: ToplevelSurface, fullscreen: bool) {
        let id = self
            .windows
            .iter()
            .find(|(_, window)| window.toplevel() == Some(&surface))
            .map(|(id, _)| *id);

        let Some(id) = id else {
            surface.send_configure();
            return;
        };

        if self.wm.set_window_fullscreen(id, fullscreen) {
            self.sync_layout();
            self.sync_focus();
            self.needs_redraw = true;
        } else {
            // A repeated request may leave the model unchanged. Reply with its
            // current state so the client still receives a configure.
            surface.send_configure();
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
        // After the first bind, the EGL surface remains current between swaps.
        // A missing or unavailable age forces a correct full redraw in Smithay.
        let age = backend.buffer_age().unwrap_or(0);
        let background = self.wm.config().appearance.background_rgba();
        let damage = {
            let (renderer, mut framebuffer) = backend.bind()?;
            let elements = self
                .space
                .render_elements_for_output(renderer, &self.output, 1.0)?;
            let rendered = self.damage_tracker.render_output(
                renderer,
                &mut framebuffer,
                age,
                &elements,
                background,
            )?;
            rendered.damage.cloned()
        };

        if let Some(damage) = damage.filter(|damage| !damage.is_empty()) {
            backend.submit(Some(&damage))?;
        }
        Ok(())
    }

    fn send_frame_callbacks(&self) {
        let time = self.start.elapsed().as_millis() as u32;
        for window in self.windows.values() {
            if let Some(toplevel) = window.toplevel() {
                send_frames_surface_tree(toplevel.wl_surface(), time);
            }
        }
        // Panels and other layer clients animate on frame callbacks too.
        let layers: Vec<_> = layer_map_for_output(&self.output)
            .layers()
            .cloned()
            .collect();
        for layer in layers {
            send_frames_surface_tree(layer.wl_surface(), time);
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

        if self
            .space
            .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            .is_some()
        {
            let rearranged = { layer_map_for_output(&self.output).arrange() };
            if rearranged {
                let layers: Vec<_> = layer_map_for_output(&self.output)
                    .layers()
                    .cloned()
                    .collect();
                for layer in layers {
                    layer.layer_surface().send_pending_configure();
                }
            }
            self.sync_layer_work_area();
            self.needs_redraw = true;
            return;
        }
        self.popup_manager.commit(surface);

        let mut committed = None;
        for (id, window) in &self.windows {
            if window
                .toplevel()
                .map(|toplevel| toplevel.wl_surface() == surface)
                .unwrap_or(false)
            {
                window.on_commit();
                committed = Some(*id);
            }
        }

        if let Some(id) = committed {
            self.sync_identity(id);
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
            self.configured_fullscreen.remove(&id);
            self.animations.remove(&id);
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
        self.handle_fullscreen_request(surface, true);
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        self.handle_fullscreen_request(surface, false);
    }
}

impl WlrLayerShellHandler for Veyron {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        output: Option<wl_output::WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        // Nested mode has exactly one output, so a surface that asks for any
        // other output is mapped here instead of being dropped.
        let output = output
            .as_ref()
            .and_then(Output::from_resource)
            .filter(|output| *output == self.output)
            .unwrap_or_else(|| self.output.clone());
        let layer_surface = DesktopLayerSurface::new(surface, namespace);
        if layer_map_for_output(&output)
            .map_layer(&layer_surface)
            .is_err()
        {
            tracing::warn!("could not map a layer surface");
            return;
        }
        // Mapping arranged the surface, so this is its initial configure.
        layer_surface.layer_surface().send_configure();
        self.sync_layer_work_area();
        self.needs_redraw = true;
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let doomed = layer_map_for_output(&self.output)
            .layers()
            .find(|layer| layer.layer_surface() == &surface)
            .cloned();
        if let Some(layer) = doomed {
            layer_map_for_output(&self.output).unmap_layer(&layer);
            self.sync_layer_work_area();
            self.needs_redraw = true;
        }
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        if self
            .popup_manager
            .track_popup(PopupKind::Xdg(popup))
            .is_err()
        {
            tracing::warn!("could not track a layer popup");
        }
    }
}

smithay::delegate_layer_shell!(Veyron);
smithay::delegate_compositor!(Veyron);
smithay::delegate_xdg_shell!(Veyron);
smithay::delegate_shm!(Veyron);
smithay::delegate_seat!(Veyron);
smithay::delegate_data_device!(Veyron);
smithay::delegate_output!(Veyron);
