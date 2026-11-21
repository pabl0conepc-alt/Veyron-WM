# Veyron WM implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Do not spawn subagents; user has asked for direct work.

**Goal:** Turn the current Smithay prototype into an integrated, testable compositor by closing the core output, surface-lifecycle, window-management, workspace, input, configuration, IPC and rendering foundations before adding the larger desktop ecosystem.

**Architecture:** Keep Smithay as the Wayland/backend/render foundation and keep Veyron's window/workspace policy independent of protocol types. Add a small runtime adapter mapping real xdg-shell surfaces to stable model IDs, and make output geometry, input actions, configuration and IPC explicit state boundaries. Defer UI/effects/advanced protocol features until their underlying runtime and security interfaces exist.

**Tech Stack:** Rust 2024, Smithay 0.7.0, winit nested backend, GLES renderer, `wayland-server`, existing pure Rust model modules. Add dependencies only when a concrete feature requires them.

**Spec:** `VEYRON_AUDIT.md`, `VEYRON_FEATURE_MATRIX.md`, and the user's pasted engineering prompt.

## Global Constraints

- Target machine: x86_64 NixOS, 4 GiB RAM; keep local compilation serial and avoid full codegen when it pressures the graphical session.
- Rust minimum is 1.88; Smithay is pinned to 0.7.0 pending deliberate compatibility review.
- Do not call a feature `IMPLEMENTED` without compositor integration and evidence.
- No mocks as the only evidence for Wayland/runtime behavior.
- Preserve all uncommitted files in the current `Veyron-WM` worktree; do not merge them with the older `Veyron-WM-finish` branch.
- Keep direct DRM/KMS, XWayland, shell applications, blur and animation out of the runtime until their prerequisites and recovery behavior are tested.

## Review Focus

- xdg surface destruction while a window is focused: remove its model record and select a live focus target.
- Client min/max size smaller/larger than the output: configure without integer overflow or impossible tiled placements.
- Empty, rejected, or disconnected clients: compositor continues and releases state.
- Output resize while tiled/floating surfaces exist: geometry and configure sizes stay within the new bounds.
- Invalid key/config/IPC action: reject without mutating last-valid state or blocking the event loop.

## File map

- `src/main.rs`: keep the event-loop entry compact; register protocol handlers and invoke runtime services.
- `src/window/`: protocol-independent layout/window/desktop policy; add tests here for deterministic policy.
- `src/compositor/` (proposed): stable surface registry, output model, and Smithay-to-policy adapter, split by responsibility as the integration grows.
- `src/config/` (proposed): strict config types, parser, defaults and reload transaction.
- `src/ipc/` (proposed): owner-only Unix listener, bounded framing, validated command dispatch and responses.
- `tests/` (proposed): real Wayland client scenarios after an executable can be built reliably.
- `VEYRON_AUDIT.md`, `VEYRON_FEATURE_MATRIX.md`, `VEYRON_IMPLEMENTATION_PLAN.md`, `VEYRON_PROGRESS.md`: persistent evidence and handoff.

## Ordered implementation tasks

### 1. Core Architecture and build gate

- [ ] Reconcile the untracked current source into a reviewable versioned baseline without dropping README edits; record exact `git status` and base commit in progress.
- [ ] Establish a safe CI build/test/fmt/clippy workflow so full Smithay compilation occurs away from the 4 GiB target.
- [ ] Re-run current `cargo check`, model tests, then build under a measured safe limit; keep Phase 1 open until executable launch evidence exists.

### 2. Wayland Protocol Infrastructure and outputs

- [ ] Add a single-output model initialized from nested winit output size/scale.
- [ ] Advertise `wl_output` with coherent logical/physical metadata and update clients on resize.
- [ ] Add protocol-level test asserting output global/mode/scale and output-size changes.

### 3. Rendering Infrastructure

- [ ] Establish a persistent Smithay `Space` or equivalent surface-location registry and explicit output association.
- [ ] Render mapped surfaces at policy placements, report output enter/leave, and track buffer commits/damage.
- [ ] Replace unconditional whole-output redraw only after a correctness test for buffer age/damage and idle wakeups exists.
- [ ] Add a nested real-client smoke test; record hardware acceleration only if renderer/driver is queried on target.

### 4. Window Management

- [ ] Map each `ToplevelSurface` to a unique nonzero `WindowId`; register on first valid map and remove on destruction.
- [ ] Derive geometry/output bounds and usable min/max size constraints from protocol state; arrange and send configure sizes from `WindowManager`.
- [ ] Keep focus/activation, close, tiled/floating mode, move/resize and stacking consistent between model, Smithay seat, and render order.
- [ ] Add multi-client real Wayland tests for map, configure, focus, unmap/destroy, resize constraints and floating placement.
- [ ] Defer named layout families, groups, rules, scratchpads, PIP and minimize until base operations are stable.

### 5. Workspace Management

- [ ] Bind registered surfaces to `SpatialDesktop` IDs and render only the active cell while preserving inactive surface state.
- [ ] Add deterministic keyboard actions for 2D navigation and moving a focused surface between cells.
- [ ] Add real-client integration tests for both axes, revisiting, client identity and closed surfaces.
- [ ] Add workspace names/persistence/output assignment only after config and output boundaries are stable.

### 6. Input and Gestures

- [ ] Correct pointer routing using actual hit testing, surface-local coordinates, pointer/button/axis forwarding and focus policy.
- [ ] Add validated keybinding-to-action dispatch without consuming unbound keys.
- [ ] Add tablet/touch/gesture support only with matching Smithay/libinput events and device-level tests; do not infer from synthetic input.

### 7. Animation Engine

- [ ] First record idle/client-load CPU, RSS and frame timing on target when runtime works.
- [ ] Only if measurements permit, implement optional, disabled-by-default event-driven transitions with bounded state, duration, easing and reduced-motion configuration.
- [ ] Prove frame synchronization and disabled-motion behavior in renderer tests; skip physics if it threatens input/render latency.

### 8. Visual Effects

- [ ] Query actual GLES capabilities and implement only supported opacity/shadow/blur paths with resource bounds.
- [ ] Keep unsupported effects absent and report them as such; compare CPU/GPU/memory/frame times before enabling any by default.

### 9. Configuration System

- [ ] Define strict versioned TOML types for keybindings and already-implemented compositor behavior.
- [ ] Validate the full candidate before replacing last-valid runtime state; test malformed, unknown and boundary values.
- [ ] Add file-watch/reload only after the transactional parser exists; persist profiles later.

### 10. IPC and Command System

- [ ] Design bounded versioned Unix-socket messages, owner-only permissions, stale-socket handling and explicit command authorization.
- [ ] Implement structured success/error responses for actual existing actions and read-only state.
- [ ] Test malformed/oversized requests, peer disconnect, permissions and end-to-end command effects.

### 11. Persistence and Recall

- [ ] Persist only Veyron-owned workspace/layout/config metadata atomically and with a schema version.
- [ ] Restore compositor state without claiming app relaunch unless a client supplies a valid restart token; test missing/changed identities.

### 12. Intelligent Features

- [ ] Derive policies only from real, available surface/app/output metadata; define deterministic, configurable rules and tests.
- [ ] Implement snapping/automatic arrangements only after pointer, constraints and output models are correct.
- [ ] No focus prediction, app discovery or “smart” claims without input data and measured user-visible behavior.

### 13. Native Desktop Components

- [ ] Decide whether these are separate applications/services or compositor-owned UI; use documented real D-Bus/portal/service protocols.
- [ ] Implement one service at a time with failure handling and end-to-end integration; do not add demo-only panels/menus.

### 14. Veyron Studio

- [ ] Start after config, IPC and profile schemas stabilize.
- [ ] Editors must update the validated real configuration path and demonstrate reload/persisted effect; preview is not proof of runtime integration.

### 15. Advanced Wayland Features

- [ ] Add XWayland only with complete Smithay XWM lifecycle and real X11 client test.
- [ ] Add screencopy/screen sharing only with protocol/portal permission handling and real client validation.
- [ ] Add multi-output, fractional scale, color, HDR/VRR, hotplug and devices only behind explicit capability detection and relevant hardware tests.

### 16. Integration and Optimization

- [ ] Run combined window/workspace/input/output/config/IPC tests and audit lifecycle/error cleanup.
- [ ] Capture repeatable CPU/RSS/frame/input latency on the target; optimize evidence-backed hot paths.
- [ ] Audit dependencies/licenses/security; document tested compatibility and unsupported features.
- [ ] Update all four persistent progress documents after each meaningful milestone; retain PR/branch context only if this repo is intentionally moved back to the earlier feature branch.

## Exit criteria

The broad vision remains open until each feature in `VEYRON_FEATURE_MATRIX.md` is either implemented and validated or explicitly deferred/blocked with a technical reason. Phase-level completion requires integration evidence at the same scope as the phase; a pure-model test never proves runtime compositor behavior.
