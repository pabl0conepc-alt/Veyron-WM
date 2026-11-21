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
            damage::OutputDamageTracker,
            element::{
                render_elements,
                solid::{SolidColorBuffer, SolidColorRenderElement},
                surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
                AsRenderElements, Kind,
            },
            gles::{element::PixelShaderElement, GlesRenderer},
            utils::on_commit_buffer_handler,
        },
        winit::{WinitEvent, WinitGraphicsBackend, WinitInput},
    },
    desktop::{
        layer_map_for_output, LayerSurface as DesktopLayerSurface, PopupKind, PopupManager, Space,
        Window, WindowSurfaceType,
    },
    input::{
        keyboard::{xkb, FilterResult, KeyboardHandle},
        pointer::{AxisFrame, ButtonEvent, CursorImageStatus, MotionEvent, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
    reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason},
        protocol::{wl_buffer, wl_output, wl_seat, wl_shm, wl_surface::WlSurface},
        Client, DisplayHandle,
    },
    utils::{Logical, Point, Rectangle, Scale, Serial, Transform, SERIAL_COUNTER},
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
                decoration::{XdgDecorationHandler, XdgDecorationState},
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                XdgToplevelSurfaceData,
            },
        },
        shm::{ShmHandler, ShmState},
    },
};
use veyron_core::{
    workspace_slide, Action, Config, Design, Effects, Modifiers, NewWindow, Palette, Rect,
    RectAnimator, Session, Size as VSize, Snapshot, WindowId, Wm, WorkspacePosition,
};
use veyron_ipc::{
    OverviewReport, OverviewTileReport, OverviewWindowReport, Response, StateReport, ThemeReport,
    WindowReport, MAX_LINE_BYTES,
};

use crate::{
    decoration::{Decoration, DecorationParams, RoundedSurface, Shaders},
    ipc::Job,
};

/// Linux input event code of the left mouse button.
const BTN_LEFT: u32 = 0x110;

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
    /// Serves `zxdg_decoration_manager_v1`: Veyron draws borders, shadows and
    /// rounded corners itself, so clients are asked not to draw title bars.
    _decoration_state: XdgDecorationState,
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
    /// In-flight window rectangles; the scene graph draws `current()`.
    animations: HashMap<WindowId, RectAnimator>,
    last_animation_frame: Instant,
    /// Workspace drawn last, so a switch can slide in from the right side.
    last_workspace_position: WorkspacePosition,

    pointer_location: Point<f64, Logical>,
    /// Floating window being dragged with `super` + left button, and the
    /// pointer's offset from its top-left corner when the drag started.
    drag: Option<(WindowId, Point<f64, Logical>)>,
    /// Material You scheme generated from `[theme]`.
    palette: Palette,
    /// Window decorations (borders, shadows, rounded corners) kept across frames.
    chrome: Chrome,
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
        let decoration_state = XdgDecorationState::new::<Self>(&dh);
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

        let palette = wm.config().theme.palette();
        let mut state = Self {
            start: Instant::now(),
            config_path,
            compositor_state,
            xdg_shell_state,
            shm_state,
            seat_state,
            data_device_state,
            _output_manager_state: output_manager_state,
            layer_shell_state,
            _decoration_state: decoration_state,
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
            animations: HashMap::new(),
            last_animation_frame: Instant::now(),
            last_workspace_position: WorkspacePosition::default(),
            pointer_location: (0.0, 0.0).into(),
            drag: None,
            palette,
            chrome: Chrome::default(),
            needs_redraw: true,
            exit: false,
        };

        state.output.create_global::<Self>(&dh);
        state.refresh_theme();

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
                let modifiers = Modifiers {
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                    shift: modifiers.shift,
                    logo: modifiers.logo,
                };
                match binding_for(
                    data.wm.config(),
                    modifiers,
                    keysym.raw_latin_sym_or_raw_current_sym(),
                    keysym.modified_sym(),
                ) {
                    Some(action) => FilterResult::Intercept(action),
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

        if let Some((id, offset)) = self.drag {
            let corner = location - offset;
            if self
                .wm
                .move_floating_to(id, corner.x.round() as i32, corner.y.round() as i32)
            {
                self.sync_layout();
                self.needs_redraw = true;
            }
            // The dragged client does not see pointer motion during the grab.
            return;
        }

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
        // Only the left button ends a drag: the press that started it was never
        // sent to the client, while other buttons pressed meanwhile were, and
        // swallowing their release would leave them stuck in that client.
        if event.state() == ButtonState::Released
            && event.button_code() == BTN_LEFT
            && self.drag.take().is_some()
        {
            return;
        }
        // A click on a panel, OSD or notification above the windows belongs
        // to that layer surface; it must not focus, raise or drag the window
        // underneath.
        let over_layer = self
            .layer_surface_under(self.pointer_location, &[Layer::Overlay, Layer::Top])
            .is_some();
        if event.state() == ButtonState::Pressed && !over_layer {
            if let Some(drag) = self.begin_drag(event.button_code()) {
                self.drag = Some(drag);
                return;
            }
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

    /// Start dragging the floating window under the pointer when the left
    /// button is pressed with `super` held. Tiled windows are left to the
    /// layout, so the press is forwarded to them as usual.
    fn begin_drag(&mut self, button: u32) -> Option<(WindowId, Point<f64, Logical>)> {
        if button != BTN_LEFT || !self.keyboard.modifier_state().logo {
            return None;
        }
        let (window, _) = self.space.element_under(self.pointer_location)?;
        let window = window.clone();
        let id = self.window_id(&window)?;
        if !self.wm.snapshot().is_floating(id) {
            return None;
        }
        let corner = self.space.element_location(&window)?;
        if self.wm.focus_window(id) {
            self.sync_focus();
        }
        self.space.raise_element(&window, false);
        self.needs_redraw = true;
        Some((id, self.pointer_location - corner.to_f64()))
    }

    fn on_pointer_axis<E>(&mut self, event: &E)
    where
        E: PointerAxisEvent<WinitInput>,
    {
        let mut frame = AxisFrame::new(event.time_msec()).source(event.source());
        for axis in [Axis::Horizontal, Axis::Vertical] {
            let (v120, value) = axis_values(event.amount_v120(axis), event.amount(axis));
            if let Some(v120) = v120 {
                frame = frame.v120(axis, v120);
            }
            if let Some(value) = value {
                frame = frame.value(axis, value);
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

        if effects.theme_changed {
            self.refresh_theme();
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
            veyron_ipc::Request::Theme => self.theme_report(),
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

    /// Regenerate the Material palette from `[theme]` and, in a real session,
    /// write it to `$XDG_STATE_HOME/veyron/theme/` for waybar, GTK and shell
    /// widgets. Headless runs never touch the user's state directory.
    fn refresh_theme(&mut self) {
        // Corner radius and colours live in shader uniforms the damage tracker
        // cannot see, so a theme change redraws the whole output once.
        if let Some(mode) = self.output.current_mode() {
            self.damage_tracker = OutputDamageTracker::new(mode.size, 1.0, Transform::Flipped180);
        }
        let theme = &self.wm.config().theme;
        self.palette = theme.palette();
        self.needs_redraw = true;
        if !theme.export || self.backend.is_none() {
            return;
        }
        let Some(dir) = veyron_core::theme::export_dir() else {
            tracing::warn!("no HOME or XDG_STATE_HOME: theme files not exported");
            return;
        };
        match veyron_core::theme::export(&self.palette, &dir) {
            Ok(0) => {}
            Ok(written) => {
                tracing::info!(written, dir = %dir.display(), "theme exported");
            }
            Err(error) => {
                tracing::warn!(%error, dir = %dir.display(), "could not export theme");
            }
        }
    }

    fn theme_report(&self) -> Response {
        let theme = &self.wm.config().theme;
        Response::Theme(ThemeReport {
            mode: theme.mode.as_str().to_string(),
            seed: theme.seed.clone(),
            variant: theme.variant.clone(),
            colors: self
                .palette
                .roles
                .iter()
                .map(|(role, hex)| ((*role).to_string(), hex.clone()))
                .collect(),
            export_dir: veyron_core::theme::export_dir()
                .filter(|_| theme.export)
                .map(|dir| dir.display().to_string()),
        })
    }

    fn reload_config(&mut self) {
        match Config::load_or_default(&self.config_path) {
            Ok(config) => {
                tracing::info!(path = %self.config_path.display(), "configuration reloaded");
                self.wm.set_config(config);
                self.refresh_theme();
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
            // Floating windows are configured too: `resize`, snap zones,
            // picture-in-picture and fullscreen change their rectangle, and
            // only a configure makes the client follow. Unchanged sizes are
            // skipped below, so a floating client is not resized needlessly.
            let target_size = (target.w, target.h);
            if self.configured.get(&placement.window) == Some(&target_size) {
                continue;
            }
            if let Some(toplevel) = window.toplevel() {
                // Only the size is set here: activation is applied uniformly
                // by `Window::set_activated` in `sync_focus`, so a configure
                // can never mark every tiled window as active.
                toplevel.with_pending_state(|state| {
                    state.size = Some((target_size.0, target_size.1).into());
                });
                toplevel.send_configure();
                self.configured.insert(placement.window, target_size);
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
        // Panels, OSDs and notifications above windows take the pointer first;
        // wallpapers and desktop layers below only receive it where no window is.
        if let Some(hit) = self.layer_surface_under(location, &[Layer::Overlay, Layer::Top]) {
            return Some(hit);
        }
        let Some((window, _)) = self.space.element_under(location) else {
            return self.layer_surface_under(location, &[Layer::Bottom, Layer::Background]);
        };
        // Smithay expects the focused surface's origin in global coordinates and
        // derives surface-local positions from it; subsurfaces and popups are
        // resolved so they receive their own events.
        let origin = self.space.element_location(window)? - window.geometry().loc;
        window
            .surface_under(location - origin.to_f64(), WindowSurfaceType::ALL)
            .map(|(surface, offset)| (surface, (origin + offset).to_f64()))
    }

    /// The layer-shell surface (or one of its subsurfaces) under `location`,
    /// searching `layers` in order, with that surface's global origin.
    fn layer_surface_under(
        &self,
        location: Point<f64, Logical>,
        layers: &[Layer],
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let map = layer_map_for_output(&self.output);
        layers.iter().find_map(|layer| {
            let surface = map.layer_under(*layer, location)?;
            let origin = map.layer_geometry(surface)?.loc;
            surface
                .surface_under(location - origin.to_f64(), WindowSurfaceType::ALL)
                .map(|(wl_surface, offset)| (wl_surface, (origin + offset).to_f64()))
        })
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
        let config = self.wm.config();
        let background = if config.theme.use_surface_background {
            self.palette
                .rgba("surface")
                .unwrap_or_else(|| config.appearance.background_rgba())
        } else {
            config.appearance.background_rgba()
        };
        let snapshot = self.wm.snapshot();
        let damage = {
            let (renderer, mut framebuffer) = backend.bind()?;
            let elements = scene_elements(
                renderer,
                &self.space,
                &self.output,
                &self.windows,
                &snapshot,
                &mut self.chrome,
                Style::new(self.wm.config(), &self.palette, snapshot.output.h),
            );
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
                send_frames_surface_and_popups(toplevel.wl_surface(), time);
            }
        }
        // Panels and other layer clients animate on frame callbacks too.
        let layers: Vec<_> = layer_map_for_output(&self.output)
            .layers()
            .cloned()
            .collect();
        for layer in layers {
            send_frames_surface_and_popups(layer.wl_surface(), time);
        }
    }
}

render_elements! {
    /// Everything a frame is made of: client surfaces (optionally clipped to
    /// rounded corners), shader-drawn decorations and solid fallback borders.
    pub SceneElement<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Rounded=RoundedSurface,
    Decoration=PixelShaderElement,
    Solid=SolidColorRenderElement,
}

/// Window decoration appearance for one frame: the active design with the
/// Material palette's colours.
#[derive(Debug, Clone, Copy)]
struct Style {
    design: Design,
    active: [f32; 4],
    inactive: [f32; 4],
    shadow: [f32; 4],
    /// Maps framebuffer rows to output rows for the clip shader; the nested
    /// winit output is drawn vertically flipped.
    frag_map: [f32; 2],
}

impl Style {
    fn new(config: &Config, palette: &Palette, output_height: i32) -> Self {
        Self {
            design: config.theme.design(),
            active: palette.border_active_rgba(),
            inactive: palette.border_inactive_rgba(),
            shadow: palette.rgba("shadow").unwrap_or([0.0, 0.0, 0.0, 1.0]),
            frag_map: [output_height as f32, -1.0],
        }
    }
}

/// Per-window decoration state kept across frames so unchanged decorations
/// cause no damage, plus the lazily compiled shaders.
#[derive(Debug, Default)]
struct Chrome {
    /// `None` until the first frame tries to compile; `Some(None)` when the
    /// driver rejected the shaders and square solid borders are used.
    shaders: Option<Option<Shaders>>,
    shaded: HashMap<WindowId, Decoration>,
    solid: HashMap<WindowId, [SolidColorBuffer; 4]>,
}

impl Chrome {
    fn shaders(&mut self, renderer: &mut GlesRenderer) -> Option<Shaders> {
        self.shaders
            .get_or_insert_with(|| match Shaders::compile(renderer) {
                Ok(shaders) => Some(shaders),
                Err(error) => {
                    tracing::warn!(%error, "decoration shaders unavailable, using square borders");
                    None
                }
            })
            .clone()
    }

    fn forget_hidden(&mut self, snapshot: &Snapshot) {
        self.shaded
            .retain(|id, _| snapshot.placement_of(*id).is_some());
        self.solid
            .retain(|id, _| snapshot.placement_of(*id).is_some());
    }
}

/// Build the frame's render elements front to back: overlay and top layer
/// surfaces, then windows from the top of the stack down, each followed by
/// its decoration, then bottom and background layer surfaces.
///
/// A window's popups are drawn unclipped in front of it; its own surface tree
/// is clipped to rounded corners when the design asks for them. The output is
/// unscaled (scale 1.0), so logical and physical coordinates coincide.
fn scene_elements(
    renderer: &mut GlesRenderer,
    space: &Space<Window>,
    output: &Output,
    windows: &HashMap<WindowId, Window>,
    snapshot: &Snapshot,
    chrome: &mut Chrome,
    style: Style,
) -> Vec<SceneElement> {
    let scale = Scale::from(1.0);
    let mut elements = Vec::new();
    let map = layer_map_for_output(output);
    let push_layers =
        |elements: &mut Vec<SceneElement>, layer: Layer, renderer: &mut GlesRenderer| {
            for surface in map.layers_on(layer).rev() {
                if let Some(geometry) = map.layer_geometry(surface) {
                    elements.extend(
                        surface
                            .render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                                renderer,
                                geometry.loc.to_physical_precise_round(1.0),
                                scale,
                                1.0,
                            )
                            .into_iter()
                            .map(SceneElement::from),
                    );
                }
            }
        };
    push_layers(&mut elements, Layer::Overlay, renderer);
    push_layers(&mut elements, Layer::Top, renderer);

    let design = style.design;
    let shaders = if design.needs_shader() {
        chrome.shaders(renderer)
    } else {
        None
    };

    // `Space::elements` yields bottom to top; elements are drawn front first.
    let stacked: Vec<&Window> = space.elements().collect();
    for window in stacked.into_iter().rev() {
        let Some(location) = space.element_location(window) else {
            continue;
        };
        let id = windows
            .iter()
            .find(|(_, candidate)| *candidate == window)
            .map(|(id, _)| *id);
        let placement = id.and_then(|id| snapshot.placement_of(id));
        let focused = id.is_some() && snapshot.focused == id;
        let fullscreen = placement.is_some_and(|placement| placement.fullscreen);
        let mut alpha = placement.map_or(1.0, |placement| placement.opacity.clamp(0.0, 1.0));
        if !focused && !fullscreen {
            alpha *= design.inactive_opacity;
        }
        let geometry = window.geometry();
        let origin = (location - geometry.loc).to_physical_precise_round(1.0);
        let window_rect = Rectangle::new(location, geometry.size);

        match (&shaders, window.toplevel()) {
            (Some(shaders), Some(toplevel)) if design.corner_radius > 0 && !fullscreen => {
                let surface = toplevel.wl_surface();
                for (popup, offset) in PopupManager::popups_for_surface(surface) {
                    let popup_origin = origin
                        + (geometry.loc + offset - popup.geometry().loc)
                            .to_physical_precise_round(1.0);
                    elements.extend(
                        render_elements_from_surface_tree::<
                            _,
                            WaylandSurfaceRenderElement<GlesRenderer>,
                        >(
                            renderer,
                            popup.wl_surface(),
                            popup_origin,
                            scale,
                            alpha,
                            Kind::Unspecified,
                        )
                        .into_iter()
                        .map(SceneElement::from),
                    );
                }
                let clip = window_rect.to_physical_precise_round(1.0);
                elements.extend(
                    render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<GlesRenderer>>(
                        renderer,
                        surface,
                        origin,
                        scale,
                        alpha,
                        Kind::Unspecified,
                    )
                    .into_iter()
                    .map(|element| {
                        SceneElement::from(shaders.rounded(
                            element,
                            clip,
                            design.corner_radius as f32,
                            style.frag_map,
                        ))
                    }),
                );
            }
            _ => elements.extend(
                window
                    .render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                        renderer, origin, scale, alpha,
                    )
                    .into_iter()
                    .map(SceneElement::from),
            ),
        }

        let Some(id) = id.filter(|_| placement.is_some() && !fullscreen) else {
            continue;
        };
        let mut border = if focused {
            style.active
        } else {
            style.inactive
        };
        border[3] *= alpha;

        if let Some(shaders) = &shaders {
            let mut shadow = if design.glow && focused {
                style.active
            } else {
                style.shadow
            };
            shadow[3] = design.shadow_opacity * alpha;
            let params = DecorationParams {
                window: window_rect,
                radius: design.corner_radius,
                border_width: design.border_width,
                border_color: border,
                shadow_color: shadow,
                shadow_size: design.shadow_size,
                shadow_offset: design.shadow_offset,
                frag_map: style.frag_map,
            };
            let decoration = chrome
                .shaded
                .entry(id)
                .or_insert_with(|| Decoration::new(shaders.decoration_program(), params));
            decoration.update(params);
            elements.push(SceneElement::from(decoration.element()));
            continue;
        }

        if design.border_width <= 0 {
            continue;
        }
        let (x, y, w, h, b) = (
            location.x,
            location.y,
            geometry.size.w,
            geometry.size.h,
            design.border_width,
        );
        // Top, bottom, left, right: the frame sits just outside the window.
        let sides = [
            (x - b, y - b, w + 2 * b, b),
            (x - b, y + h, w + 2 * b, b),
            (x - b, y, b, h),
            (x + w, y, b, h),
        ];
        let buffers = chrome
            .solid
            .entry(id)
            .or_insert_with(|| std::array::from_fn(|_| SolidColorBuffer::new((0, 0), [0.0; 4])));
        for (buffer, (sx, sy, sw, sh)) in buffers.iter_mut().zip(sides) {
            buffer.update((sw, sh), border);
            elements.push(SceneElement::from(SolidColorRenderElement::from_buffer(
                buffer,
                (sx, sy),
                scale,
                1.0,
                Kind::Unspecified,
            )));
        }
    }
    chrome.forget_hidden(snapshot);

    push_layers(&mut elements, Layer::Bottom, renderer);
    push_layers(&mut elements, Layer::Background, renderer);
    elements
}

/// The action bound to a key press. The unmodified (level 1) symbol is used,
/// preferring a Latin layout, so `super+shift+1` matches the `1` key instead
/// of `exclam`, `alt+shift+tab` matches `tab` instead of `iso_left_tab`, and
/// bindings keep working on non-Latin layouts. A chord written with the typed
/// symbol (`super+shift+plus`, as before raw symbols were used) still matches
/// when no binding uses the unshifted key.
fn binding_for(
    config: &Config,
    modifiers: Modifiers,
    raw: Option<xkb::Keysym>,
    modified: xkb::Keysym,
) -> Option<Action> {
    raw.into_iter()
        .chain(std::iter::once(modified))
        .find_map(|sym| config.lookup(modifiers, &keysym_name(sym)))
        .cloned()
}

/// Lowercase keysym name, the form binding chords are stored in.
fn keysym_name(sym: xkb::Keysym) -> String {
    xkb::keysym_get_name(sym).to_ascii_lowercase()
}

/// Discrete and continuous scroll amounts for one axis of a pointer-axis
/// event: the `v120` value (120 per wheel notch) and the scroll distance.
fn axis_values(v120: Option<f64>, amount: Option<f64>) -> (Option<i32>, Option<f64>) {
    match v120 {
        // Smithay expects the raw v120 value, not a notch count: passing
        // notches made every wheel click 1/120 of a step for clients.
        Some(v120) => (
            Some(v120.round() as i32),
            Some(v120 / 120.0 * AXIS_STEP_PIXELS),
        ),
        None => (None, amount),
    }
}

/// Wait for a child on a small background thread. A dropped `Child` is never
/// waited for, so every exited application stayed a zombie process until the
/// compositor itself exited.
pub(crate) fn reap(mut child: std::process::Child) {
    let reaper = std::thread::Builder::new()
        .name("veyron-reaper".to_string())
        .stack_size(64 * 1024)
        .spawn(move || {
            let _ = child.wait();
        });
    if let Err(error) = reaper {
        tracing::warn!(%error, "could not start a thread to reap a child process");
    }
}

/// Run a shell command, returning the child's process id.
fn spawn(command: &str) -> Option<u32> {
    match std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .spawn()
    {
        Ok(child) => {
            tracing::info!(command, "spawned command");
            let pid = child.id();
            reap(child);
            Some(pid)
        }
        Err(error) => {
            tracing::error!(%error, command, "could not spawn command");
            None
        }
    }
}

/// Deliver frame callbacks to a toplevel or layer surface tree and to its
/// popups, which are separate surface trees.
fn send_frames_surface_and_popups(surface: &WlSurface, time: u32) {
    send_frames_surface_tree(surface, time);
    for (popup, _) in PopupManager::popups_for_surface(surface) {
        send_frames_surface_tree(popup.wl_surface(), time);
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
            self.needs_redraw = true;
            return;
        }
        self.popup_manager.commit(surface);
        // A popup may not attach a buffer before its first configure, and
        // nothing else sends one: without this, menus never appear.
        if let Some(PopupKind::Xdg(popup)) = self.popup_manager.find_popup(surface) {
            if !popup.is_initial_configure_sent() {
                if let Err(error) = popup.send_configure() {
                    tracing::warn!(?error, "could not configure a popup");
                }
            }
        }

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
            // `sync_layout` only configures windows that have a placement; one
            // opened behind a fullscreen window has none yet, and a client
            // may not draw before its first configure.
            if let Some(toplevel) = self.windows.get(&id).and_then(Window::toplevel) {
                if !toplevel.is_initial_configure_sent() {
                    toplevel.send_configure();
                }
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
        // Fullscreen is compositor policy in Veyron: acknowledge the request
        // without changing the layout.
        surface.send_configure();
    }

    fn unfullscreen_request(&mut self, _surface: ToplevelSurface) {}
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
        self.needs_redraw = true;
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let doomed = layer_map_for_output(&self.output)
            .layers()
            .find(|layer| layer.layer_surface() == &surface)
            .cloned();
        if let Some(layer) = doomed {
            layer_map_for_output(&self.output).unmap_layer(&layer);
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

/// Every toplevel gets server-side decorations: Veyron draws its own borders,
/// shadows and rounded corners, and client title bars would sit inside them.
impl XdgDecorationHandler for Veyron {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        set_server_side(&toplevel);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecorationMode) {
        set_server_side(&toplevel);
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        set_server_side(&toplevel);
    }
}

fn set_server_side(toplevel: &ToplevelSurface) {
    toplevel.with_pending_state(|state| {
        state.decoration_mode = Some(DecorationMode::ServerSide);
    });
    // Before the initial configure the mode simply rides along with it.
    if toplevel.is_initial_configure_sent() {
        toplevel.send_pending_configure();
    }
}

smithay::delegate_xdg_decoration!(Veyron);
smithay::delegate_compositor!(Veyron);
smithay::delegate_xdg_shell!(Veyron);
smithay::delegate_shm!(Veyron);
smithay::delegate_seat!(Veyron);
smithay::delegate_data_device!(Veyron);
smithay::delegate_output!(Veyron);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_notches_are_reported_in_v120_units() {
        // One notch is 120: clients divide by 120 themselves, and Smithay
        // derives `axis_discrete` for older clients from whole multiples.
        assert_eq!(
            axis_values(Some(120.0), None),
            (Some(120), Some(AXIS_STEP_PIXELS))
        );
        assert_eq!(
            axis_values(Some(-240.0), None),
            (Some(-240), Some(-2.0 * AXIS_STEP_PIXELS))
        );
        assert_eq!(axis_values(None, Some(3.5)), (None, Some(3.5)));
        assert_eq!(axis_values(None, None), (None, None));
    }

    #[test]
    fn bindings_match_the_unshifted_key_and_fall_back_to_the_typed_symbol() {
        let config = Config::from_toml_str(
            r#"
            [bindings]
            "super+shift+1" = "move-to-workspace 1"
            "super+shift+plus" = "gaps 4"
            "#,
        )
        .expect("valid config");
        let shifted = Modifiers {
            shift: true,
            logo: true,
            ..Modifiers::default()
        };
        // US layout: shift+1 types `exclam`, shift+equal types `plus`.
        assert_eq!(
            binding_for(&config, shifted, Some(xkb::Keysym::_1), xkb::Keysym::exclam),
            Some(Action::MoveToWorkspaceIndex { index: 1 })
        );
        assert_eq!(
            binding_for(
                &config,
                shifted,
                Some(xkb::Keysym::equal),
                xkb::Keysym::plus
            ),
            Some(Action::Gaps { delta: 4 }),
            "a chord written with the shifted symbol keeps working"
        );
        assert_eq!(
            binding_for(&config, shifted, Some(xkb::Keysym::_2), xkb::Keysym::at),
            None
        );
    }

    #[test]
    fn spawned_commands_do_not_linger_as_zombies() {
        let pid = spawn("exit 0").expect("sh is available");
        let stat = PathBuf::from(format!("/proc/{pid}/stat"));
        let deadline = Instant::now() + Duration::from_secs(5);
        // `/proc/<pid>` disappears once the exited child has been reaped.
        while stat.exists() {
            assert!(
                Instant::now() < deadline,
                "child {pid} exited but was never reaped"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
