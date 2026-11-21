# Veyron WM engineering audit

Audit started at `ed18b75` on 2026-11-21 and was refreshed on branch
`codex/current-main-audit` through `70bc7d1`. The repository had advanced well
beyond the earlier flat prototype: the current source is a three-crate Rust
workspace. This document records code-backed behavior, not feature names or
README claims.

## Architecture found

- Workspace root: `Cargo.toml` / `Cargo.lock`; Rust 2021, minimum Rust 1.87.
- `crates/veyron-core`: protocol- and I/O-independent state, configuration,
  window/workspace policy, layouts, rules, groups, snapping, motion, profiles,
  overview geometry, launcher parsing, smart placement and session records.
- `crates/veyron-ipc`: JSON request/response protocol, bounded framing,
  Unix-domain socket lifecycle and client helpers.
- `crates/veyron`: Smithay 0.7 compositor, `calloop`, nested Winit/GLES runtime,
  headless Wayland server, xdg-shell/SHM/seat/output/data-device/layer-shell
  handling, IPC server, CLI and XDG desktop-entry scanning/spawning.
- The compositor translates protocol events and actions into `veyron-core`,
  consumes snapshots to configure/render windows, and sends effects back to
  clients or the process launcher. Core tests run without a GPU; headless
  integration tests use actual Wayland clients.

## Modules and evidence

| Area | Implementation | Evidence and boundary |
|---|---|---|
| Event loop and backends | `crates/veyron/src/main.rs` | Nested Winit/GLES and headless modes share a Smithay display/calloop path. Only nested Winit creates a visible output; headless has no renderer. |
| Surface/window lifecycle | `crates/veyron/src/state.rs` | xdg toplevel create/commit/destroy, configure, app identity, activation, output assignment and snapshots are wired. Headless tests exercise real protocol clients. |
| Window model/layout | `crates/veyron-core/src/{wm,layout,tiling,window,rules,groups,snap}.rs` | Pure policy with unit/integration coverage; behavior reaches headless configure/action paths, but not all features have client-level tests or nested visual validation. |
| Workspaces/overview | `crates/veyron-core/src/{wm,overview}.rs` | Named and sparse signed 2D workspaces, transfer, multi-output policy and overview geometry. Overview is exposed as IPC data, not rendered by the compositor. |
| Input | `crates/veyron/src/state.rs`, Smithay `Seat` | Keyboard bindings and pointer forwarding exist. Pointer hit testing targets toplevel roots; subsurface/popup-local targeting and layer-shell pointer/keyboard focus are incomplete. No gesture recognizer is connected. |
| Rendering/motion | `crates/veyron/src/state.rs`, `crates/veyron-core/src/motion.rs` | GLES scene rendering, output damage/buffer-age handling, event-driven redraw and position/workspace transitions exist. No nested visual/pixel test; opacity/blur policy reaches snapshots but renderer ignores it. |
| Layer shell | `crates/veyron/src/state.rs` | Surfaces are arranged, configured and rendered via Smithay's output space. Exclusive zones are not applied to the core tiling viewport, so panels can overlap tiled clients. No layer-shell test. |
| Configuration | `crates/veyron-core/src/config.rs` | Strict TOML parsing, validation and reload retaining last valid config are present. IPC reload currently replies OK even when the reload fails internally. |
| IPC/commands | `crates/veyron-ipc/src/lib.rs`, `crates/veyron/src/{ipc.rs,state.rs,bin/veyronctl.rs}` | Owner-only socket, framing/message bounds, structured serde requests and CLI exist. `PROTOCOL_VERSION` is not transmitted or negotiated; server starts a thread per accepted connection without a concurrency cap. |
| Recall | `crates/veyron-core/src/session.rs`, `crates/veyron/src/state.rs` | Bounded validated TOML capture/restore over CLI/IPC exists. Restore produces model-only records; it neither launches apps nor reconciles later real surfaces, so actual session recovery is incomplete. Capture omits full multi-output/workspace-selection state. |
| Launcher | `crates/veyron-core/src/launcher.rs`, `crates/veyron/src/apps.rs` | Bounded XDG desktop-file scanner and direct argv spawn, exposed as `veyronctl launch`; it is not a native graphical launcher and has limited desktop-entry semantics. |
| Packaging/CI | `.github/workflows/ci.yml`, `flake.nix`, `flake.lock` | Nix runtime library paths are configured for checks and wrapped binaries. The parallel SHM collision and custom install path were fixed; run `44932470919` passed the standard CI and Nix package jobs. Flake targets `x86_64-linux`. |

## Important gaps and defects

1. Current main is a large feature advance over the previous task checkout. The
   previous branch's flat `src/main.rs` and its in-progress output scaffold are
   not the current architecture and must not be copied into this workspace.
2. Fixed on the active branch: `crates/veyron-core/src/wm.rs` now refreshes
   smart workspace affinity after manual workspace/output moves, whole-output
   workspace moves, output migration, and first identity/rule routing. CI run
   `44926751159` passed the complete standard job at `de011c3`.
3. `crates/veyron-core/src/overview.rs`: sparse `i64` workspace-coordinate
   deltas are cast to `i32`, and `tile.w + gap` / `tile.h + gap` can overflow at
   extreme input sizes. This needs bounded/saturating arithmetic and edge tests.
4. `crates/veyron/src/state.rs`: layer exclusive zones do not reduce the core
   usable viewport; pointer hit testing misses child/popup surfaces and layer
   surfaces; popup grab/reposition handlers are no-ops; client fullscreen
   requests are accepted without applying a fullscreen state change.
5. Session restore has no pending-client match/reconciliation, so restored
   metadata can become ghost windows or duplicate later client surfaces.
6. IPC's declared version is a constant only, not a wire-level handshake;
   reload success is reported even when parsing/reload fails; accepted peers
   are handled on uncapped threads.
7. Native graphical ecosystem components (panel, notifications, OSD, quick
   settings, Studio, screenshot/recorder, clipboard/emoji/color pickers) are
   absent. Existing launcher and overview are CLI/API/model features.
8. XWayland, portals, compositor gesture recognition, direct DRM/KMS, backend
   multi-output/hotplug, fractional scaling, HDR/VRR/color management, input
   device lifecycle, blur/rounded corners/shadows and automatic startup session
   recovery remain absent or partial.
9. Nix library lookup, parallel SHM names and binary installation were corrected
   in `253a901`, `bf79323`, and `f9f2105`; Actions run `44932470919` passed both
   jobs. This validates the locked Nix package build and tests, not an installed
   runtime on target hardware. Target-machine nested visual behavior, GPU
   compatibility, idle/load usage,
   frame pacing, input latency and memory measurements have not been recorded.

## Dependencies and environment

- Main dependencies: Smithay 0.7, calloop 0.14, winit 0.30, serde/JSON/TOML,
  thiserror and tracing. Exact Rust lock data is in `Cargo.lock`.
- The target documented by the project is NixOS on x86_64 with 4 GiB RAM and an
  older Intel GPU. Do not run concurrent heavy local Cargo builds; the shell
  currently has Nix but not Cargo/rustc on `PATH`.
- CI run `44912416024` at `ed18b75`: standard build/test/Clippy/fmt job passed;
  Nix package job failed because six Wayland-client headless tests reported
  `NoWaylandLib`. This is a packaging test/runtime environment defect, not a
  Rust compile failure.
- At `253a901`, Nix tests found `NoWaylandLib` resolved, then one headless test
  failed creating a duplicate SHM path under parallel execution. Commit
  `0eaa47d` added a deterministic repeated-clock regression test; its standard
  test job failed as expected. Commit `bf79323` adds an atomic per-process
   suffix; run `44931933845` passed the standard job and its Nix job reached
   installation before failing on the custom `target/release/veyron` path.
   Commit `f9f2105` replaces that install phase with the Cargo install hook;
   run `44932470919` passed both standard and Nix jobs.
- A real-client top-panel regression in `e46bc78`/`70bc7d1` fails in both CI
  jobs at `44936140199`: a 24 px exclusive zone changes client height by 0 px.
  This is the current red test; the core usable-area bridge is not implemented.

## Audit conclusion

The project now has a substantial integrated Wayland compositor and tested
policy/IPC foundation. It is not yet a production desktop: visual runtime and
hardware validation are missing, several interactions have known protocol
gaps, and much of the requested native ecosystem and advanced Wayland scope is
not implemented. The feature matrix records the individual states and next
steps.
