//! `veyronctl` — control client for VEYRON WM.

use std::{error::Error, path::PathBuf, process::ExitCode};

use veyron_ipc::{Request, Response};

const HELP: &str = "\
veyronctl — control a running VEYRON WM

Usage:
  veyronctl state [--json]        Show the current layout
  veyronctl overview [--json]     Show every workspace on its grid
  veyronctl session save <FILE>   Save the current session as TOML
  veyronctl session restore <FILE> Restore a session from TOML
  veyronctl launch <NAME>         Start an installed application
  veyronctl watch [--waybar]      Print the state as a JSON line on every change
                                  (--waybar: a waybar custom-module line)
  veyronctl theme [--json]        Show the Material You colours
  veyronctl theme <OP>            toggle, dark, light, next, seed <#hex|preset>,
                                  variant <name|next>
  veyronctl ping                  Check the compositor is alive
  veyronctl reload                Reload the configuration
  veyronctl quit                  Ask the compositor to exit
  veyronctl <action...>           Run any action, e.g. `veyronctl focus left`

Options:
  -s, --socket <PATH>   Control socket (default: $VEYRON_SOCKET or $XDG_RUNTIME_DIR/veyron.sock)
      --json            Print the state as JSON
  -h, --help            Print this help
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("veyronctl: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut socket = veyron_ipc::default_socket_path();
    let mut json = false;
    let mut waybar = false;
    let mut words: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-s" | "--socket" => {
                let path = args.next().ok_or("--socket needs a path")?;
                socket = PathBuf::from(path);
            }
            "--json" => json = true,
            "--waybar" => waybar = true,
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            _ => words.push(argument),
        }
    }

    if words.is_empty() {
        print!("{HELP}");
        return Err("no command given".into());
    }

    if words[0] == "watch" {
        return watch(&socket, waybar);
    }

    let mut save_session_to = None;
    let request = match words[0].as_str() {
        "state" | "status" if words.len() == 1 => Request::State,
        "overview" if words.len() == 1 => Request::Overview,
        "session" if words.len() == 3 && words[1] == "save" => {
            save_session_to = Some(PathBuf::from(&words[2]));
            Request::CaptureSession
        }
        "session" if words.len() == 3 && words[1] == "restore" => {
            let toml = std::fs::read_to_string(&words[2])?;
            Request::RestoreSession { toml }
        }
        "session" => return Err("usage: veyronctl session save|restore <FILE>".into()),
        "launch" if words.len() >= 2 => Request::Launch {
            query: words[1..].join(" "),
        },
        "launch" => return Err("usage: veyronctl launch <NAME>".into()),
        "theme" if words.len() == 1 => Request::Theme,
        "ping" if words.len() == 1 => Request::Ping,
        "reload" if words.len() == 1 => Request::Reload,
        "quit" if words.len() == 1 => Request::Quit,
        "action" => Request::Action {
            action: words[1..].join(" "),
        },
        // `veyronctl focus left` and friends.
        _ => Request::Action {
            action: words.join(" "),
        },
    };

    let response = veyron_ipc::request(&socket, &request)
        .map_err(|error| format!("could not reach Veyron at {} ({error})", socket.display()))?;

    match response {
        Response::Ok => Ok(()),
        Response::Pong => {
            println!("pong");
            Ok(())
        }
        Response::Overview(report) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                for tile in &report.tiles {
                    println!(
                        "workspace {} ({}, {}) {}x{} at ({}, {}) windows {}{}",
                        tile.workspace,
                        tile.position[0],
                        tile.position[1],
                        tile.rect[2],
                        tile.rect[3],
                        tile.rect[0],
                        tile.rect[1],
                        tile.windows.len(),
                        if tile.active { "  active" } else { "" }
                    );
                }
            }
            Ok(())
        }
        Response::Session { toml } => {
            let path = save_session_to.ok_or("unexpected session response")?;
            std::fs::write(&path, toml)?;
            println!("saved session to {}", path.display());
            Ok(())
        }
        Response::Launched { name, command } => {
            println!("launched {name} ({command})");
            Ok(())
        }
        Response::Theme(report) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "theme {} seed {} variant {}",
                    report.mode, report.seed, report.variant
                );
                for (role, hex) in &report.colors {
                    println!("  {role:<28} {hex}");
                }
                if let Some(dir) = &report.export_dir {
                    println!("exported to {dir}");
                }
            }
            Ok(())
        }
        Response::State(report) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "workspace {} ({}) {}/{} @ ({}, {})  windows {}  gap {}px  scroll {:.0}px",
                    report.workspace,
                    report.layout,
                    report.workspace_index + 1,
                    report.workspace_count,
                    report.workspace_position[0],
                    report.workspace_position[1],
                    report.windows.len(),
                    report.gap,
                    report.scroll
                );
                for window in &report.windows {
                    let marker = if report.focused == Some(window.id) {
                        "*"
                    } else {
                        " "
                    };
                    let kind = if window.fullscreen {
                        "fullscreen"
                    } else if window.floating {
                        "floating"
                    } else {
                        "tiled"
                    };
                    println!(
                        "{marker} #{} {:<10} app={:<24} x={} y={} w={} h={} opacity={:.2}",
                        window.id,
                        kind,
                        window.app_id,
                        window.rect[0],
                        window.rect[1],
                        window.rect[2],
                        window.rect[3],
                        window.opacity
                    );
                    if !window.title.is_empty() {
                        println!("      title: {}", window.title);
                    }
                }
            }
            Ok(())
        }
        Response::Error { message } => Err(message.into()),
    }
}

/// How often `watch` samples the compositor state.
const WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// `veyronctl watch`: print the state as one JSON line whenever it changes, for
/// bars and shell widgets. With `--waybar` each line is a waybar custom-module
/// object. Survives compositor restarts by reconnecting.
fn watch(socket: &std::path::Path, waybar: bool) -> Result<(), Box<dyn Error>> {
    use std::io::Write as _;

    let mut last = String::new();
    loop {
        let line = match veyron_ipc::request(socket, &Request::State) {
            Ok(Response::State(report)) if waybar => waybar_line(&report),
            Ok(Response::State(report)) => serde_json::to_string(&report)?,
            _ if waybar => {
                r#"{"text":"","tooltip":"Veyron is not running","class":"offline"}"#.to_string()
            }
            _ => String::new(),
        };
        if line != last {
            if !line.is_empty() {
                let mut stdout = std::io::stdout().lock();
                // A closed pipe (the bar exited) ends the watcher.
                if writeln!(stdout, "{line}")
                    .and_then(|()| stdout.flush())
                    .is_err()
                {
                    return Ok(());
                }
            }
            last = line;
        }
        std::thread::sleep(WATCH_INTERVAL);
    }
}

/// One waybar custom-module line: workspace dots and the layout as text, the
/// focused window in the tooltip, and the layout as CSS class.
fn waybar_line(report: &veyron_ipc::StateReport) -> String {
    let dots: Vec<&str> = (0..report.workspace_count)
        .map(|index| {
            if index == report.workspace_index {
                "●"
            } else {
                "○"
            }
        })
        .collect();
    let focused = report
        .focused
        .and_then(|id| report.windows.iter().find(|window| window.id == id));
    let tooltip = match focused {
        Some(window) if !window.title.is_empty() => {
            format!("{} — {}", window.app_id, window.title)
        }
        Some(window) => window.app_id.clone(),
        None => format!("workspace {}", report.workspace),
    };
    serde_json::json!({
        "text": format!("{}  {}", dots.join(" "), report.layout),
        "alt": report.workspace,
        "tooltip": tooltip,
        "class": report.layout,
    })
    .to_string()
}
