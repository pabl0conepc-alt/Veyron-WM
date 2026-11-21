//! VEYRON WM control protocol.
//!
//! A small, versioned, line-delimited JSON protocol over a Unix socket. It is
//! intentionally boring: no framing tricks, no partial writes left to the
//! caller, a hard line-size limit, and one request/response pair per message.
//! That makes `veyronctl`, status bars and scripts trivial to write in any
//! language while keeping the attack surface small.
//!
//! ```text
//! -> {"request":"action","action":"focus left"}
//! <- {"response":"ok"}
//! ```

#![forbid(unsafe_code)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Bumped whenever a message shape changes incompatibly.
pub const PROTOCOL_VERSION: u32 = 3;

/// Hard limit for a single message, newline included.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Socket read/write timeout used by clients and by the server.
pub const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// A request sent to the compositor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum Request {
    /// Run an action in the exact grammar used by keybindings,
    /// e.g. `focus left`, `workspace 2`, `spawn foot`.
    Action { action: String },
    /// Ask for a description of the current layout.
    State,
    /// Ask for every workspace laid out on the sparse grid.
    Overview,
    /// Capture the current window-manager session as validated TOML.
    CaptureSession,
    /// Restore a previously captured session from TOML.
    RestoreSession { toml: String },
    /// Launch an installed XDG application by name.
    Launch { query: String },
    /// Re-read the configuration file.
    Reload,
    /// Ask the compositor to exit.
    Quit,
    /// Liveness check.
    Ping,
}

/// A response from the compositor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "kebab-case")]
pub enum Response {
    /// The request succeeded and has no payload.
    Ok,
    /// Reply to [`Request::Ping`].
    Pong,
    /// Reply to [`Request::State`].
    State(StateReport),
    /// Reply to [`Request::Overview`].
    Overview(OverviewReport),
    /// Reply to [`Request::CaptureSession`].
    Session { toml: String },
    /// Reply to [`Request::Launch`]: the application that was started.
    Launched { name: String, command: String },
    /// The request was rejected. The message is meant for humans.
    Error { message: String },
}

/// Current layout, suitable for a status bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateReport {
    /// Active workspace name.
    pub workspace: String,
    /// Zero-based index of the active workspace.
    pub workspace_index: usize,
    /// Number of workspaces on the active output.
    pub workspace_count: usize,
    /// Signed coordinates of the active sparse 2D workspace.
    pub workspace_position: [i64; 2],
    /// Resolved tiling layout of the active workspace (`scroll`,
    /// `master-stack`, `dwindle`).
    pub layout: String,
    /// Effective inner gap in logical pixels after smart gaps.
    pub gap: i32,
    /// Windows on the active workspace, in draw order.
    pub windows: Vec<WindowReport>,
    /// Focused window id, if any.
    pub focused: Option<u64>,
    /// Current horizontal scroll offset in logical pixels.
    pub scroll: f64,
}

/// One window in a [`StateReport`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowReport {
    /// Window id (same ids the compositor logs and the layout uses).
    pub id: u64,
    /// `[x, y, width, height]` in logical pixels.
    pub rect: [i32; 4],
    /// Whether the window floats above the tiling layout.
    pub floating: bool,
    /// Whether the window is fullscreen.
    pub fullscreen: bool,
    /// App id the client announced (`""` until it does).
    pub app_id: String,
    /// Title the client announced (`""` until it does).
    pub title: String,
    /// Opacity requested by the window rules (`1.0` when none match).
    pub opacity: f32,
}

/// Every workspace laid out on the sparse grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewReport {
    /// `[x, y, width, height]` covering every tile.
    pub bounds: [i32; 4],
    /// Output area the window rectangles are scaled from.
    pub viewport: [i32; 4],
    pub tiles: Vec<OverviewTileReport>,
}

/// One workspace tile of an [`OverviewReport`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewTileReport {
    pub workspace: String,
    pub index: usize,
    /// Sparse grid coordinate of the workspace.
    pub position: [i64; 2],
    pub active: bool,
    pub rect: [i32; 4],
    pub windows: Vec<OverviewWindowReport>,
}

/// A window drawn inside an overview tile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewWindowReport {
    pub id: u64,
    pub rect: [i32; 4],
    pub floating: bool,
}

/// IPC failure.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Socket or filesystem error.
    #[error("ipc i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The peer sent something that is not a valid message.
    #[error("invalid ipc message: {0}")]
    Json(#[from] serde_json::Error),
    /// The message exceeded [`MAX_LINE_BYTES`].
    #[error("ipc message exceeds the {MAX_LINE_BYTES} byte limit")]
    MessageTooLarge,
    /// The connection ended before the response arrived.
    #[error("ipc connection closed before a response arrived")]
    ConnectionClosed,
    /// Another compositor is already listening on the path.
    #[error("another compositor is already listening on {0}")]
    AlreadyRunning(PathBuf),
    /// The path exists but is not a socket; refusing to delete it.
    #[error("refusing to replace {0}: it exists and is not a socket")]
    UnsafeSocketPath(PathBuf),
    /// The peer sent bytes that are not valid UTF-8.
    #[error("ipc message is not valid UTF-8")]
    InvalidUtf8,
}

/// Default control socket: `$VEYRON_SOCKET`, else `$XDG_RUNTIME_DIR/veyron.sock`.
pub fn default_socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("VEYRON_SOCKET").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(dir).join("veyron.sock");
    }
    std::env::temp_dir().join("veyron.sock")
}

/// Write one message as a single newline-terminated JSON line.
pub fn write_message<W: Write, T: Serialize>(writer: &mut W, message: &T) -> Result<(), IpcError> {
    let mut line = serde_json::to_string(message)?;
    if line.len() + 1 > MAX_LINE_BYTES {
        return Err(IpcError::MessageTooLarge);
    }
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()?;
    Ok(())
}

/// Read one message. `Ok(None)` means the peer closed the connection cleanly.
pub fn read_message<R: BufRead, T: DeserializeOwned>(
    reader: &mut R,
) -> Result<Option<T>, IpcError> {
    let mut raw = Vec::new();
    {
        // Bounded read: never buffer more than one message worth of bytes, even
        // if the peer never sends a newline.
        let mut bounded = reader.take(MAX_LINE_BYTES as u64 + 1);
        bounded.read_until(b'\n', &mut raw)?;
    }

    if raw.is_empty() {
        return Ok(None);
    }
    if raw.len() > MAX_LINE_BYTES || !raw.ends_with(b"\n") {
        return Err(IpcError::MessageTooLarge);
    }

    let line = std::str::from_utf8(&raw).map_err(|_| IpcError::InvalidUtf8)?;
    Ok(Some(serde_json::from_str(line.trim_end())?))
}

/// Bind the control socket, replacing a stale socket left by a crash but never
/// deleting a regular file.
pub fn bind_listener(path: &Path) -> Result<UnixListener, IpcError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    let listener = match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if UnixStream::connect(path).is_ok() {
                return Err(IpcError::AlreadyRunning(path.to_path_buf()));
            }
            let metadata = std::fs::symlink_metadata(path)?;
            if !metadata.file_type().is_socket() {
                return Err(IpcError::UnsafeSocketPath(path.to_path_buf()));
            }
            std::fs::remove_file(path)?;
            UnixListener::bind(path).map_err(IpcError::Io)
        }
        Err(error) => Err(IpcError::Io(error)),
    }?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Counts control requests that are in flight, so a compositor can let the last
/// reply reach its client before the process exits.
///
/// A request is in flight from the moment it is read until its response has been
/// written, which is exactly the window in which exiting would lose it.
#[derive(Clone, Debug, Default)]
pub struct Activity {
    busy: Arc<AtomicUsize>,
}

impl Activity {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wait until every in-flight request has been answered.
    ///
    /// Returns `false` if they did not finish within `timeout`.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let busy = self.busy.load(Ordering::Acquire);
            if busy == 0 {
                return true;
            }
            if Instant::now() >= deadline {
                tracing::warn!(busy, "control requests still in flight");
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn enter(&self) -> ActivityGuard {
        self.busy.fetch_add(1, Ordering::AcqRel);
        ActivityGuard(Arc::clone(&self.busy))
    }
}

/// Holds one in-flight slot; dropping it releases the slot.
struct ActivityGuard(Arc<AtomicUsize>);

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Serve requests until the process exits.
///
/// Runs blocking, one thread per connection, so callers normally put it on its
/// own thread. A handler must be quick: it is called on the connection thread.
/// Requests are tracked by `activity`, so a caller can wait for the last reply
/// to be written before it exits.
pub fn serve<F>(path: &Path, activity: &Activity, handler: F) -> Result<(), IpcError>
where
    F: Fn(Request) -> Response + Send + Sync + 'static,
{
    let listener = bind_listener(path)?;
    let handler = Arc::new(handler);
    let activity = activity.clone();

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let handler = Arc::clone(&handler);
                let activity = activity.clone();
                std::thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, handler.as_ref(), &activity) {
                        tracing::warn!(%error, "ipc client disconnected with an error");
                    }
                });
            }
            Err(error) => {
                // A single failed accept must not take the control socket down.
                tracing::warn!(%error, "ipc accept failed");
            }
        }
    }
    Ok(())
}

fn handle_connection(
    mut stream: UnixStream,
    handler: &(dyn Fn(Request) -> Response + Send + Sync),
    activity: &Activity,
) -> Result<(), IpcError> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        match read_message::<_, Request>(&mut reader) {
            Ok(Some(request)) => {
                // In flight until the response is on the wire.
                let _in_flight = activity.enter();
                let response = handler(request);
                write_message(&mut stream, &response)?;
            }
            Ok(None) => return Ok(()),
            Err(error) => {
                // Tell the client what went wrong, then drop the connection.
                let response = Response::Error {
                    message: error.to_string(),
                };
                let _ = write_message(&mut stream, &response);
                return Err(error);
            }
        }
    }
}

/// Connect to a compositor control socket.
pub fn connect(path: &Path) -> Result<UnixStream, IpcError> {
    let stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    Ok(stream)
}

/// Send one request and wait for its response.
pub fn request(path: &Path, request: &Request) -> Result<Response, IpcError> {
    let mut stream = connect(path)?;
    write_message(&mut stream, request)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    read_message::<_, Response>(&mut reader)?.ok_or(IpcError::ConnectionClosed)
}

/// [`request`] against [`default_socket_path`].
pub fn request_default(message: &Request) -> Result<Response, IpcError> {
    request(&default_socket_path(), message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn requests_round_trip_over_the_wire_format() {
        let requests = [
            Request::Ping,
            Request::State,
            Request::Overview,
            Request::CaptureSession,
            Request::RestoreSession {
                toml: "workspaces = []\nwindows = []\n".into(),
            },
            Request::Launch {
                query: "editor".into(),
            },
            Request::Quit,
            Request::Action {
                action: "focus left".into(),
            },
        ];
        for request in requests {
            let mut buffer = Vec::new();
            write_message(&mut buffer, &request).expect("writes");
            assert!(buffer.ends_with(b"\n"));
            let mut reader = Cursor::new(buffer);
            let decoded: Option<Request> = read_message(&mut reader).expect("reads");
            assert_eq!(decoded, Some(request));
        }
    }

    #[test]
    fn responses_round_trip_over_the_wire_format() {
        let response = Response::State(StateReport {
            workspace: "2".into(),
            workspace_index: 1,
            workspace_count: 3,
            workspace_position: [-1, 2],
            layout: "master-stack".into(),
            gap: 8,
            windows: vec![WindowReport {
                id: 7,
                rect: [10, 20, 30, 40],
                floating: false,
                fullscreen: true,
                app_id: "foot".into(),
                title: "Terminal".into(),
                opacity: 0.8,
            }],
            focused: Some(7),
            scroll: 12.5,
        });

        let mut buffer = Vec::new();
        write_message(&mut buffer, &response).expect("writes");
        let mut reader = Cursor::new(buffer);
        let decoded: Option<Response> = read_message(&mut reader).expect("reads");
        assert_eq!(decoded, Some(response));
    }

    #[test]
    fn overview_responses_round_trip_over_the_wire_format() {
        let response = Response::Overview(OverviewReport {
            bounds: [0, 0, 410, 250],
            viewport: [0, 0, 1280, 800],
            tiles: vec![OverviewTileReport {
                workspace: "1".into(),
                index: 0,
                position: [0, 0],
                active: true,
                rect: [0, 0, 200, 120],
                windows: vec![OverviewWindowReport {
                    id: 7,
                    rect: [4, 6, 120, 60],
                    floating: false,
                }],
            }],
        });

        let mut buffer = Vec::new();
        write_message(&mut buffer, &response).expect("writes");
        let mut reader = Cursor::new(buffer);
        let decoded: Option<Response> = read_message(&mut reader).expect("reads");
        assert_eq!(decoded, Some(response));
    }

    #[test]
    fn closing_a_connection_yields_none() {
        let mut reader = Cursor::new(Vec::new());
        let decoded: Option<Request> = read_message(&mut reader).expect("reads");
        assert_eq!(decoded, None);
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        let mut reader = Cursor::new(b"{\"request\":\"unknown\"}\n".to_vec());
        assert!(read_message::<_, Request>(&mut reader).is_err());
    }

    #[test]
    fn oversized_messages_are_rejected_on_both_sides() {
        let mut reader = Cursor::new(vec![b'a'; MAX_LINE_BYTES + 8]);
        assert!(matches!(
            read_message::<_, Request>(&mut reader),
            Err(IpcError::MessageTooLarge)
        ));

        let big = Request::Action {
            action: "x".repeat(MAX_LINE_BYTES),
        };
        let mut buffer = Vec::new();
        assert!(matches!(
            write_message(&mut buffer, &big),
            Err(IpcError::MessageTooLarge)
        ));
        assert!(
            buffer.is_empty(),
            "nothing is written for a rejected message"
        );
    }

    #[test]
    fn server_and_client_exchange_requests() {
        let path = std::env::temp_dir().join(format!(
            "veyron-ipc-test-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));

        let server_path = path.clone();
        std::thread::spawn(move || {
            let _ = serve(&server_path, &Activity::new(), |request| match request {
                Request::Ping => Response::Pong,
                Request::Action { action } if action == "fail" => Response::Error {
                    message: "not now".into(),
                },
                Request::Action { action: _ } => Response::Ok,
                _ => Response::Ok,
            });
        });

        // Wait for the listener to come up.
        for _ in 0..100 {
            if UnixStream::connect(&path).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(matches!(request(&path, &Request::Ping), Ok(Response::Pong)));
        assert!(matches!(
            request(
                &path,
                &Request::Action {
                    action: "focus left".into()
                }
            ),
            Ok(Response::Ok)
        ));
        assert!(matches!(
            request(
                &path,
                &Request::Action {
                    action: "fail".into()
                }
            ),
            Ok(Response::Error { .. })
        ));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn refuses_to_replace_a_regular_file() {
        let path = std::env::temp_dir().join(format!(
            "veyron-ipc-file-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::write(&path, b"do not delete me").expect("write file");

        assert!(matches!(
            bind_listener(&path),
            Err(IpcError::UnsafeSocketPath(_))
        ));
        assert_eq!(
            std::fs::read(&path).expect("file still exists"),
            b"do not delete me"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn control_socket_is_private_to_the_current_user() {
        let path = std::env::temp_dir().join(format!(
            "veyron-ipc-mode-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));

        let listener = bind_listener(&path).expect("binds a private socket");
        let mode = std::fs::metadata(&path)
            .expect("socket metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);

        drop(listener);
        let _ = std::fs::remove_file(&path);
    }
}
