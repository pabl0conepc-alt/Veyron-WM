# VEYRON WM — Architecture

This document records the design of Veyron as implemented, the invariants the
code relies on, and the reasoning behind the choices. It is meant to be updated
with the code: an undocumented invariant is a future bug.

## 1. Layering

```
┌──────────────────────────────────────────────────────────────┐
│ crates/veyron (Smithay, Wayland, winit, calloop)             │
│   protocol handlers · input · scene graph · rendering · IPC   │
└───────────────┬──────────────────────────────────────────────┘
                │ WindowId, NewWindow, Action, Snapshot
┌───────────────▼──────────────────────────────────────────────┐
│ crates/veyron-core (no I/O, no protocols, no GPU)            │
│   geometry · layouts · motion · rules · workspaces · config   │
└──────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│ crates/veyron-ipc (serde, Unix sockets)                      │
│   request/response protocol · framing · server/client helpers │
└──────────────────────────────────────────────────────────────┘
```

The two directions of the core boundary are deliberate:

- **In:** the compositor reports windows appearing/disappearing and forwards
  `Action`s. It never manipulates layout state directly.
- **Out:** the core returns a `Snapshot` — the full geometry of the current
  workspace — plus `Effects` describing what the compositor still has to do
  (ask a client to close, spawn a command, reload, quit).

Nothing in `veyron-core` knows what a Wayland surface is. That is what makes the
window-management model verifiable in milliseconds with `cargo test` instead of a
GPU session, and it is why the tests can pin down behaviours such as "the focused
column is always fully visible".

## 2. The scrollable tiling model

A workspace is an **unbounded horizontal strip of columns**. Each column holds
windows stacked vertically. Outputs contain a sparse, lazily-created grid of
these workspaces at signed `(x, y)` coordinates. Moving across the grid keeps
each workspace's layout and focus state; moving a window transfers it to a
neighbor and activates that destination.

Why this model:

- It gives scrollable tiling (endless left/right movement) and traditional
  vertical stacking without two separate layout modes competing for the same
  state.
- Geometry is a pure function of `(strip, focus, scroll, viewport)`. There is no
  per-window position cache that can go stale.
- Columns are measured in **fractions of the viewport**, so resizing the output
  reflows everything consistently instead of producing a mix of fixed and
  relative widths.

Policies:

- A new window opens as a new column immediately right of the focused column.
  Stacking is always an explicit user action (`move left/right` moves the
  focused window into the neighbouring column). Predictability wins over
  guessing.
- Fullscreen covers the viewport. The tiling strip is untouched, so leaving
  fullscreen restores the exact previous layout.
- Floating windows are a separate list drawn above the strip. Toggling float off
  re-inserts the window as a column at the current focus, and toggling it on
  starts from the window's last tiled geometry, so the operation is reversible.

### Invariants (enforced by the core, asserted by tests)

1. No column is ever empty; an empty column is removed immediately.
2. `focus`, when present, always addresses an existing window.
3. `scroll` is always within `0..=max_scroll()`.
4. The focused column is fully inside the viewport whenever it fits, and is
   left-aligned when it is wider than the viewport.
5. A window id is never reused, so a stale id is detected rather than silently
   addressing another window.
6. Every placement has positive width and height and lies within the viewport's
   vertical extent.
7. Each workspace coordinate maps to exactly one workspace on its output;
   navigating to an unvisited coordinate creates it once.

### Layout modes

`LayoutKind` selects the tiling algorithm of a workspace:

- `scroll` (default) keeps the strip and its scroll offset.
- `master-stack` gives the first window `master_ratio` of the width and stacks
  the rest in the remaining column.
- `dwindle` splits the usable area recursively, alternating axes.
- `auto` resolves to `master-stack` for up to three windows and to `dwindle`
  from four windows on (dynamic tiling).

The strip still owns window order, focus and column ratios for every layout, so
there is one source of truth. In flat layouts `left/up` and `right/down` step
through that order, and moving a window swaps it with its neighbour in it.
Scroll-specific actions (column width, centering) are ignored outside `scroll`.

Smart gaps apply per workspace from the number of tiled windows: zero for a
lone window, the configured gap for two or three, and two thirds of it when the
layout is dense. The effective gap is part of the snapshot, so the geometry the
compositor draws and the geometry the user configured cannot disagree.

### Rules, scratchpad and memory

`config.rules` is an ordered `RuleSet`. A rule matches `app_id` and/or `title`
with a small glob language (`*`, `?`, no regex dependency) and contributes
floating, opacity, blur, scratchpad, named workspace and layout policy. Later
rules override earlier ones per field. The policy is applied when the window is
created, before it is inserted into a workspace, so a rule can never briefly
show a window in the wrong place.

Windows that live in the scratchpad are kept in `Wm::scratchpad` and detached
from every workspace. Toggling the scratchpad shows them as cascading floating
windows on the active workspace, or detaches them again. `behavior.remember_geometry`
stores the last floating rectangle per app id and restores it on the next window
of that application.

### Motion

`motion.rs` provides a spring, easing curves and a rectangle animator that take
`dt` from the caller. The compositor does not yet own a frame clock for
animations, so the module is wired into configuration and validated but not into
rendering; see [ROADMAP](../ROADMAP.md) Phase 7.

### Snapping, groups and sessions

`snap.rs` owns the geometry of screen zones and picture-in-picture corners. Both
only ever reposition a floating window, so they cannot violate the strip
invariants, and every result is clamped into the viewport.

`groups.rs` tracks group membership and order only. The strip stays the single
source of placement order: grouping a window triggers
`ScrollLayout::reorder`, which rebuilds the strip with the group's members next
to each other while preserving column ratios. Groups hold ids, never geometry,
and dead windows are pruned on removal.

`session.rs` defines the serialisable session (workspace names plus one record
per window) used by Veyron Recall. Capture reads only public window-manager
state; restore goes through `Wm::create_window_in` and the same floating-layer
helpers as the interactive actions, so both paths share their invariants.

Smart focus lives in `wm.rs` as a bounded most-recently-used history. Every
focus change is recorded once, which makes `focus previous` a simple scan of the
history for the last window that is still alive.

### Geometry math

With viewport width `W`, column ratio `r`, gap `g`:

- slot width `outer = round(r * W)`
- content width `= max(1, outer - g)`
- column `i` starts at `sum(outer_j for j < i)`
- total content width `= sum(outer) - g` (the trailing gap is trimmed)
- `max_scroll = max(0, content - W)`

Rows in a column split `viewport height - g * (rows - 1)` evenly; the remainder
goes to the last row so the column always ends exactly at the viewport bottom.

## 3. State machine and effects

`Wm::dispatch(Action) -> Effects` is the only mutation entry point. `Effects`
carries what the compositor cannot derive from the snapshot alone:

| Effect | Compositor reaction |
| --- | --- |
| `close_requested` | `xdg_toplevel.send_close()`; the window is removed when the client destroys its surface |
| `spawn` | run `sh -c <command>` |
| `reload_config` | re-read the TOML file and apply it |
| `quit` | leave the event loop |
| `layout_changed` / `focus_changed` / `workspace_changed` | resync scene graph, focus, render |

Removal is always driven by surface destruction, not by the close action. A
client that ignores `close` therefore cannot desynchronise the manager from the
protocol.

## 4. Protocol layer

Built on Smithay 0.7 with the `backend_winit`, `desktop` and `wayland_frontend`
features.

- **Backend:** nested `winit` output; window resize events are forwarded to
  `Wm::set_output_size`, so the whole strip reflows.
- **Scene graph:** Smithay's `Space<Window>` handles clipping to the output,
  z-order and popup rendering. Veyron sets positions from its own snapshot; the
  space never decides geometry. Windows are only remapped when their position
  actually changes, which avoids reordering the stack on every frame.
- **Configures:** tiled windows are sent a `xdg_toplevel.configure` only when
  their target size changes (`configured` map). The scene graph and the client
  therefore converge without configure storms.
- **Focus:** keyboard focus follows the snapshot's focused window; activation
  state is applied to every managed window. Pointer buttons focus the window
  under the cursor and raise floating windows.
- **Input:** key events go through the XKB keyboard handle. The filter looks up
  the exact modifier set and keysym name in the configuration; unbound keys are
  forwarded to the focused client. Presses only trigger actions, releases are
  always forwarded.
- **Rendering:** GLES renderer with Smithay's `OutputDamageTracker`; buffer age
  is used when the Winit EGL surface exposes it and age `0` safely falls back to
  a full redraw. The output damage is also submitted to EGL for swap damage.
- **Event loop:** `calloop` watches the winit loop, Wayland display fd and IPC
  channel. Nested mode blocks on readiness sources; headless mode retains a
  bounded 50 ms wait to retry clients whose socket buffers were full.
- **Spatial indicator:** nested window decorations show the active signed
  workspace coordinate; the same coordinate is part of the version 2 IPC state
  report and `veyronctl state` output.

## 5. Configuration

TOML, parsed into `RawConfig` and validated into `Config`:

- Unknown keys are errors (`deny_unknown_fields`) — a typo must not silently
  disable a binding.
- Ratios must be in `(0, 1]` with `min <= default <= max`; gaps and margins are
  non-negative; colours accept `#rgb`, `#rrggbb`, `#rrggbbaa`.
- Chords are parsed with exactly one grammar (`super`, `ctrl`, `alt`, `shift`,
  last token is the key) and duplicates are rejected.
- A missing file means defaults; an existing but invalid file is an error.
  Silently ignoring a broken configuration is a UX and security hazard.

Reloading applies layout and keybindings without restarting the compositor; new
geometry is pushed to clients on the next sync.

## 6. Control protocol

`veyron-ipc` is deliberately small:

- one JSON object per line, hard limit 64 KiB on both directions;
- one request → exactly one response;
- socket permissions are restricted to owner read/write (`0600`) after bind;
- the server removes only a stale **socket** (never a regular file), refuses to
  start if another compositor answers on the path;
- read/write timeouts so a stuck client cannot pin a thread forever.

Because actions are text in the same grammar as keybindings, there is a single
parser (`Action::parse`) and no second source of truth for "what can Veyron do".

Threading: IPC connections are handled on their own threads; requests are
forwarded to the compositor loop through a `calloop` channel and replies return
over a oneshot channel with a 2 s timeout. Window-management state is therefore
only ever touched on the compositor thread.

Both run modes share the same `calloop` loop. Nested mode waits on winit, the
Wayland display poll fd and IPC readiness; headless mode waits on the display
fd and control channel with a bounded timeout to retry blocked client flushes.

### XWayland boundary

XWayland remains unsupported. Smithay 0.7 provides process and XWM helpers, but
the compositor still needs an X11 window-manager implementation and an adapter
for X11 window map/configure/focus/destroy events. See
[ADR 0002](decisions/0002-xwayland-integration-boundary.md); enabling a Cargo
feature alone must not be presented as application compatibility.

## 7. Testing strategy

- `veyron-core` unit tests cover geometry, the action grammar, configuration
  validation, layout invariants, workspace isolation, floating/fullscreen
  round-trips and zero-sized outputs.
- `veyron-ipc` tests cover wire round-trips, oversized and malformed messages,
  stale/unsafe socket handling, and a real socket request/response exchange.
- `crates/veyron/tests/headless.rs` runs the real `veyron --headless` binary
  (no display, GPU or DRM) and connects to it with `wayland-client` over the
  socket it publishes. Each test is a genuine client: it binds
  `wl_compositor`/`xdg_wm_base`/`wl_shm`/`wl_seat`/`wl_output`, creates
  xdg-toplevels with SHM buffers, commits and acks configures, then asserts on
  Veyron's own state through the control protocol (`Request::State`) and on what
  the client observed (configure size, activation states, viewport scrolling).
  The suite covers mapping, layout-driven configure size and re-configure,
  focus/activation following the model, horizontal scroll offsets, 2D workspace
  navigation and window transfer, unmap on destroy and clean shutdown.
- These tests are mutation-checked: disabling the configure send in
  `sync_layout`, the activation configure in `sync_focus`, or window removal in
  `toplevel_destroyed` each makes the suite fail. Development of the harness
  itself already caught a real defect — activation was staged in the xdg state
  but never sent as a configure, so clients never saw it.

## 8. Roadmap

Ordered by the project's priority hierarchy (correctness → security →
performance → architecture → compatibility → features → effects).

1. **Interactive validation**: visually verify the nested compositor, damage
   behavior and spatial indicator in the target session; measure the static
   renderer on target hardware.
2. **Window interactions**: real-client nested move/resize, popup grabs,
   floating geometry and drag-and-drop wiring.
3. **Compatibility and shell boundary**: investigate XWayland and portal
   integration; define a versioned external shell interface and test toolkit
   behavior before claiming compatibility.
4. **Stabilization**: security/dependency audit, measured performance and a
   reproducible installation path.
5. **Optional follow-up**: disableable animations only after the static renderer
   is validated; DRM/KMS, multi-output and DPMS after nested behavior and
   security boundaries are stable.

## 9. Security notes

- No `unsafe` code in any crate.
- Untrusted input boundaries: configuration files (strict validation), IPC
  (size limits, UTF-8 checks, no path deletion unless it is a socket), Wayland
  clients (all surface state owned by Smithay; Veyron never indexes client data
  directly).
- Spawned commands come from the user's own configuration or control socket; the
  socket lives in `$XDG_RUNTIME_DIR` and is only as private as that directory.
