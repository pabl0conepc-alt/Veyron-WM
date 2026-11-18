//! VEYRON WM — nested Wayland compositor entry point.

use std::{error::Error, path::PathBuf, sync::Arc, time::Duration};

use calloop::{
    channel::{channel, Event},
    EventLoop,
};
use smithay::{
    backend::{renderer::gles::GlesRenderer, winit},
    reexports::wayland_server::{Display, ListeningSocket},
    wayland::socket::ListeningSocketSource,
};
use veyron_core::{Config, Size as VSize};

use state::{ClientState, Veyron};

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
                        Socket name for headless mode (default: wayland-veyron)
      --output-size <WxH>
                        Headless output size in pixels (default: 1920x1080)
  -h, --help            Print this help
  -V, --version         Print the version

By default the compositor runs nested inside an existing Wayland or X11 session,
which is the interactive development backend. With --headless it serves the real
Wayland protocol over a socket without rendering, needing only XDG_RUNTIME_DIR.
Child applications connect through the wayland socket advertised in
WAYLAND_DISPLAY.

Control:  veyronctl state | veyronctl focus left | veyronctl quit
";

/// How long shutdown waits for an in-flight control reply before giving up.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

struct Args {
    config: PathBuf,
    socket: PathBuf,
    print_config: bool,
    headless: bool,
    wayland_display: Option<String>,
    output_size: VSize,
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

/// Run the compositor without a rendering backend: real Display, real socket,
/// real protocol handlers and the real window model.
fn run_headless(args: Args, config: Config) -> Result<(), Box<dyn Error>> {
    let mut display: Display<Veyron> = Display::new()?;
    let socket_name = args.wayland_display.as_deref().unwrap_or("wayland-veyron");
    let listener = ListeningSocket::bind(socket_name).map_err(|error| {
        format!(
            "could not bind the wayland socket '{socket_name}' ({error}); \
             headless mode needs XDG_RUNTIME_DIR set to a private directory"
        )
    })?;
    let wayland_display = listener
        .socket_name()
        .map(|name| name.to_os_string())
        .ok_or("the listening socket has no name")?;
    std::env::set_var("WAYLAND_DISPLAY", &wayland_display);

    let mut state = Veyron::new_headless(
        args.config.clone(),
        config,
        args.output_size,
        &display.handle(),
    )?;
    let mut client_handle = display.handle();

    let (ipc_tx, ipc_rx) = channel::<ipc::Job>();
    let ipc_socket = args.socket.clone();
    let ipc_activity = veyron_ipc::Activity::new();
    let server_activity = ipc_activity.clone();
    std::thread::Builder::new()
        .name("veyron-ipc".to_string())
        .spawn(move || {
            let result = veyron_ipc::serve(&ipc_socket, &server_activity, move |request| {
                ipc::handle(&ipc_tx, request)
            });
            if let Err(error) = result {
                tracing::error!(%error, "control socket stopped");
            }
        })?;

    tracing::info!(
        wayland = %wayland_display.to_string_lossy(),
        control = %args.socket.display(),
        width = args.output_size.w,
        height = args.output_size.h,
        "VEYRON WM is running headless"
    );

    while !state.exit {
        // Control requests are answered on the compositor thread.
        while let Ok(job) = ipc_rx.try_recv() {
            state.handle_ipc(job);
        }

        while let Some(stream) = listener.accept()? {
            client_handle
                .insert_client(stream, Arc::new(ClientState::default()))
                .map_err(|error| format!("could not accept a wayland client: {error:?}"))?;
        }

        display.dispatch_clients(&mut state)?;
        state.tick()?;
        display.flush_clients()?;

        std::thread::sleep(Duration::from_millis(1));
    }

    // Let the last control reply reach its client before the process exits.
    ipc_activity.wait_idle(SHUTDOWN_GRACE);
    let _ = std::fs::remove_file(&args.socket);
    tracing::info!("VEYRON WM stopped");
    Ok(())
}

/// Run the compositor nested inside an existing Wayland or X11 session.
fn run_nested(args: Args, config: Config) -> Result<(), Box<dyn Error>> {
    let mut display: Display<Veyron> = Display::new()?;
    let (backend, winit_loop) = winit::init::<GlesRenderer>().map_err(|error| {
        format!(
            "could not open the nested backend ({error}); Veyron needs a running Wayland or X11 session"
        )
    })?;

    // The compositor's own Wayland socket. Children inherit WAYLAND_DISPLAY.
    let socket_source = ListeningSocketSource::new_auto()?;
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

    let ipc_socket = args.socket.clone();
    let server_tx = ipc_tx.clone();
    let ipc_activity = veyron_ipc::Activity::new();
    let server_activity = ipc_activity.clone();
    std::thread::Builder::new()
        .name("veyron-ipc".to_string())
        .spawn(move || {
            let result = veyron_ipc::serve(&ipc_socket, &server_activity, move |request| {
                ipc::handle(&server_tx, request)
            });
            if let Err(error) = result {
                tracing::error!(%error, "control socket stopped");
            }
        })?;

    tracing::info!(
        wayland = ?wayland_display,
        control = %args.socket.display(),
        "VEYRON WM is running"
    );

    while !state.exit {
        // Bounded wait: client commits arrive on the display socket, which is
        // dispatched explicitly below, so the loop must not block indefinitely.
        event_loop.dispatch(Some(Duration::from_millis(16)), &mut state)?;
        display.dispatch_clients(&mut state)?;
        display.flush_clients()?;
        state.tick()?;
    }

    // Let the last control reply reach its client, then remove our socket so a
    // restart starts from a clean state.
    ipc_activity.wait_idle(SHUTDOWN_GRACE);
    let _ = std::fs::remove_file(veyron_ipc::default_socket_path());
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
    let mut output_size = VSize::new(1920, 1080);

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
                output_size = parse_size(&value)?;
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
