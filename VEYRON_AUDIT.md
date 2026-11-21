# Veyron WM engineering audit

Audit date: 2026-11-21
Audited worktree: `/home/pablo/Documents/Codex/2026-11-19/lsi/outputs/Veyron-WM`
Branch/commit: `main` at `1338ebb` (`Initial commit`). The source, architecture notes and README changes in this worktree are currently uncommitted/untracked; they are treated as the authoritative user-provided working state and have been preserved.

## Executive finding

The current tree is an early Smithay prototype, not yet a functional desktop compositor. It has a real Rust binary entry point and nested winit/GLES initialization code, xdg-shell/SHM/seat protocol setup, a renderer loop, and separate tested Rust models for split layout, tiled/floating window state, and sparse 2D workspaces. The pure models are not connected to the xdg-shell surfaces. The only checked-in commit contains the original README; the current source and most project documents are untracked. Do not switch to, reset, or merge in the older `Veyron-WM-finish` worktree without reconciling these changes.

No runtime claim is made from source inspection. The repository's own status notes record a successful Smithay `cargo check`, 25 standalone model tests, and two full-build attempts stopped under memory pressure. Those are historical records in the current tree, not rerun in this audit. The test rerun was attempted through a temporary Nix shell; Nix estimated 558.5 MiB download and 1.9 GiB unpacked, so that fetch was interrupted to protect the 4 GiB host.

## Repository and technology inventory

| Area | Current evidence | Finding |
|---|---|---|
| Repository state | `git status`, `git log` | Local `main` and `origin/main` point at `1338ebb`; README has a tracked edit; `.cargo/`, Cargo files, docs and `src/` are untracked. Preserve all. |
| Rust package | `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml` | One Rust 2024 binary (`veyron-wm`), minimum Rust 1.88, serial Cargo jobs, no workspace split. Lockfile exists. |
| Compositor framework | `Cargo.toml`, `src/main.rs` | Smithay exactly `0.7.0`, `backend_winit` and `wayland_frontend`; winit with Wayland/X11; `wayland-server`, xdg protocol bindings, tracing. |
| Renderer | `src/main.rs::run_nested` | Creates Smithay winit backend with `GlesRenderer`, binds and draws Wayland surface trees. This proves intended GLES path in source only, not driver acceleration on target. |
| Protocols | Smithay delegates in `src/main.rs` | compositor, xdg-shell, shm, seat. Toplevels are activated/configured; popup callbacks are empty. No output global, layer-shell, data-device, screencopy, XWayland, or DRM/KMS setup was found. |
| Entry points | `src/main.rs::main`, `run_nested` | Only mode is nested compositor; socket defaults to `veyron-0` and can be selected with `VEYRON_WAYLAND_DISPLAY`. |
| Core policy | `src/window/layout.rs` | Pure split tree with horizontal/vertical splits, minimum sizes, ratios, placement, removal, and overflow errors. |
| Window manager | `src/window/manager.rs` | Pure state model for tiled/floating mode, focus history, floating order/geometry, close, split ratio and arrangement. No maximize/minimize/fullscreen/rules. |
| Workspaces | `src/window/desktop.rs` | Sparse signed 2D cells, stable local IDs, lazy navigation, bounded history and transfer. No output association, names, persistence or compositor actions. |
| Runtime state/lifecycle | `src/main.rs` | `new_toplevel` activates/configures without registering it in the model; surfaces are enumerated directly from Smithay state for rendering. Client data only logs connect/disconnect. No layout application, close/focus policy, or output geometry model. |
| Input | `src/main.rs` | Keyboard events are forwarded with no compositor keybinding/action dispatch. Absolute pointer motion focuses the first toplevel; pointer motion/button/axis are not sent to clients. No touch/tablet/libinput configuration. |
| Render scheduling | `src/main.rs` | Every loop clears and draws the full window-sized rectangle, waits synchronously on renderer sync, sends frame callbacks, dispatches clients and submits full damage. No damage tracking, explicit frame clock, animation engine or measured performance data. |
| Configuration | Inventory / `rg` | No parser, schema, defaults, validation, live reload, profiles, or migration code found. |
| IPC/commands | Inventory / `rg` | No Unix control socket, command dispatcher, external API, script bridge, or structured IPC responses found. |
| Persistence | Inventory / `rg` | No serialized session/workspace/configuration state found. Wayland app relaunch identity is a known limitation that must be stated rather than inferred. |
| UI/ecosystem | Inventory / `rg` | No panel, launcher, notifications, settings app, plugin manager, or integrated desktop service found. |
| Tests | source `#[test]` inventory and current docs | 10 layout + 6 window-manager + 9 spatial-desktop = 25 model tests. They are embedded in source modules and do not exercise Smithay. No Rust integration-test directory or CI workflow found. |
| Tooling/package | file inventory | No Nix flake/package, reproducible runtime package, CI workflow, benchmark harness, dependency audit, or license audit found in this current checkout. |
| Documentation | README, ARCHITECTURE, ROADMAP, PROJECT_STATUS, KNOWN_ISSUES, PERFORMANCE, DEVELOPMENT_LOG, PHASE0 report, ADR | Phase 0 research exists. Phase notes contain build and resource constraints. New audit/matrix/plan/progress documents are being added by this task. |

## Module map and actual responsibilities

### `src/main.rs`

The binary initializes tracing, binds one Wayland listening socket, creates Smithay compositor/SHM/seat/xdg-shell state, initializes the nested winit GLES backend, and loops over window events, render, frame callbacks, Wayland dispatch, and client flushes. It retains inserted clients in a vector. It does not call the separate `window` models. Toplevel activation is currently unconditional. Popup/grab/reposition callbacks and buffer destruction handling are empty. The code currently advertises no `wl_output`, so clients cannot receive a compositor output description from Veyron. The code's actual compile and runtime state needs renewed verification against the current files.

### `src/window/layout.rs`

Defines `WindowId`, geometry primitives, a binary split tree and deterministic arrangement. It guards duplicate/missing IDs, split ratios, minimum dimensions, and coordinate overflow. The model is a useful foundation; it does not implement the named layout families or Wayland configure protocol.

### `src/window/manager.rs`

Tracks a `BTreeMap` of window state, focus history, and floating z-order. It can arrange tiled and floating placements and clamp floating rectangles. This is pure policy and uses opaque numeric IDs; no mapping to `ToplevelSurface`, activation serial, actual client min/max constraints, or Smithay `Space` exists.

### `src/window/desktop.rs`

Tracks sparse `(x, y)` workspace cells, a stable local ID, active cell, and a bounded navigation history. Each cell owns a manager. It supports lazy directional creation, switching, movement and close. IDs only exist inside this process; there is no persistence or output scope.

## Architecture risks / defects to resolve

1. **Window lifecycle is disconnected:** the compositor does not own stable model IDs for `ToplevelSurface` objects, apply model placements as xdg configure sizes/locations, or remove model entries on surface destruction.
2. **No output global or output model:** clients cannot be told the virtual output's mode/scale/transform, and layouts have no authoritative bounds. This is a prerequisite for correct window placement.
3. **Input has incorrect semantics:** pointer motion changes keyboard focus to the first toplevel; actual pointer events and keyboard focus policy are absent. Keybindings are not dispatched.
4. **Frame loop is not demand-driven:** full redraw and synchronous GPU wait occur on every pump iteration. This can waste CPU/battery and can stall input; measure before tuning.
5. **Protocol completeness is minimal:** popup callbacks are no-ops and there is no data-device/selection, output, layer-shell, screencopy, XWayland or direct-session support. Do not call these supported.
6. **Build gate:** full code generation has been documented as exceeding safe memory pressure on the 4 GiB target. Compilation strategy and CI are needed before runtime-dependent changes can be trusted.
7. **State recovery and IPC are absent:** avoid building Studio/Profiles/Recall before a validated config schema and bounded secure local IPC boundary.
8. **Persistence cannot promise app restoration:** a compositor can persist Veyron's own workspace/layout data, but many Wayland clients do not expose a reliable restart token or stable app instance identity.
9. **No measured performance baseline:** do not claim efficient, low-RAM, low-latency or smooth behavior based only on architecture/source.
10. **Uncommitted project baseline:** nearly the entire current implementation exists outside the initial Git commit. Keep that work intact and make new files/diffs reviewable; do not assume a clean checkout reproduces it yet.

## Initial audit classifications

The individual 187 requested product/ecosystem features are recorded in [`VEYRON_FEATURE_MATRIX.md`](VEYRON_FEATURE_MATRIX.md). None is classified `IMPLEMENTED` in this audit: the pure layout/window/workspace model items are `PARTIAL`, and the others are `MISSING` until actual compositor integration and validation exist. The implementation dependencies and next gates are in [`VEYRON_IMPLEMENTATION_PLAN.md`](VEYRON_IMPLEMENTATION_PLAN.md). Session handoff is maintained in [`VEYRON_PROGRESS.md`](VEYRON_PROGRESS.md).

## Validation record

- This audit is based on source/docs/file inventory in the current local worktree, not the previous `Veyron-WM-finish` checkout.
- Historical project record: `cargo check` succeeded for an earlier state; executable build did not complete safely; 25 pure model tests reportedly passed.
- Current audit attempt to rerun standalone model tests: not completed. Temporary Nix shell estimated 558.5 MiB network transfer / 1.9 GiB unpacked; fetch was stopped before compilation. No current-pass claim is made.
- Runtime, real client rendering, window arrangement, input behavior, GPU acceleration and desktop-service integrations remain unverified.
