# Roadmap

States follow the project prompt: `PLANNED`, `RESEARCHING`, `DESIGNED`, `IMPLEMENTING`, `IMPLEMENTED`, `TESTING`, `VERIFIED`, `BLOCKED`, or `DEFERRED`.

## Phase 0 — Environment and research — VERIFIED

- [x] Inspect repository and target environment.
- [x] Identify CPU, memory, graphics PCI ID, OS, kernel, and current graphical session.
- [x] Compare Smithay with lower-level protocol bindings and wlroots as a reference.
- [x] Record graphics device/module and rendering candidates; list actual EGL/GLES capabilities as unverified because no GL query tool is installed.
- [x] Record initial architecture, hardware constraints, and risks.
- [x] Establish a temporary Rust/Cargo toolchain and pin the initial Smithay dependency set.

**Acceptance:** environment report, dependency matrix, initial architecture, ADRs, risks, roadmap, and test plan are documented. The Smithay build baseline is a Phase 1 acceptance item, not a Phase 0 exit criterion.

## Phase 1 — Foundation — IMPLEMENTING

1. [x] Create a minimal Rust 2024 binary with pinned Smithay and serial Cargo builds.
2. [x] Add an initial nested Wayland/GLES compositor path with xdg-shell, shm and seat handlers.
3. [x] Type-check and correct the first implementation against Smithay 0.7 with `cargo check` (full executable build remains blocked by memory pressure).
4. [ ] Produce and run the executable; exercise a real Wayland client inside the nested session.

**Acceptance:** reproducible build; the Veyron nested session starts; a real client surface appears and can be closed; errors are handled; smoke/integration evidence is documented. This does not claim direct-seat, DRM/KMS, XWayland, or production readiness.

## Phase 2 — Window management — IMPLEMENTING

- [x] Add a protocol-independent split tree with deterministic placement, horizontal/vertical splits, minimum sizes, removal/promotion, and adjustable split ratios.
- [x] Add a state manager for tiled/floating modes, focus history, floating geometry and deterministic placement.
- [x] Add 16 standalone tests for layout geometry and manager state transitions.
- [ ] Integrate layout state with real xdg-shell surfaces, focus, floating, and compositor actions.
- [ ] Add normal Cargo unit/integration tests once the compositor dependencies build within available memory.

**Acceptance:** multiple real client windows can be arranged, focused, moved, resized, and closed without losing state.

## Phase 3 — Infinite Desktop — IMPLEMENTING

- [x] Implement a sparse 2D workspace map keyed by signed coordinates.
- [x] Create spaces lazily, preserve workspace identity and window IDs, and bound navigation history.
- [x] Add standalone tests for movement, revisiting, history, overflow, and invalid operations.
- [x] Give each workspace independent window-manager state and preserve a moved window's mode and floating geometry.
- [ ] Connect workspace/window-manager state to real Wayland surfaces.
- [ ] Add compositor keyboard actions and a visible location indicator; gestures and multiple outputs come later.

**Acceptance:** navigation in both axes works across dynamically created spaces and preserves client/window state.

## Phase 4 — Rendering and animations — PLANNED

Measure the baseline first. Add damage-aware rendering and frame scheduling through Smithay abstractions. Keep animations optional and off by default until resource measurements on target hardware support them.

**Acceptance:** stable rendering with animations disabled and measured frame/resource behavior; effects are not treated as a stability prerequisite.

## Phase 5 — Configuration and ecosystem — PLANNED

Add validated text configuration, then versioned and bounded local IPC, XWayland, portals, and compatibility features individually with evidence.

**Acceptance:** configuration errors retain the last valid state; IPC is local and validated; compatibility is documented per feature and tested.

## Phase 6 — Stabilization — PLANNED

Profile, test, harden IPC/session handling, document known limitations, and establish repeatable packaging before release claims.

**Acceptance:** reproducible builds and tests, measured performance on supported hardware, security review, compatibility notes, and known issues.
