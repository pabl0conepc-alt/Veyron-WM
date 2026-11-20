# Development log

## 2026-11-20 — Layouts, rules, scratchpad and motion core

- Added `tiling`, `motion` and `rules` modules to `veyron-core` and wired them
  into configuration, actions and the control protocol (version 3).
- Layouts: `master-stack`, `dwindle` and dynamic `auto`, selectable per
  workspace; the strip remains the single source of window order and focus.
- Smart gaps, per-application rules (floating, opacity, blur, scratchpad, named
  workspace, layout), scratchpad recall and per-app floating geometry memory.
- Motion: validate-able spring/easing/rect animator with reduced-motion policy,
  intentionally not scheduled by the renderer yet.
- Verified in this environment: `cargo test -p veyron-core` (88 passed),
  `cargo test -p veyron-ipc` (8 passed), `cargo clippy -p veyron-core -p
  veyron-ipc --all-targets --locked -- -D warnings` and `cargo fmt --all
  --check`. The compositor crate could not be compiled here (no Wayland/XKB
  development libraries, no root to install them), and no nested runtime visual
  check was possible. GitHub Actions covers the full workspace build.

## 2026-11-19 — Project state audit

- Confirmed that GitHub branch `feat/headless-calloop-loop` contains the Rust
  workspace, scroll tiling/window model, configuration, IPC, headless real
  Wayland client tests and CI workflow.
- Verified GitHub Actions run `44437184079` succeeded for commit `c91d39e`:
  format, Clippy, workspace build and workspace tests all passed.
- Compared the implementation with the attached master prompt. Phase 0
  research was not present on that branch, so its host report and Smithay
  foundation decision were carried forward. Phase status and acceptance checks
  are recorded in [ROADMAP.md](ROADMAP.md).
- Found a scope gap: the current workspace strip is unbounded horizontally, but
  the requested Infinite Desktop also requires vertical movement and 2D spatial
  state. This remains open in Phase 3.
- Created `codex/finish-roadmap` from the tested Veyron feature branch and added
  separate commits for research and phase-gate documentation.

## 2026-11-19 — Phase 4 rendering and event readiness

- Registered the Wayland display poll fd in the nested calloop loop and removed
  its 16 ms periodic timeout. The headless 50 ms bound remains for retrying
  blocked client flushes.
- Integrated Smithay `OutputDamageTracker`, EGL buffer-age querying and swap
  damage. Missing age uses Smithay's full-redraw path. Reset the tracker on
  output resize and retain the existing flipped framebuffer orientation.
- `cargo fmt --all --check` and `git diff --check` pass. Full local compilation
  was deferred because the host had almost no free swap; the existing GitHub CI
  workflow will validate compilation and headless regressions after push.
- Nested runtime, animations, performance baselines, XWayland/portal support,
  external shell boundary and Phase 3 vertical movement remain unverified.

## 2026-11-19 — Phase 3 sparse spatial workspaces

- Added signed sparse `(x, y)` workspace coordinates with four-way navigation,
  lazy creation, revisiting, and focused-window transfer across workspaces.
- Preserved moved window identity, floating/tiled mode, floating geometry and
  fullscreen state; added real-client headless integration coverage.
- Exposed coordinates in IPC state (protocol version 2), added default key
  bindings and a visible workspace coordinate in the nested window title.
- `git diff --check` passes. GitHub Actions run `44649598532` passed format,
  Clippy, build and the complete workspace tests for commit `1fcd322`. The full
  local Smithay build remains delegated to CI to avoid memory pressure on the
  4 GiB target laptop.

## 2026-11-19 — Phase 2 floating-window resizing

- Added `resize width|height <pixels>` to the shared keybinding/IPC action
  grammar and mapped `super+alt+arrow` to 32-pixel floating-window resizing.
- Resizing keeps the window centered around its previous center, clamps it to
  the active viewport and honors client minimum dimensions. Tiled windows keep
  using the separate column-width controls.
- Added core boundary tests and extended the real-client headless scenario to
  exercise resize over IPC. After correcting the test's assumption about
  initial floating height, GitHub Actions run `44650727970` passed format,
  Clippy, build and all workspace tests on `e632b52`. Nested pointer-driven
  interaction and popup runtime checks remain open.

## 2026-11-19 — Phase 5 XWayland integration research

- Reviewed Smithay 0.7's official `XWayland`, `X11Wm` and `XwmHandler` APIs.
  Smithay starts the XWayland process, but the compositor must still implement
  the X11 WM lifecycle and surface adapter.
- Recorded the optional-feature boundary and real-client acceptance criteria in
  [ADR 0002](docs/decisions/0002-xwayland-integration-boundary.md). The feature
  remains unsupported until that adapter and an X11-client smoke test exist.

## 2026-11-19 — Phase 6 IPC socket permissions

- Restricted the control socket to owner read/write permissions (`0600`) after
  binding, including after stale-socket recovery.
- Added a filesystem-mode regression test and documented the trust boundary.
  GitHub Actions run `44651444722` passed format, Clippy, build and all
  workspace tests for commit `17d8562`.

## 2026-11-19 — Phase 2 floating-window movement

- Added `move-floating <direction> <pixels>` to keyboard/IPC actions with
  viewport clamping and preserved floating size.
- Added core boundary coverage and extended the real-client IPC scenario to
  move and resize the same floating window. GitHub Actions run `44651966135`
  passed format, Clippy, build and all workspace tests for `5ce2aa4`.

## 2026-11-19 — Phase 6 Nix packaging started

- Added a locked `x86_64-linux` Nix flake package for `veyron` and `veyronctl`,
  a GitHub Actions package-build job, and initial README usage instructions.
- Local flake evaluation returned `veyron-wm-0.1.0`. A full local build was
  intentionally not run on the 4 GiB netbook; the first package CI build is
  pending. Keep this Phase 6 item open until CI succeeds.
