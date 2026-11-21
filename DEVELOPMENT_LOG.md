# Development log

## 2026-11-19 — Initial inspection and Phase 0

- Repository was an initial commit containing only a marketing-style README; no Rust code, build files, tests, or project memory existed.
- Inspected NixOS 26.11 / Linux 6.18.63, i3-2370M, 4 GiB RAM, Intel graphics PCI ID 8086:0116, current Plasma Wayland session, and missing Rust/Wayland development tools.
- Checked Smithay handbook, repository guide, current crate documentation, examples, backend docs, and release information.
- Smithay is selected provisionally because it provides Rust-native compositor building blocks, a nested backend useful for development, and DRM/EGL/GLES facilities while leaving Veyron's window policy to the project. Low-level wayland-rs alone would require reimplementing too much protocol/backend plumbing; wlroots remains a reference because its main implementation is C, conflicting with the project's Rust-only implementation requirement.
- A temporary Nix shell with Rust 1.99.1, Cargo 1.99.0 and pkg-config was fetched (reported 328.7 MB / 1.1 GB unpacked); it does not alter the system profile. GCC was used temporarily for native build scripts.
- Added the Phase 0 research report, initial project status, architecture, roadmap, known issues, performance plan, and ADR.
- Started Phase 1 with a Smithay 0.7 nested Wayland/GLES compositor binary supporting xdg-shell toplevels, SHM, a keyboard seat, client rendering and frame callbacks. It is **not yet validated**.
- First `cargo check` failed because `cc` was absent. Retried with temporary GCC and one Cargo job; dependency compilation progressed, but available memory dropped to about 1.0 GiB and swap to about 95 MiB. Stopped the build deliberately to protect the graphical session. No successful compile, test, or runtime check is claimed.
- Phase 0 research deliverables are now `VERIFIED`; Phase 1 remains `IMPLEMENTING` until the binary builds and displays a real client surface.

## 2026-11-19 — Phase 2 model work

- Added a protocol-independent split-tree layout with equal-by-default horizontal/vertical splits, minimum-size enforcement, ratio resizing, deterministic placement, and sibling promotion on close.
- Added a window-state manager for tiled/floating windows, focus history, floating z order, mode transitions, and output-bounded floating geometry.
- Ran the standalone `rustc --test src/window/mod.rs` target: 16 tests passed. This validates the pure Rust model only; it does not type-check or exercise Smithay integration.
- Phase 2 remains `IMPLEMENTING`; real xdg-shell surface integration is still pending Phase 1 build/runtime validation.

## 2026-11-19 — Phase 3 spatial model

- Added a sparse 2D workspace map using signed coordinates, with lazy creation, stable workspace IDs, direct switching, and bounded 64-entry navigation history.
- Gave each workspace its own window manager and added cross-space transfer that retains mode and floating geometry.
- Re-ran the standalone Rust test target with `-D warnings`: 25 tests passed across layout, window management, and spatial navigation.
- Keyboard bindings, visual indicators, gestures, and integration with real Wayland surfaces/outputs remain open.

## 2026-11-19 — Phase 1 type-check and build attempt

- Ran `cargo check --jobs 1` with a temporary Nix Rust/GCC toolchain, incremental compilation disabled, and a systemd user-scope cap of 900 MiB RAM / 64 MiB swap. It completed successfully.
- Fixed the backend/crate `winit` name collision, explicit Smithay surface render-element type, mutable seat initialization, and focus borrow lifetime. Renderer frame completion now waits on the returned sync point.
- `rustfmt --check` passed and the 25 standalone Phase 2/3 model tests passed again with warnings denied.
- Attempted a full `cargo build` with the same cap. System swap fell to about 32 MiB during code generation, so the build was interrupted. The application has not yet been linked or run; no real client surface has been verified.
- Retried with `CARGO_PROFILE_DEV_CODEGEN_UNITS=1` and a 700 MiB RAM / 96 MiB swap cap. The compiler reached about 676 MiB RSS as host available memory fell below 400 MiB; stopped the build. This slower codegen setting did not provide a safe path to a linked executable.
- Phase 1 remains `IMPLEMENTING`; Phase 2/3 models remain unintegrated. A lower-memory executable-build strategy is needed before the nested runtime test.

## 2026-11-21 — Updated repository audit and remote build gate

- Audited the updated `Veyron-WM` worktree rather than the previous `Veyron-WM-finish` checkout. Preserved its README, Cargo package/lockfile, Smithay source, models, and Phase 0 docs on branch `codex/engineering-audit`.
- Added `VEYRON_AUDIT.md`, `VEYRON_FEATURE_MATRIX.md` (187 individual feature rows), `VEYRON_IMPLEMENTATION_PLAN.md`, and `VEYRON_PROGRESS.md`; added GitHub Actions so full compilation occurs away from the 4 GiB target.
- CI run `44922775244` passed format, Clippy, all 25 model tests, and release build on Ubuntu. Two earlier runs identified and fixed the `while_let_loop` lint, formatter indentation, and a test-fixture `field_reassign_with_default` lint.
- Phase 1 remains open: the release binary has not been launched in the target nested session and no real client surface has been observed. The next implementation dependency is a correctly advertised virtual output before surface placement integration.
