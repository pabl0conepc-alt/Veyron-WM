# Veyron WM roadmap

Phase states follow the master development prompt. A passing build or a model
unit test alone does not verify a compositor feature; protocol and runtime
criteria are listed separately.

## Phase 0 — Environment and research — VERIFIED

- [x] Record the 4 GiB target machine, NixOS/Wayland session and Intel GPU PCI ID.
- [x] Compare Smithay, low-level Wayland bindings and wlroots as a reference.
- [x] Select a nested Smithay backend as the safe first runtime target.
- [x] Document the initial dependency policy, architecture and open risks.

Evidence: [Phase 0 research](docs/PHASE0_RESEARCH.md) and [ADR 0001](docs/decisions/0001-compositor-foundation.md).

## Phase 1 — Foundation — TESTING

- [x] Build a Rust workspace around Smithay 0.7.
- [x] Serve real Wayland clients in headless mode and run protocol-level tests
  that map, configure, focus, scroll and destroy real xdg-shell toplevels.
- [x] Compile, test and lint the workspace in GitHub Actions on the current
  `feat/headless-calloop-loop` base.
- [ ] Run the nested GLES compositor in the target Plasma Wayland session and
  visually verify a real client surface.

**Acceptance:** a reproducible build plus a working interactive nested session
with an application surface that can be closed. Headless protocol coverage is
strong integration evidence, but does not replace this visual runtime check.

## Phase 2 — Window management — TESTING

- [x] Implement protocol-independent scroll tiling, stacking, focus, floating,
  fullscreen, sizing and workspace state.
- [x] Drive layout configures, activation and removal from real xdg-shell
  surfaces in headless integration tests.
- [x] Provide keyboard actions and a versioned control socket.
- [x] Resize floating windows through the same keybinding/IPC action grammar,
  respecting client minimum size and the current viewport.
- [x] Move floating windows through the same keybinding/IPC action grammar,
  preserving size and keeping the full rectangle within the viewport.
- [ ] Exercise user-driven move/resize, popup interactions and floating geometry
  against real clients in a nested session.

**Acceptance:** multiple application windows can be opened, arranged, focused,
moved, resized and closed without losing client state.

## Phase 3 — Infinite Desktop — TESTING

- [x] Allocate workspaces on demand and preserve their independent window state.
- [x] Navigate indefinitely through a horizontal strip; focus scrolls the strip.
- [x] Verify workspace switching and scroll offsets through real headless
  Wayland clients.
- [x] Add sparse signed two-dimensional coordinates and four-way navigation.
- [x] Move windows between spatial workspaces while preserving identity, focus,
  layout mode and floating geometry; expose coordinates through IPC.
- [x] Add default key bindings, CLI state coordinates and a visible nested
  window-title location indicator.
- [ ] Validate spatial navigation in a nested session. Gesture navigation and
  multi-output support remain later work.

**Acceptance:** horizontal and vertical spatial navigation works dynamically,
keeps windows and focus state, and visibly identifies the active region.

## Phase 4 — Rendering and animations — IMPLEMENTING

- [x] Render nested GLES surfaces through Smithay's scene graph and only redraw
  when compositor state is marked dirty.
- [x] Wake the nested event loop from Wayland, IPC and window events instead of
  periodic idle polling.
- [x] Use Smithay output damage tracking and the backend's buffer-age support
  when available; fall back to a full redraw when it is unavailable.
- [ ] Verify partial damage and idle wakeups in a nested runtime on the target
  graphics session.
- [ ] Measure idle and client-load CPU, RSS and frame behavior before setting
  targets.
- [ ] Add optional, independently configurable animations only after the
  damage-aware static renderer is validated; all effects must be disableable.

**Acceptance:** stable static rendering with correct damage, demand-driven frame
scheduling, and measured behavior. Blur and transparency remain optional.

## Phase 5 — Configuration and ecosystem — TESTING

- [x] Parse strict TOML configuration with defaults, type validation and clear
  errors; invalid reloads keep the last valid configuration.
- [x] Provide a bounded, versioned Unix socket protocol and `veyronctl`.
- [x] Expose actions, state, reload, ping and graceful shutdown over IPC.
- [x] Expose spatial workspace coordinates and window-transfer actions through
  the core action parser, default bindings and state IPC.
- [x] Research Smithay's XWayland/XWM boundary and record an ADR.
- [ ] Integrate the XWayland process and X11 window manager with compatibility
  tests and a real nested X11-client smoke test.
- [ ] Define a portal integration boundary and document which desktop portal
  services are external to the compositor.
- [ ] Document an isolated external-shell API and compatibility guarantees.
- [ ] Keep in-process dynamic plugins deferred until a concrete safe permission
  and isolation model is justified.

**Acceptance:** users can configure and control verified behavior externally;
compatibility claims are backed by per-feature tests and documented limits.

## Phase 6 — Stabilization — IMPLEMENTING

- [x] Run CI formatting, Clippy with warnings denied, build and workspace tests.
- [x] Add unit, IPC and headless protocol integration coverage.
- [x] Restrict the control socket to owner-only read/write permissions and test
  the resulting mode.
- [ ] Add nested compositor smoke coverage where a usable display runner exists.
- [ ] Record repeatable idle/load RSS, CPU, frame timing, layout cost and startup
  measurements on the target hardware. No numbers are claimed before measurement.
- [ ] Review input, IPC, process spawning, socket ownership, client handling and
  shutdown paths; add dependency/license auditing.
- [ ] Document tested protocol/toolkit compatibility and unsupported features.
- [ ] Validate the locked Nix flake package in GitHub Actions (definition and
  package job added; first package build pending).
- [ ] Establish a reproducible user installation/package path after runtime
  validation.

**Acceptance:** all relevant suites pass, runtime and performance evidence is
reproducible, and security/compatibility limits are explicit.

## Phase 7 — Window management depth — IMPLEMENTING

Competitive window management features implemented as protocol-free policy in
`veyron-core`, so they are verifiable without a GPU session.

- [x] Master-stack and dwindle layouts plus `auto` dynamic tiling, selectable
  per workspace with `layout <name>` / `layout next` (bindings and IPC).
- [x] Smart gaps: no gap for a lone tiled window, tighter gaps on dense layouts.
- [x] Per-application window rules (`app_id`/`title` globs) for floating,
  opacity, blur, scratchpad, named workspace routing and layout.
- [x] Scratchpad layer: park the focused window, recall it as floating, hide it
  again; rules can start an application parked.
- [x] Per-app floating geometry memory (`behavior.remember_geometry`).
- [x] Animation core: critically tunable spring, easing curves, rectangle
  animator, duration and reduced-motion policy, all frame-driver agnostic.
- [x] Per-window transition set (`Transitions`): follows a frame of placements,
  redirects running transitions from their in-flight geometry, drops windows
  that stop being drawn and reports when another frame is needed.
- [x] Layout, gap and app-routing state exposed on the control protocol
  (version 3) and in `veyronctl state`.
- [x] Veyron Snap: floating-window zones (halves, quadrants, thirds, center,
  maximize) with `snap <zone>`; geometry is clamped into the viewport.
- [x] Picture-in-picture: `pip [corner]` docks the focused window to a 16:9
  corner overlay and restores its previous geometry when toggled off.
- [x] Window groups: `group` joins the focused window with the previously
  focused one, members are kept adjacent in the strip, `group next`/`prev`
  cycle inside the group and closed windows are pruned.
- [x] Smart focus: a bounded most-recently-used history behind `focus previous`.
- [x] Veyron Recall: session capture/restore of workspaces, windows and floating
  geometry as validated TOML (`Session`), with a bounded window count.
- [ ] Drive `Transitions` from the nested render loop (replace placement
  rectangles with in-flight ones and request a frame while it reports work);
  needs nested runtime validation on the target GPU.
- [ ] Apply per-window opacity/blur in the renderer (policy is already in the
  snapshot; no shader or scene-graph mutation is implemented).
- [ ] Rounded corners, shadows, real GPU blur, workspace-transition effects and
  per-window animation overrides.
- [ ] Veyron Studio (graphical configuration), native panel/launcher/OSD/
  notifications, plugin isolation, gesture input, HDR/VRR/fractional scaling,
  screencopy/screen sharing, XWayland and multi-GPU work.
- [x] Multi-output policy: outputs are added and removed at runtime with their
  own workspaces and viewports; `output <direction>` follows monitor adjacency,
  `move-output <direction>` moves the focused window and
  `workspace-output <direction>` moves the whole workspace, with floating
  geometry re-clamped to the destination and windows migrated on unplug.
- [x] Veyron Profiles: named layout/motion preset patches with
  `default_profile`, a `profile <name>` action and strict validation, so a preset
  can never leave the layout configuration invalid.
- [x] Workspace overview policy: a paged grid of an output's workspaces ordered
  by sparse coordinate, with window counts, hit-testing and page navigation for
  an exposé-style view.
- [ ] Draw the overview in the nested renderer (thumbnails of live surfaces) and
  add the binding/action that opens and closes it.
- [ ] Backend wiring for multiple outputs: the nested backend still creates one
  output, so per-monitor rendering and hotplug need compositor work.

**Evidence:** `cargo test -p veyron-core -p veyron-ipc` (112 core tests, 5
window-feature integration tests, 9 multi-output integration tests, 8 profile
integration tests, 6 overview integration tests, 8 IPC tests) covers every
implemented item
above; `cargo clippy -p veyron-core -p veyron-ipc --all-targets -- -D warnings`
and `cargo fmt --all --check` pass. The compositor crate was not compiled in
this environment (Wayland/XKB development libraries are missing), and no nested
runtime visual check was performed.

**Acceptance:** layouts, rules, scratchpad and gap policy are visible and
interactive in a nested session, animations are frame-scheduled and can be
disabled, and every visual effect has a measured cost on the target hardware.
