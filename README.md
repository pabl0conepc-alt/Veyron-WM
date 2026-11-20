# VEYRON WM

A scrollable-tiling Wayland compositor written entirely in Rust, built on
[Smithay](https://github.com/Smithay/smithay).

Veyron is a workspace of **columns on an endless horizontal strip**: every window
opens as a full-height column, focus scrolls the strip, and stacking windows into
a column is an explicit action. The goal is a compositor where the window
management model is small enough to reason about, test exhaustively, and grow
into a serious daily driver.

```
workspace strip ────────────────────────────────────────────────►
   ┌──────────┐ ┌──────────┐ ┌──────────┐
   │ window 1 │ │ window 2 │ │ window 3 │      ← focus scrolls here
   └──────────┘ │──────────│ └──────────┘
                │ window 4 │
                └──────────┘
```

## Status

The headless protocol path and window-management core are tested, and a nested
GLES path is implemented. Visual presentation in the target session is still
unverified. Veyron is **not** a finished desktop yet.

See [the phase roadmap](ROADMAP.md) for verified behavior and remaining work.

What works:

- Rust workspace with three crates and no `unsafe` code.
- Scrollable tiling engine: columns, stacking, focus, viewport scrolling,
  column resizing, floating layer, fullscreen, on-demand workspaces.
- Deterministic, unit-tested state machine (43 core tests) with no GPU or
  display server required.
- TOML configuration with strict validation and a documented default keymap.
- Versioned line-delimited JSON control protocol on a Unix socket
  (`veyronctl`) with request/response, size limits and stale-socket handling.
- Nested (winit) Wayland compositor: `xdg-shell` toplevels, SHM buffers,
  keyboard, pointer, wheel, `wl_output`, popups tracked, tiled layout rendered
  through Smithay's scene graph.
- Headless runtime (`--headless`) that needs no display, GPU or DRM device, and
  end-to-end tests that drive it with a real `wayland-client` connection and
  assert on Veyron's own state: toplevel mapping, configure size against the
  tiling layout, focus and activation, workspace scrolling, and unmap on
  destroy.

Known gaps, deliberately out of this milestone (see `docs/ARCHITECTURE.md`):

- No DRM/KMS backend, session management or multi-output hotplug; Veyron
  currently runs nested inside an existing session.
- No client-side decorations drawn by the compositor, no cursor image, no
  animated transitions.
- Popup grabs, layer-shell (bars/panels), screencopy, idle and input-method
  protocols are not implemented.
- Window titles/app ids are not exposed to the core yet, so `veyronctl state`
  identifies windows by id and geometry.

## Building

Requirements: Rust 1.87+, a C toolchain, `pkg-config`, and the usual EGL/Mesa
development packages. The nested backend needs a running Wayland or X11 session.

```sh
cargo build --release          # builds veyron and veyronctl
cargo test                     # core, IPC and headless protocol suites
```

## Running

Veyron runs **nested**: it opens a window in your current session and serves a
new Wayland socket to its children.

```sh
# 1. start the compositor
target/release/veyron

# Veyron prints the socket it created; in another terminal:
export WAYLAND_DISPLAY=wayland-1     # use the name printed by veyron
target/release/veyronctl state

# 2. launch a client (any toolkit works)
alacritty
```

Veyron sets `WAYLAND_DISPLAY` for processes it spawns through actions, so
`super+return` starts the terminal from your configuration inside the nested
session.

### Headless

`--headless` runs the same `Display`, protocol handlers and window manager
without a window, GPU or DRM device. It is meant for tests, CI and debugging:
clients connect to a normal Wayland socket and everything except rendering
works.

```sh
target/release/veyron --headless --output-size 1920x1080 \
    --wayland-display veyron-headless
# from another terminal, with XDG_RUNTIME_DIR set the same way:
WAYLAND_DISPLAY=veyron-headless alacritty
target/release/veyronctl quit
```

`--output-size` sets the logical output size the layout is computed for
(default `1920x1080`). `veyronctl quit` shuts it down cleanly and removes its
control and Wayland sockets.

Default keybindings (all configurable):

| Chord | Action |
| --- | --- |
| `super+return` | spawn `alacritty` |
| `super+q` | close the focused window |
| `super+left/right/up/down` | focus column / row |
| `super+shift+left/right/up/down` | move window into a column / swap rows |
| `super+minus` / `super+equal` | shrink / grow the focused column |
| `super+alt+arrow` | resize the focused floating window by 32 logical pixels |
| `super+alt+shift+arrow` | move the focused floating window by 32 logical pixels |
| `super+c` | center the focused column |
| `super+f` | toggle fullscreen |
| `super+space` | toggle floating |
| `super+1` … `super+5` | switch workspace |
| `super+bracketright` / `super+bracketleft` | next / previous workspace |
| `super+ctrl+arrow` | navigate the sparse 2D workspace grid |
| `super+ctrl+shift+arrow` | move the focused window to a neighboring workspace |
| `super+r` | reload the configuration |
| `super+shift+e` | quit |

## Configuration

`$XDG_CONFIG_HOME/veyron/veyron.toml` (or `~/.config/veyron/veyron.toml`).
Veyron starts with built-in defaults when the file does not exist; a file that
exists but is invalid is reported and refused rather than silently ignored.

```sh
target/release/veyron --print-config > ~/.config/veyron/veyron.toml
```

```toml
[layout]
gap = 8                      # pixels between columns and stacked windows
outer_margin = 12            # margin around the tiled area
default_column_ratio = 0.5   # new column width, fraction of the viewport
min_column_ratio = 0.2
max_column_ratio = 1.0

[appearance]
background = "#101014"

# Commands started once at launch:
startup = ["waybar"]

[bindings]
"super+return" = "spawn foot"
"super+left" = "focus left"
"super+shift+left" = "move left"
"super+f" = "toggle-fullscreen"
```

Binding values use the same action grammar as the control protocol, so anything
`veyronctl` can do can be bound to a key. If a `[bindings]` table is present it
replaces the default keymap entirely; omit it to keep the defaults.

## Control protocol

`veyronctl` speaks a versioned, newline-delimited JSON protocol over a Unix
socket (`$XDG_RUNTIME_DIR/veyron.sock` by default, `--socket` or
`$VEYRON_SOCKET` to override):

```sh
veyronctl state --json
veyronctl focus left
veyronctl workspace 2
veyronctl workspace up
veyronctl move-workspace right
veyronctl resize width 32
veyronctl move-floating right 32
veyronctl spawn "foot --class veyron"
veyronctl reload
veyronctl quit
```

Any other program can use the socket directly:

```json
{"request":"action","action":"focus left"}
{"response":"ok"}
```

Protocol version 2 includes the signed 2D coordinate in state responses.
Requests: `action`, `state`, `reload`, `quit`, `ping`. Messages are capped at
64 KiB; the socket is owner-only (`0600`), the server never deletes a non-socket
file, and a crashed compositor's stale socket is safely replaced. State reports
include the active workspace's signed `(x, y)` coordinate.

## Repository layout

```
crates/veyron-core/   layout, workspaces, focus, config, actions (no Wayland)
crates/veyron-ipc/    control protocol, framing, client and server helpers
crates/veyron/        Smithay compositor binary + veyronctl
docs/ARCHITECTURE.md  architecture, invariants, current protocol boundaries
docs/decisions/       architecture decision records
```

## Design rules

1. Correctness and stability before features; a visual effect never risks the
   session.
2. The core is protocol-free and testable without a compositor.
3. No `unsafe` code (`#![forbid(unsafe_code)]` on every crate).
4. Configuration is validated before use; unknown keys are errors.
5. Every dependency must earn its place; the core needs only `serde` and `toml`.

## License

MIT.
