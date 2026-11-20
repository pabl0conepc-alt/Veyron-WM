//! `veyronctl` — control client for VEYRON WM.

use std::{error::Error, path::PathBuf, process::ExitCode};

use veyron_ipc::{Request, Response};

const HELP: &str = "\
veyronctl — control a running VEYRON WM

Usage:
  veyronctl state [--json]        Show the current layout
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
    let mut words: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-s" | "--socket" => {
                let path = args.next().ok_or("--socket needs a path")?;
                socket = PathBuf::from(path);
            }
            "--json" => json = true,
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

    let request = match words[0].as_str() {
        "state" | "status" if words.len() == 1 => Request::State,
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
