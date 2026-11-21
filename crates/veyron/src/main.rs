//! VEYRON WM — nested Wayland compositor entry point.

use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use calloop::{
    channel::{channel, Event, Sender},
    generic::Generic,
    EventLoop, Interest, Mode, PostAction,
};
use smithay::{
    backend::{renderer::gles::GlesRenderer, winit},
    reexports::wayland_server::Display,
    reexports::winit::{dpi::LogicalSize, window::Window as WinitWindow},
    wayland::socket::ListeningSocketSource,
};
use veyron_core::{Config, Size as VSize};

use state::{ClientState, Veyron};

mod apps;
mod decoration;
mod ipc;
mod state;

const HELP: &str = "\
VEYRON WM — scrollable tiling Wayland compositor

Usage: veyron [OPTIONS]

Options:
  -c, --config <PATH>   Configuration file (default: $XDG_CONFIG_HOME/veyron/veyron.toml)
  -s, --socket <PATH>   Control socket (default: $XDG_RUNTIME_DIR/veyron.sock)
      --print-config    Print the default configuration and exit
      --headless        Run without a display, GPU or DRM device (for tests/CI)
      --wayland-display <NAME>
                        Wayland socket name (default: wayland-veyron headless,
                        the first free wayland-N nested)
      --output-size <WxH>
                        Output size in pixels (default: 1920x1080 headless,
                        1280x800 for the nested window)
  -h, --help            Print this help
  -V, --version         Print the version

By default the compositor runs nested inside an existing Wayland or X11 session,
which is the interactive development backend. With --headless it serves the real
Wayland protocol over a socket without rendering, needing only XDG_RUNTIME_DIR.
Child applications connect through the wayland socket advertised in
WAYLAND_DISPLAY.

Control:  veyronctl state | veyronctl focus left | veyronctl quit
";

/// Headless bound also retries Wayland client flushes whose buffers were full.
const HEADLESS_WAIT: Duration = Duration::from_millis(50);
/// How long shutdown waits for an in-flight control reply before giving up.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

struct Args {
    config: PathBuf,
    socket: PathBuf,
    print_config: bool,
    headless: bool,
    wayland_display: Option<String>,
    /// `None` keeps the mode default (1920x1080 headless, 1280x800 nested).
    output_size: Option<VSize>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("veyron: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    if args.print_config {
        print!("{}", Config::default().to_toml_string()?);
        return Ok(());
    }

    init_tracing();

    let config = Config::load_or_default(&args.config)?;
    if !args.config.exists() {
        tracing::info!(
            path = %args.config.display(),
            "configuration file not found, using built-in defaults"
        );
    }

    if args.headless {
        run_headless(args, config)
    } else {
        run_nested(args, config)
    }
}

/// Run the compositor without a rendering backend: real `Display`, real socket,
/// real protocol handlers and the real window model.
///
/// This is the same `calloop` loop as the nested path; `render` is simply a
/// no-op and the display's poll fd is what wakes the loop.
fn run_headless(args: Args, config: Config) -> Result<(), Box<dyn Error>> {
    let output_size = args.output_size.unwrap_or(VSize::new(1920, 1080));
    let mut display: Display<Veyron> = Display::new()?;
    let socket_name = args.wayland_display.as_deref().unwrap_or("wayland-veyron");
    let socket_source = ListeningSocketSource::with_name(socket_name).map_err(|error| {
        format!(
            "could not bind the wayland socket '{socket_name}' ({error}); \
             headless mode needs XDG_RUNTIME_DIR set to a private directory"
        )
    })?;
    let wayland_display = socket_source.socket_name().to_os_string();
    std::env::set_var("WAYLAND_DISPLAY", &wayland_display);

    let mut event_loop: EventLoop<Veyron> = EventLoop::try_new()?;
    let mut state =
        Veyron::new_headless(args.config.clone(), config, output_size, &display.handle())?;

    let mut client_display = display.handle();
    event_loop
        .handle()
        .insert_source(socket_source, move |stream, _, _| {
            match client_display.insert_client(stream, Arc::new(ClientState::default())) {
                Ok(_) => tracing::debug!("accepted a wayland client"),
                Err(error) => tracing::warn!(%error, "could not accept a wayland client"),
            }
        })?;

    // Nothing else wakes this loop (there is no winit source), so wait on the
    // display's poll fd: it is readable while any client has pending requests.
    let wakeup = display.backend().poll_fd().try_clone_to_owned()?;
    event_loop.handle().insert_source(
        Generic::new(wakeup, Interest::READ, Mode::Level),
        |_, _, _| Ok::<_, std::io::Error>(PostAction::Continue),
    )?;

    let (ipc_tx, ipc_rx) = channel::<ipc::Job>();
    event_loop
        .handle()
        .insert_source(ipc_rx, |event, _, state| {
            if let Event::Msg(job) = event {
                state.handle_ipc(job);
            }
        })?;
    let ipc_activity = spawn_ipc_server(&args.socket, ipc_tx)?;

    tracing::info!(
        wayland = %wayland_display.to_string_lossy(),
        control = %args.socket.display(),
        width = output_size.w,
        height = output_size.h,
        "VEYRON WM is running headless"
    );

    serve(
        &mut event_loop,
        &mut display,
        &mut state,
        Some(HEADLESS_WAIT),
    )?;

    // Let the last control reply reach its client before the process exits.
    ipc_activity.wait_idle(SHUTDOWN_GRACE);
    let _ = std::fs::remove_file(&args.socket);
    tracing::info!("VEYRON WM stopped");
    Ok(())
}

/// Run the compositor loop until `state.exit` is set.
///
/// Each iteration blocks until a source is ready (bounded by `wait`), then
/// dispatches protocol events, advances the window model and flushes. Flushing
/// last means configures produced by `tick` leave in the same iteration.
fn serve(
    event_loop: &mut EventLoop<'static, Veyron>,
    display: &mut Display<Veyron>,
    state: &mut Veyron,
    wait: Option<Duration>,
) -> Result<(), Box<dyn Error>> {
    while !state.exit {
        event_loop.dispatch(state.dispatch_wait(wait), state)?;
        display.dispatch_clients(state)?;
        state.tick()?;
        display.flush_clients()?;
    }
    Ok(())
}

/// Serve the control socket on its own thread, forwarding jobs to the loop.
///
/// The returned [`veyron_ipc::Activity`] lets the compositor wait for the last
/// reply (for example the `Quit` acknowledgement) before it exits.
fn spawn_ipc_server(
    socket: &Path,
    sender: Sender<ipc::Job>,
) -> std::io::Result<veyron_ipc::Activity> {
    let socket = socket.to_path_buf();
    let activity = veyron_ipc::Activity::new();
    let server_activity = activity.clone();
    std::thread::Builder::new()
        .name("veyron-ipc".to_string())
        .spawn(move || {
            let result = veyron_ipc::serve(&socket, &server_activity, move |request| {
                ipc::handle(&sender, request)
            });
            if let Err(error) = result {
                tracing::error!(%error, "control socket stopped");
            }
        })?;
    Ok(activity)
}

/// Run the compositor nested inside an existing Wayland or X11 session.
fn run_nested(args: Args, config: Config) -> Result<(), Box<dyn Error>> {
    let mut display: Display<Veyron> = Display::new()?;
    let size = args.output_size.unwrap_or(VSize::new(1280, 800));
    let attributes = WinitWindow::default_attributes()
        .with_inner_size(LogicalSize::new(f64::from(size.w), f64::from(size.h)))
        .with_title("VEYRON WM")
        .with_visible(true);
    let (backend, winit_loop) = winit::init_from_attributes::<GlesRenderer>(attributes).map_err(|error| {
        format!(
            "could not open the nested backend ({error}); Veyron needs a running Wayland or X11 session"
        )
    })?;

    // The compositor's own Wayland socket. Children inherit WAYLAND_DISPLAY.
    let socket_source = match args.wayland_display.as_deref() {
        Some(name) => ListeningSocketSource::with_name(name)
            .map_err(|error| format!("could not bind the wayland socket '{name}' ({error})"))?,
        None => ListeningSocketSource::new_auto()?,
    };
    let wayland_display = socket_source.socket_name().to_os_string();
    std::env::set_var("WAYLAND_DISPLAY", &wayland_display);

    let mut event_loop: EventLoop<Veyron> = EventLoop::try_new()?;
    let mut state = Veyron::new(args.config, config, backend, &display.handle())?;

    let mut client_display = display.handle();
    event_loop
        .handle()
        .insert_source(socket_source, move |stream, _, _| {
            match client_display.insert_client(stream, Arc::new(ClientState::default())) {
                Ok(_) => tracing::debug!("accepted a wayland client"),
                Err(error) => tracing::warn!(%error, "could not accept a wayland client"),
            }
        })?;

    // Client protocol requests wake calloop directly. This lets the nested
    // compositor wait for a real source instead of waking at 60 Hz while idle.
    let wakeup = display.backend().poll_fd().try_clone_to_owned()?;
    event_loop.handle().insert_source(
        Generic::new(wakeup, Interest::READ, Mode::Level),
        |_, _, _| Ok::<_, std::io::Error>(PostAction::Continue),
    )?;

    event_loop
        .handle()
        .insert_source(winit_loop, |event, _, state: &mut Veyron| {
            state.on_winit_event(event);
        })?;

    let (ipc_tx, ipc_rx) = channel::<ipc::Job>();
    event_loop
        .handle()
        .insert_source(ipc_rx, |event, _, state| {
            if let Event::Msg(job) = event {
                state.handle_ipc(job);
            }
        })?;

    let ipc_activity = spawn_ipc_server(&args.socket, ipc_tx)?;

    tracing::info!(
        wayland = ?wayland_display,
        control = %args.socket.display(),
        "VEYRON WM is running"
    );

    serve(&mut event_loop, &mut display, &mut state, None)?;

    // Let the last control reply reach its client, then remove our socket so a
    // restart starts from a clean state.
    ipc_activity.wait_idle(SHUTDOWN_GRACE);
    let _ = std::fs::remove_file(&args.socket);
    tracing::info!("VEYRON WM stopped");
    Ok(())
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("veyron=info,warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut config = Config::default_path();
    let mut socket = veyron_ipc::default_socket_path();
    let mut print_config = false;
    let mut headless = false;
    let mut wayland_display = None;
    let mut output_size = None;

    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-c" | "--config" => {
                let path = args.next().ok_or("--config needs a path")?;
                config = PathBuf::from(path);
            }
            "-s" | "--socket" => {
                let path = args.next().ok_or("--socket needs a path")?;
                socket = PathBuf::from(path);
            }
            "--print-config" => print_config = true,
            "--headless" => headless = true,
            "--wayland-display" => {
                wayland_display = Some(args.next().ok_or("--wayland-display needs a name")?);
            }
            "--output-size" => {
                let value = args.next().ok_or("--output-size needs WxH")?;
                output_size = Some(parse_size(&value)?);
            }
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("veyron {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument '{other}' (try --help)").into()),
        }
    }

    Ok(Args {
        config,
        socket,
        print_config,
        headless,
        wayland_display,
        output_size,
    })
}

/// Parse `WIDTHxHEIGHT` into an output size.
fn parse_size(value: &str) -> Result<VSize, Box<dyn Error>> {
    let (width, height) = value
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("invalid size '{value}', expected WxH"))?;
    let width: i32 = width
        .parse()
        .map_err(|_| format!("invalid width in '{value}'"))?;
    let height: i32 = height
        .parse()
        .map_err(|_| format!("invalid height in '{value}'"))?;
    if width <= 0 || height <= 0 {
        return Err(format!("invalid size '{value}', dimensions must be positive").into());
    }
    Ok(VSize::new(width, height))
}
