# Development log

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
