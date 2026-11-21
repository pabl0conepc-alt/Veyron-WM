# Veyron WM project status

Last reviewed: 2026-11-21 against updated `origin/main` commit `ed18b75` and
active branch `codex/current-main-audit` (latest pushed commit `f9f2105`).

## Current repository state

- Current project is a three-crate Rust workspace: `veyron-core`, `veyron-ipc`,
  and `veyron`. It uses Smithay 0.7 and calloop; it is not the older flat
  prototype on `codex/engineering-audit`.
- `VEYRON_AUDIT.md`, `VEYRON_FEATURE_MATRIX.md`,
  `VEYRON_IMPLEMENTATION_PLAN.md`, and `VEYRON_PROGRESS.md` are the detailed
  current audit/implementation handoff.
- Core supports scroll/column layouts, master-stack/dwindle/auto, floating,
  rules, scratchpad, window groups, snapping/PiP, workspace navigation/transfers,
  output policy, profiles, motion state, launcher parsing, overview geometry,
  app affinity, and session-record capture/restore.
- The compositor runs nested Winit/GLES or headless Wayland protocol mode.
  Headless tests use real clients but do not validate visible GPU presentation.
- Current high-priority defects: layer-shell exclusive zones do not reduce
  tiling area;
  pointer hit testing misses subsurface/popup/layer surfaces; popup operations
  and client fullscreen behavior are incomplete; Recall does not reconcile
  restored records with future client surfaces; IPC reload can report false
  success, its version is not on-wire, and concurrent IPC peers are uncapped.

## Phase gates

- Phase 0 — environment/research: VERIFIED per `docs/PHASE0_RESEARCH.md` and
  ADR 0001.
- Phase 1 — foundation: TESTING; real headless Wayland clients and CI pass at
  base `ed18b75`, but visible nested GLES runtime is unverified.
- Phase 2 — window management: TESTING; policy and headless configure/actions
  exist, but real nested move/resize/popup/fullscreen interaction remains open.
- Phase 3 — workspaces: TESTING; sparse 2D policy and real-client navigation
  coverage exist; visible nested validation remains open.
- Phase 4 — rendering/motion: IMPLEMENTING; damage/event-driven rendering and
  position/workspace transitions exist, but no target visual/performance data.
- Phase 5 — configuration/ecosystem: TESTING; strict TOML, IPC, CLI launcher,
  layer-shell and overview reporting exist. XWayland, portal boundaries,
  external shell API and native UI remain open.
- Phase 6 — stabilization: IMPLEMENTING; standard and reproducible Nix CI pass
  at `f9f2105`, but target runtime, performance/security review and installed
  hardware evidence remain open.
- Phase 7 — window-management depth: IMPLEMENTING; core policy exists and is
  tested, but compositor/runtime validation and visual effects remain open.

## Build/test evidence

- At `ed18b75`, Actions run [44912416024](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44912416024)
  passed the normal format, Clippy, build, and test job. The Nix package job
  failed because six headless client tests could not dynamically load Wayland.
- Smart workspace affinity after manual/workspace/output moves and late identity
  routing is fixed in `1c203fa`. Standard CI passed format, Clippy, build and
  all tests at `de011c3` (run `44926751159`). Nix library discovery was fixed in
  `253a901`; run `44926821591` passed standard CI and reached headless tests in
  Nix, where one test exposed a shared-memory filename collision. The test-only
  commit `0eaa47d` failed the expected regression test; `bf79323` adds an atomic
  unique suffix and passed standard CI. Run `44931933845` then failed at the
  custom install phase because `target/release/veyron` was absent. Commit
  `f9f2105` uses the standard Cargo install hook; run `44932470919` passed both
  standard CI and the Nix package build/test job.
- The netbook shell has Nix but no `cargo`/`rustc` on PATH. A prior compiler
  closure estimate was too large for the 4 GiB machine; continue using CI for
  full workspace validation and avoid concurrent local builds.
- No nested visual run or target CPU/RAM/frame/input measurement is recorded.

## Target hardware and limits

Project notes identify NixOS 26.11, x86_64, Linux 6.18.63, KDE Plasma Wayland,
Intel Core i3-2370M, 4 GiB RAM, likely Intel HD Graphics 3000. GPU/EGL capability
has not been queried directly. The nested backend creates one output; multi-
output policy in `veyron-core` is not proof of backend hotplug or independent
physical monitors.
