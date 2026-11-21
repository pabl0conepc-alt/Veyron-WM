use std::{error::Error, sync::Arc, time::Instant};

use smithay::{
    backend::{
        input::{InputEvent, KeyboardKeyEvent},
        renderer::{
            Color32F, Frame, Renderer,
            element::{
                Kind,
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
            gles::GlesRenderer,
            utils::{draw_render_elements, on_commit_buffer_handler},
        },
        winit::{self as smithay_winit, WinitEvent},
    },
    delegate_compositor, delegate_seat, delegate_shm, delegate_xdg_shell,
    input::{Seat, SeatHandler, SeatState, keyboard::FilterResult},
    reexports::wayland_server::{Display, protocol::wl_seat},
    utils::{Rectangle, Serial, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes,
            TraversalAction, with_surface_tree_downward,
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
    },
};
use wayland_protocols::xdg::shell::server::xdg_toplevel;
use wayland_server::{
    Client, ListeningSocket,
    backend::{ClientData, ClientId, DisconnectReason},
    protocol::{
        wl_buffer,
        wl_surface::{self, WlSurface},
    },
};
use winit::platform::pump_events::PumpStatus;

// Window models are built and tested independently; Phase 2/3 protocol integration is pending.
#[allow(dead_code)]
mod window;

struct Veyron {
    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    shm_state: ShmState,
    seat_state: SeatState<Self>,
    // Retain the handle for the lifetime of the wl_seat global.
    #[allow(dead_code)]
    seat: Seat<Self>,
}

impl BufferHandler for Veyron {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl CompositorHandler for Veyron {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        // Smithay creates this data for every client inserted into this display.
        &client
            .get_data::<ClientState>()
            .expect("Veyron inserts clients with ClientState")
            .compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
    }
}

impl XdgShellHandler for Veyron {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
        });
        surface.send_configure();
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
    }
}

impl ShmHandler for Veyron {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl SeatHandler for Veyron {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
    }
}

delegate_compositor!(Veyron);
delegate_xdg_shell!(Veyron);
delegate_shm!(Veyron);
delegate_seat!(Veyron);

#[derive(Default)]
struct ClientState {
    compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {
        tracing::info!("Wayland client connected");
    }

    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {
        tracing::info!("Wayland client disconnected");
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("veyron_wm=info")),
        )
        .init();

    run_nested()
}

fn run_nested() -> Result<(), Box<dyn Error>> {
    let socket_name =
        std::env::var("VEYRON_WAYLAND_DISPLAY").unwrap_or_else(|_| "veyron-0".to_owned());
    let listener = ListeningSocket::bind(&socket_name)?;

    let mut display: Display<Veyron> = Display::new()?;
    let display_handle = display.handle();
    let compositor_state = CompositorState::new::<Veyron>(&display_handle);
    let shm_state = ShmState::new::<Veyron>(&display_handle, Vec::new());
    let mut seat_state = SeatState::new();
    let mut seat = seat_state.new_wl_seat(&display_handle, "veyron");
    let keyboard = seat.add_keyboard(Default::default(), 200, 200)?;

    let mut state = Veyron {
        compositor_state,
        xdg_shell_state: XdgShellState::new::<Veyron>(&display_handle),
        shm_state,
        seat_state,
        seat,
    };

    let (mut backend, mut event_loop) = smithay_winit::init::<GlesRenderer>()?;
    let started = Instant::now();
    let mut clients = Vec::new();
    tracing::info!(display = %socket_name, "Veyron nested compositor started");
    tracing::info!("Connect a Wayland client with WAYLAND_DISPLAY={socket_name}");

    while let PumpStatus::Continue = event_loop.dispatch_new_events(|event| match event {
            WinitEvent::Input(InputEvent::Keyboard { event }) => {
                keyboard.input::<(), _>(
                    &mut state,
                    event.key_code(),
                    event.state(),
                    0.into(),
                    0,
                    |_, _, _| FilterResult::Forward,
                );
            }
            WinitEvent::Input(InputEvent::PointerMotionAbsolute { .. }) => {
                let focus = state
                    .xdg_shell_state
                    .toplevel_surfaces()
                    .iter()
                    .next()
                    .map(|surface| surface.wl_surface().clone());
                if let Some(focus) = focus {
                    keyboard.set_focus(&mut state, Some(focus), 0.into());
                }
            }
            _ => {}
        })
    {

        let size = backend.window_size();
        let damage = Rectangle::from_size(size);
        {
            let (renderer, mut framebuffer) = backend.bind()?;
            let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = state
                .xdg_shell_state
                .toplevel_surfaces()
                .iter()
                .flat_map(|surface| {
                    render_elements_from_surface_tree(
                        renderer,
                        surface.wl_surface(),
                        (0, 0),
                        1.0,
                        1.0,
                        Kind::Unspecified,
                    )
                })
                .collect::<Vec<_>>();

            let mut frame = renderer.render(&mut framebuffer, size, Transform::Flipped180)?;
            frame.clear(Color32F::new(0.035, 0.043, 0.055, 1.0), &[damage])?;
            draw_render_elements(&mut frame, 1.0, &elements, &[damage])?;
            let sync = frame.finish()?;
            renderer.wait(&sync)?;

            let frame_time = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
            for surface in state.xdg_shell_state.toplevel_surfaces() {
                send_frame_callbacks(surface.wl_surface(), frame_time);
            }

            if let Some(stream) = listener.accept()? {
                let client = display
                    .handle()
                    .insert_client(stream, Arc::new(ClientState::default()))?;
                clients.push(client);
            }
            display.dispatch_clients(&mut state)?;
            display.flush_clients()?;
        }

        backend.submit(Some(&[damage]))?;
    }

    tracing::info!("Veyron nested compositor stopped");
    Ok(())
}

fn send_frame_callbacks(surface: &wl_surface::WlSurface, time: u32) {
    with_surface_tree_downward(
        surface,
        (),
        |_, _, &()| TraversalAction::DoChildren(()),
        |_, states, &()| {
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
