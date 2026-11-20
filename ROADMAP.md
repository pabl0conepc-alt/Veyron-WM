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
- [ ] Establish a reproducible user installation/package path after runtime
  validation.

**Acceptance:** all relevant suites pass, runtime and performance evidence is
reproducible, and security/compatibility limits are explicit.
