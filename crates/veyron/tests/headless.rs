//! End-to-end proof that the Veyron compositor speaks Wayland.
//!
//! Each test spawns the real `veyron --headless` binary — the actual Smithay
//! `Display`, the actual listening socket, the actual protocol handlers and the
//! actual `veyron-core` window model — then connects a real `wayland-client`
//! over that socket, creates and commits `xdg-toplevel` surfaces with SHM
//! buffers, and asserts both on what the client observes (configures,
//! activation) and on Veyron's own state through its control protocol.
//!
//! No display server, GPU or DRM device is involved.

use std::{
    fs::{self, File, OpenOptions},
    os::{
        fd::AsFd,
        unix::{fs::PermissionsExt, net::UnixStream},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

use veyron_ipc::{Request, Response, StateReport};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{
        wl_buffer,
        wl_compositor::WlCompositor,
        wl_output, wl_registry,
        wl_seat::{self, WlSeat},
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols::xdg::shell::client::{
    xdg_surface, xdg_toplevel,
    xdg_wm_base::{self, XdgWmBase},
};

/// How long to wait for the compositor process to serve its sockets.
const READY_TIMEOUT: Duration = Duration::from_secs(10);
/// How long to wait for a protocol round trip to produce an expected event.
const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

/// A running `veyron --headless` process plus its runtime directory.
struct Harness {
    child: Child,
    dir: PathBuf,
    control_socket: PathBuf,
    wayland_socket: PathBuf,
}

impl Harness {
    fn start(test: &str, width: i32, height: i32) -> Self {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let dir =
            std::env::temp_dir().join(format!("veyron-it-{test}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).expect("create runtime dir");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .expect("restrict runtime dir permissions");

        let wayland_name = "wayland-veyron-it";
        let control_socket = dir.join("control.sock");
        let child = Command::new(env!("CARGO_BIN_EXE_veyron"))
            .arg("--headless")
            .arg("--wayland-display")
            .arg(wayland_name)
            .arg("--socket")
            .arg(&control_socket)
            .arg("--output-size")
            .arg(format!("{width}x{height}"))
            .arg("--config")
            .arg(dir.join("veyron.toml"))
            .env("XDG_RUNTIME_DIR", &dir)
            .env("RUST_LOG", "veyron=warn")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn the veyron compositor");

        let harness = Self {
            child,
            wayland_socket: dir.join(wayland_name),
            dir,
            control_socket,
        };
        harness.wait_ready();
        harness
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if matches!(
                veyron_ipc::request(&self.control_socket, &Request::Ping),
                Ok(Response::Pong)
            ) && self.wayland_socket.exists()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "veyron headless did not become ready within {READY_TIMEOUT:?} \
             (wayland socket exists: {}, control socket exists: {})",
            self.wayland_socket.exists(),
            self.control_socket.exists()
        );
    }

    fn state(&self) -> StateReport {
        match veyron_ipc::request(&self.control_socket, &Request::State) {
            Ok(Response::State(state)) => state,
            other => panic!("control protocol did not return a state report: {other:?}"),
        }
    }

    fn action(&self, action: &str) {
        match veyron_ipc::request(
            &self.control_socket,
            &Request::Action {
                action: action.to_string(),
            },
        ) {
            Ok(Response::Ok) => {}
            other => panic!("action '{action}' was not accepted: {other:?}"),
        }
    }

    fn shutdown(&mut self) {
        match veyron_ipc::request(&self.control_socket, &Request::Quit) {
            Ok(Response::Ok) => {}
            other => panic!("quit was not accepted: {other:?}"),
        }

        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    assert!(status.success(), "veyron exited with {status}");
                    break;
                }
                Ok(None) => {}
                Err(error) => panic!("could not wait for veyron: {error}"),
            }
            if Instant::now() >= deadline {
                panic!("veyron did not exit after a quit request");
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(
            !self.control_socket.exists(),
            "the control socket is removed on shutdown"
        );
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ToplevelConfigure {
    width: i32,
    height: i32,
    activated: bool,
}

#[derive(Debug, Default)]
struct ClientState {
    last_configure: Option<ToplevelConfigure>,
    acked_serials: Vec<u32>,
    configure_count: usize,
    buffer_releases: usize,
    ping_count: usize,
}

/// A minimal but genuine Wayland client.
struct Client {
    queue: EventQueue<ClientState>,
    state: ClientState,
    qh: QueueHandle<ClientState>,
    compositor: WlCompositor,
    wm_base: XdgWmBase,
    shm: WlShm,
    // Kept alive so the server-side globals stay bound for the whole test.
    _seat: WlSeat,
    _output: wl_output::WlOutput,
}

impl Client {
    fn connect(wayland_socket: &Path) -> Self {
        let stream = UnixStream::connect(wayland_socket).expect("connect to veyron socket");
        stream
            .set_read_timeout(Some(EVENT_TIMEOUT))
            .expect("set socket timeout");
        let conn = Connection::from_socket(stream).expect("create a wayland connection");
        let (globals, queue) =
            registry_queue_init::<ClientState>(&conn).expect("read the registry");
        let qh = queue.handle();

        let compositor = globals
            .bind::<WlCompositor, ClientState, ()>(&qh, 4..=6, ())
            .expect("wl_compositor");
        let wm_base = globals
            .bind::<XdgWmBase, ClientState, ()>(&qh, 1..=6, ())
            .expect("xdg_wm_base");
        let shm = globals
            .bind::<WlShm, ClientState, ()>(&qh, 1..=1, ())
            .expect("wl_shm");
        let seat = globals
            .bind::<WlSeat, ClientState, ()>(&qh, 1..=8, ())
            .expect("wl_seat");
        let output = globals
            .bind::<wl_output::WlOutput, ClientState, ()>(&qh, 1..=4, ())
            .expect("wl_output");

        let mut client = Self {
            queue,
            state: ClientState::default(),
            qh,
            compositor,
            wm_base,
            shm,
            _seat: seat,
            _output: output,
        };
        client.roundtrip();
        client
    }

    fn roundtrip(&mut self) {
        self.queue
            .roundtrip(&mut self.state)
            .expect("wayland round trip");
    }

    fn configure(&self) -> ToplevelConfigure {
        self.state
            .last_configure
            .expect("the compositor has sent a configure")
    }

    fn wait_for_configure(&mut self) -> ToplevelConfigure {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            if let Some(configure) = self.state.last_configure {
                if configure.width > 0 && configure.height > 0 {
                    return configure;
                }
            }
            assert!(
                Instant::now() < deadline,
                "no usable configure arrived: {:?}",
                self.state.last_configure
            );
            self.roundtrip();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn wait_for_activated(&mut self, expected: bool) -> ToplevelConfigure {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            if let Some(configure) = self.state.last_configure {
                if configure.activated == expected {
                    return configure;
                }
            }
            assert!(
                Instant::now() < deadline,
                "activation never became {expected}; last configure: {:?}",
                self.state.last_configure
            );
            self.roundtrip();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn wait_for_width_change(&mut self, previous: i32) -> ToplevelConfigure {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            if let Some(configure) = self.state.last_configure {
                if configure.width != previous && configure.width > 0 {
                    return configure;
                }
            }
            assert!(
                Instant::now() < deadline,
                "width was not reconfigured from {previous}; last configure: {:?}",
                self.state.last_configure
            );
            self.roundtrip();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Create a toplevel, wait for its configure, attach a real SHM buffer and
    /// commit it — the same sequence any toolkit performs.
    fn create_toplevel(&mut self, title: &str) -> Toplevel {
        let surface = self.compositor.create_surface(&self.qh, ());
        let xdg_surface = self.wm_base.get_xdg_surface(&surface, &self.qh, ());
        let toplevel = xdg_surface.get_toplevel(&self.qh, ());
        toplevel.set_title(title.to_string());
        toplevel.set_app_id("veyron-integration-test".to_string());
        surface.commit();

        let configure = self.wait_for_configure();
        // The server sends `xdg_toplevel.configure` first and the
        // `xdg_surface.configure` serial afterwards; our Dispatch impl acks the
        // serial as soon as it arrives, which is the standard client sequence.
        assert!(
            !self.state.acked_serials.is_empty(),
            "the compositor's configure was acknowledged"
        );

        let stride = configure.width * 4;
        let length = (stride as usize) * (configure.height as usize);
        let path = std::env::temp_dir().join(format!(
            "veyron-buffer-{}-{}.shm",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        // The compositor mmaps the pool, so the fd must be readable *and*
        // writable: a write-only fd makes wl_shm fail with an invalid-fd error.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("create the shm file");
        file.set_len(length as u64).expect("size the shm file");

        let pool = self
            .shm
            .create_pool(file.as_fd(), length as i32, &self.qh, ());
        let buffer = pool.create_buffer(
            0,
            configure.width,
            configure.height,
            stride,
            wl_shm::Format::Argb8888,
            &self.qh,
            (),
        );

        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, configure.width, configure.height);
        surface.commit();
        self.roundtrip();

        Toplevel {
            surface,
            xdg_surface,
            toplevel,
            _pool: pool,
            _buffer: buffer,
            _file: file,
            file_path: path,
        }
    }
}

struct Toplevel {
    surface: WlSurface,
    xdg_surface: xdg_surface::XdgSurface,
    toplevel: xdg_toplevel::XdgToplevel,
    _pool: WlShmPool,
    _buffer: wl_buffer::WlBuffer,
    _file: File,
    file_path: PathBuf,
}

impl Toplevel {
    fn destroy(self) {
        self.toplevel.destroy();
        self.xdg_surface.destroy();
        self.surface.destroy();
    }
}

impl Drop for Toplevel {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.file_path);
    }
}

// ---------------------------------------------------------------------------
// Dispatch implementations: this is a normal Wayland client, binding the same
// globals any application would bind.
// ---------------------------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for ClientState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlCompositor, ()> for ClientState {
    fn event(
        _state: &mut Self,
        _proxy: &WlCompositor,
        _event: <WlCompositor as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShm, ()> for ClientState {
    fn event(
        _state: &mut Self,
        _proxy: &WlShm,
        _event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShmPool, ()> for ClientState {
    fn event(
        _state: &mut Self,
        _proxy: &WlShmPool,
        _event: <WlShmPool as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            state.buffer_releases += 1;
        }
    }
}

impl Dispatch<WlSeat, ()> for ClientState {
    fn event(
        _state: &mut Self,
        _proxy: &WlSeat,
        _event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for ClientState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_output::WlOutput,
        _event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for ClientState {
    fn event(
        _state: &mut Self,
        _proxy: &WlSurface,
        _event: <WlSurface as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgWmBase, ()> for ClientState {
    fn event(
        state: &mut Self,
        proxy: &XdgWmBase,
        event: xdg_wm_base::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            state.ping_count += 1;
            proxy.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for ClientState {
    fn event(
        state: &mut Self,
        proxy: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            state.acked_serials.push(serial);
            proxy.ack_configure(serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Configure {
            width,
            height,
            states,
        } = event
        {
            // xdg-shell transports the state list as an array of u32 values.
            let activated = states.chunks_exact(4).any(|chunk| {
                let value = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                matches!(
                    xdg_toplevel::State::try_from(value),
                    Ok(xdg_toplevel::State::Activated)
                )
            });

            state.last_configure = Some(ToplevelConfigure {
                width,
                height,
                activated,
            });
            state.configure_count += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn headless_maps_toplevel_with_configure_matching_layout() {
    let harness = Harness::start("map", 1280, 800);
    let mut client = Client::connect(&harness.wayland_socket);
    let toplevel = client.create_toplevel("first");

    let configure = client.configure();
    let state = harness.state();

    assert_eq!(state.workspace, "1");
    assert_eq!(
        state.windows.len(),
        1,
        "the window is registered in the core model"
    );
    let window = &state.windows[0];
    assert_eq!(
        state.focused,
        Some(window.id),
        "the new window takes focus in the model"
    );
    assert!(!window.floating && !window.fullscreen);

    // Output 1280x800, outer margin 12 -> viewport 1256x776 at (12, 12);
    // default column ratio 0.5 -> 628px slot minus the 8px gap = 620px.
    assert_eq!(window.rect, [12, 12, 620, 776]);
    assert_eq!(
        (configure.width, configure.height),
        (window.rect[2], window.rect[3]),
        "the client's configure matches the tiling layout"
    );
    assert!(configure.activated, "the focused window is activated");

    toplevel.destroy();
    client.roundtrip();

    let state = harness.state();
    assert!(
        state.windows.is_empty(),
        "destroying the surface unmaps the window"
    );
    assert_eq!(state.focused, None, "focus is cleared with the last window");
}

#[test]
fn focus_activation_and_reconfigure_follow_the_model() {
    let harness = Harness::start("focus", 1280, 800);
    let mut first = Client::connect(&harness.wayland_socket);
    let _first_toplevel = first.create_toplevel("first");
    first.wait_for_activated(true);

    let mut second = Client::connect(&harness.wayland_socket);
    let _second_toplevel = second.create_toplevel("second");
    second.wait_for_activated(true);

    let state = harness.state();
    assert_eq!(state.windows.len(), 2);
    assert_eq!(
        state.focused,
        Some(state.windows[1].id),
        "the newest window is focused"
    );
    first.wait_for_activated(false);

    // A layout change must reach the client as a new configure.
    let width_before = second.configure().width;
    harness.action("column-width -20");
    let reconfigured = second.wait_for_width_change(width_before);
    let state = harness.state();
    let second_window = &state.windows[1];
    assert_eq!(
        reconfigured.width, second_window.rect[2],
        "the reconfigured width matches the layout: 1256 * 0.3 - 8 = 369"
    );
    assert_eq!(second_window.rect[2], 369);
    assert_ne!(reconfigured.width, width_before);

    // Moving focus is observable by both clients through activation.
    harness.action("focus left");
    first.wait_for_activated(true);
    second.wait_for_activated(false);
    let state = harness.state();
    assert_eq!(state.focused, Some(state.windows[0].id));
    assert_ne!(state.windows[0].id, state.windows[1].id);
}

#[test]
fn workspace_scroll_and_navigation_reach_clients() {
    let harness = Harness::start("workspaces", 1280, 800);
    let mut first = Client::connect(&harness.wayland_socket);
    let _first_toplevel = first.create_toplevel("first");
    let mut _second = Client::connect(&harness.wayland_socket);
    let _second_toplevel = _second.create_toplevel("second");
    let mut third = Client::connect(&harness.wayland_socket);
    let _third_toplevel = third.create_toplevel("third");
    third.wait_for_activated(true);

    let state = harness.state();
    assert_eq!(state.windows.len(), 3);
    assert_eq!(
        state.scroll, 620.0,
        "three half-viewport columns scroll to the strip end"
    );
    assert_eq!(state.focused, Some(state.windows[2].id));

    // Walking focus back to the first column scrolls the strip to the start.
    harness.action("focus left");
    harness.action("focus left");
    let state = harness.state();
    assert_eq!(state.scroll, 0.0, "focus drives the viewport scroll");
    assert_eq!(state.focused, Some(state.windows[0].id));
    first.wait_for_activated(true);
    third.wait_for_activated(false);

    // Switching workspaces unmaps the strip: clients observe deactivation.
    harness.action("workspace 2");
    let state = harness.state();
    assert_eq!(state.workspace_index, 1);
    assert_eq!(state.workspace_count, 2);
    assert!(state.windows.is_empty(), "the new workspace has no windows");
    first.wait_for_activated(false);

    // Coming back restores the strip and its focus.
    harness.action("workspace 1");
    let state = harness.state();
    assert_eq!(state.workspace_index, 0);
    assert_eq!(state.windows.len(), 3);
    assert_eq!(state.focused, Some(state.windows[0].id));
    first.wait_for_activated(true);
}

#[test]
fn headless_shuts_down_cleanly_on_quit_request() {
    let mut harness = Harness::start("quit", 800, 600);
    let _client = Client::connect(&harness.wayland_socket);
    harness.shutdown();
}
