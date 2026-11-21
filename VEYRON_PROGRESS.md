# Veyron WM persistent progress

Last updated: 2026-11-21

## Current task and authoritative repo

Continue the user's complete Veyron WM engineering audit/development prompt,
using the repository updated by another agent. The authoritative base inspected
is fetched `origin/main` at `ed18b75`, a three-crate Rust workspace. Do not use
the prior flat prototype as the active implementation.

- Active worktree: `/home/pablo/Documents/Codex/2026-11-19/lsi/outputs/Veyron-WM-current-main`
- Branch: `codex/current-main-audit`, based on `origin/main` at `ed18b75`.
- Latest pushed code commit: `85e5754 fix: reserve layer-shell exclusive work area`.
- Draft PR: none yet; create after implementation and checks are reviewed/green.
- User instruction: **no agents**. Continue solo, only on Veyron-WM, and do not merge a PR.
- Old checkouts remain preserved: `outputs/Veyron-WM` on `codex/engineering-audit`
  (flat prototype) and `outputs/Veyron-WM-finish` on `codex/finish-roadmap`.
  Do not transplant, reset, or merge their source into this worktree.

## Architecture and project state

Current workspace: `veyron-core` (policy/config/window model), `veyron-ipc`
(bounded Unix/JSON protocol), and `veyron` (Smithay 0.7/calloop compositor,
nested Winit/GLES runtime, headless Wayland server and CLI). Detailed documents:
`VEYRON_AUDIT.md`, `VEYRON_FEATURE_MATRIX.md` (187 rows),
`VEYRON_IMPLEMENTATION_PLAN.md`, and this file.

Current gaps include incomplete pointer/popup/layer routing, client fullscreen
requests not applied,
Recall not reconciling restored records with future real clients, reload IPC
reporting success on failure, protocol version not transmitted, uncapped IPC
peer threads, no native graphical ecosystem/Studio/gesture recognizer/XWayland,
and no target visual/performance evidence. See audit and matrix for details.

## Changes in this worktree

- Added tests in `crates/veyron-core/tests/smart_placement.rs` for affinity after
  workspace moves, output moves, and late identity.
- Fixed `Wm` to refresh app affinity after workspace/output moves, output/workspace
  migration, and first client identity/rule routing (`1c203fa`).
- Fixed Nix package dynamic Wayland client library discovery during tests and
  wrapped installed binaries with the Nix runtime library path (`253a901`).
- Fixed parallel headless-test SHM path collisions with an atomic counter
  scoped to the test process (`bf79323`). A regression test fixes the clock
  value and asserts generated names are distinct.
- Replaced the Nix package's hard-coded `target/release` binary installation
  with the standard Cargo install hook (`f9f2105`), after CI could not find the
  expected executable at that path. The Wayland runtime wrappers remain in
  `postInstall`.
- Added `wayland-protocols-wlr` as a test dependency and a real Wayland-client
  regression for a mapped top layer surface with a 24 px exclusive zone. Test
  scaffolding is in `e46bc78`, `70bc7d1`, and `353eb1f`.
- Implemented output work areas in `85e5754`: the physical output rectangle
  remains separate from its usable work area; Smithay's layer map supplies the
  non-exclusive zone; workspaces reflow and floating windows clamp when the
  area changes. Added a core regression for work-area updates.
- Created current-tree audit, 187-row matrix, implementation plan, and progress
  checkpoint. Some matrix rows are generated from category-level evidence;
  refine per-feature module/test/next-action specificity before treating the
  audit artifact as final.
- Updated `PROJECT_STATUS.md` and `CONTINUATION.md` for the latest tree. Check
  `ROADMAP.md`, `DEVELOPMENT_LOG.md`, and `KNOWN_ISSUES.md` before finishing docs.

## Validation ledger

- On base `ed18b75`, standard Actions job passed; Nix package job failed six
  headless tests with `NoWaylandLib` (dynamic `libwayland-client` not found).
  Run: `44912416024`.
- Test-first red evidence: `44925733597` failed the new workspace-move test:
  after moving Firefox from workspace 1 to 2, its next window still appeared
  on workspace 1. `44926164620` also failed late-identity affinity.
- Full standard job passed Format, Clippy, Build and all tests on commit
  `de011c3`, run `44926751159`. This includes the affinity fix and the output
  move test. The Nix job on that run used the old flake and has no bearing on
  the new package fix.
- Nix parser check: `nix-instantiate --parse flake.nix` passed after `253a901`.
  Run `44926821591` passed the standard job and reached all seven headless tests
  in the Nix package job; six passed and one collided on a shared-memory path.
- Regression test commit `0eaa47d` was red in the standard CI test step, with
  Format, Clippy, and Build green. On `bf79323`, run `44931933845` passed the
  complete standard job; Nix got through the headless tests and failed because
  the custom install phase could not find `target/release/veyron`.
- Commit `f9f2105` passed both jobs in run `44932470919`: standard format,
  Clippy, build and tests, plus the reproducible Nix package build and tests.
- Layer-shell test-first evidence: run `44937098387` failed in both standard
  and Nix tests; the mapped 24 px panel reduced toplevel height by 0 px. After
  the work-area implementation, run `44937377591` passed both jobs: standard
  format, Clippy, build and tests, plus the reproducible Nix package build and
  tests.
- The shell has Nix but no `cargo`/`rustc` on PATH. Do not fetch a large compiler
  closure onto the 4 GiB netbook; use GitHub Actions for full build/test/Clippy.
- No nested visual session or target GPU/performance measurements were run.

## Exact next work (checkpoint at `85e5754`)

1. Review/refine all 187 matrix entries against the latest source and tests.
2. Improve compositor input routing for child, popup and layer surfaces; then
   add popup grab/reposition and client fullscreen behavior with isolated
   protocol regressions. The current pointer path targets toplevel roots only.
3. Fix IPC reload error reporting, wire-level version negotiation and per-peer
   resource limits; add regression tests.
4. Reconcile Recall restore state with later Wayland client surfaces; test actual
   client matching and document app limitations.
5. Fix sparse overview coordinate/size overflow with boundary tests.
6. Update `ROADMAP.md`, `DEVELOPMENT_LOG.md`, `KNOWN_ISSUES.md`, and this file as
   work lands. Commit/push coherent increments and prepare a draft PR only after
   review; never merge without explicit user request.

## Constraints

- Preserve the low-resource 4 GiB netbook design; avoid concurrent/full local
  builds and unsupported performance claims.
- Work only on Veyron-WM. No agents. Keep all progress saved locally.
