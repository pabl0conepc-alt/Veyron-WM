# Development log

## 2026-11-21 — Nix headless test runtime and SHM collision

- Added `runtimeLibs` to the Nix check environment and wrapped installed
  binaries with the Nix Wayland runtime library path in `253a901`. The Nix job
  then reached all headless integration tests instead of failing with
  `NoWaylandLib`.
- Run `44926821591` found one parallel test collision: SHM files were named
  using process ID plus a wall-clock timestamp, which is not guaranteed unique
  across threads. Six of seven headless tests passed in the Nix job.
- Added a deterministic regression with a repeated clock value in `0eaa47d`;
  its standard test step failed as expected. `bf79323` adds a process-wide
  atomic suffix to each name. Run `44931933845` passed Format, Clippy, Build,
  and all standard tests; its Nix package tests passed but installation failed
  because `target/release/veyron` was absent.
- `f9f2105` replaces the hard-coded binary copy with Cargo's standard install
  hook, retaining the runtime library wrappers. Nix parsing and local diff
  checks pass; Actions run `44932470919` passed standard CI and the reproducible
  Nix package build/tests. Installed runtime behavior on target hardware remains
  unverified.
- No Cargo toolchain is installed on the 4 GiB netbook, so this iteration uses
  GitHub Actions instead of downloading a large local compiler closure.

## 2026-11-21 — Re-audit of updated repository and smart-placement fix

- Fetched the repository update on `origin/main` through `ed18b75` and moved
  active work to branch `codex/current-main-audit`, based on the current
  three-crate workspace. The old flat-prototype checkout remains preserved.
- Added the four prompt-required audit/continuation artifacts: architecture
  audit, 187-row feature matrix, dependency-ordered implementation plan, and
  progress checkpoint. Updated `PROJECT_STATUS.md` and `CONTINUATION.md` for
  the active tree. Individual matrix entries still need review for file/test
  specificity.
- Added smart-placement regression tests for manual workspace/output moves and
  late client identity. CI confirmed stale placement after workspace move and
  late identity. `Wm` now refreshes app affinity after those operations and
  output/workspace migration.
- Verified standard CI passed format, Clippy, build and all workspace tests on
  `de011c3` (run `44926751159`). The 4 GiB netbook shell has no Cargo/rustc, so
  this implementation was not built locally.
- Investigated the Nix package failure: headless client tests reported
  `NoWaylandLib` because `wayland-client` uses `dlopen` and the Nix check/runtime
  path did not expose the library. `253a901` sets `LD_LIBRARY_PATH` in checks
  and wraps installed binaries; run `44926821591` must confirm the Nix job.
- No nested visual runtime or hardware-performance measurements were made.

## 2026-11-21 — Smart workspace and the Canvas action

- `veyron_core::smart::Affinities` learns `app id -> output/workspace` from the
  windows the core places and focuses. With `behavior.smart_workspace` enabled a
  new window whose app is already known opens next to its siblings on that
  output and workspace, without switching the user away. A rule that pins a
  workspace, or an explicit session-restore target, always wins. Learning is
  unconditional and cheap, so enabling the flag does not start from an empty
  history.
- Affinity entries are pruned when an output goes away, so a removed workspace
  cannot keep attracting windows.
- `canvas <dx> <dy>` (the "Veyron Canvas" action) moves every floating window of
  the active workspace together, clamping each window to the output. Window
  memory follows the move so the next window of the app does not jump back.
- Verified with `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`
  (125 core unit tests, 7 new smart-placement integration tests).

## 2026-11-21 — Workspace overview and layer-shell

- `Wm::overview(tile, gap)` lays every workspace of the active output on the
  sparse grid, normalises coordinates so the top-left tile starts at the origin,
  and scales each workspace's windows from the viewport into its tile.
  `build_snapshot` now takes a workspace index, so inactive workspaces can be
  inspected without switching to them.
- `veyronctl overview [--json]` reports that grid over the control protocol. The
  new request and response are additive, so the protocol stays at version 3.
- `zwlr_layer_shell_v1` is served: layer surfaces are mapped into the output's
  layer map with their exclusive zones arranged, receive an initial configure,
  are re-arranged and re-configured when the output resizes, and get frame
  callbacks. Panels, notification daemons, wallpapers and OSDs can run as
  clients. Pointer and keyboard input on layer surfaces is not implemented.
- Verified with `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`
  (122 core tests, 2 launcher unit tests in the binary, 5 window-feature,
  9 multi-output, 10 profile, 1 README-config, 9 IPC and 7 headless
  Wayland-client tests). Layer-shell is compile-verified only: no layer-shell
  client was run here.

## 2026-11-21 — Persistent workspaces, per-window motion and a launcher

- `[[workspaces]]` predefines named workspaces on the primary output with an
  optional sparse grid position and layout. A pinned position another workspace
  already owns falls back to the next free horizontal slot, and a reload only
  adds missing names, never removing a workspace in use.
- `workspace special` shows a scratch workspace named `special` (created on
  demand, so rules can route applications into it) and returns to the workspace
  it was opened from.
- Window rules gained `animation`. The resolved flag reaches `PlacementInfo`, and
  a false value both skips the workspace slide and snaps layout changes for
  that window.
- Switching workspaces slides the new workspace in from the direction of
  travel: `motion::workspace_slide` maps the sparse-grid delta to a viewport
  offset and the compositor starts each newly mapped window from there.
- `veyronctl launch <NAME>` starts an installed XDG application. The parser in
  `veyron-core::launcher` is pure (Desktop Entry group only, `Exec` field codes
  removed, quoted arguments honoured) and matching prefers an exact name or id,
  then a prefix, then a substring. The compositor scans
  `$XDG_DATA_HOME`/`$XDG_DATA_DIRS` application directories with depth, count and
  file-size bounds, lets earlier directories win, and spawns without a shell.
  `Terminal=true` entries are refused because Veyron assumes no terminal
  emulator.
- Verified with `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`
  (120 core tests, 2 launcher unit tests in the binary, 5 window-feature,
  9 multi-output, 10 profile, 1 README-config, 8 IPC and 7 headless
  Wayland-client tests). No nested or visual check was performed.

## 2026-11-21 — Frame-scheduled transitions and a dead-API pass

- The nested scene graph now drives the rectangle animator. Each mapped window
  keeps one animator; `tick` advances it by the measured frame delta and
  `dispatch_wait` caps the event-loop sleep at 16 ms while a transition runs, so
  a frame is produced until it settles. The frame that finishes a transition is
  marked dirty too, so the last rectangle drawn is the exact target. Motion
  stays off by default, which keeps a default session as static as before.
- Client sizes still configure at their target; only the scene-graph position
  interpolates. No nested-session visual check was performed.
- Restarting a transition from the rectangle actually on screen is
  `RectAnimator::new(current, target, &motion)`, which also keeps the
  disabled-motion snap in one place. That made `retarget` (it did the same, but
  only while motion was already active) and the `from()` accessor redundant;
  both are gone, along with the `if`/`else` in the layout sync.
- Removed public API no consumer called: `Rect::loc`,
  `GroupSet::ordered_members`, `RectAnimator::with_easing`,
  `Session::windows_on`, `Wm::active_workspace_index`,
  `Workspace::fullscreen_window` and `Wm::output_rect`. The write-only
  `Window::scratchpad` flag, its accessor and setter are gone too; the
  window-manager scratchpad list is the single source of truth.
- Verified with `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`
  (111 core tests, 5 window-feature, 9 multi-output, 10 profile, 1 README-config,
  8 IPC and 7 headless Wayland-client integration tests).

## 2026-11-20 — Recall over IPC

- Added `veyronctl session save FILE` and `veyronctl session restore FILE`.
- The control socket transfers validated session TOML through capture/restore
  requests. Oversized captures return a clear error at the existing 64 KiB
  message boundary; restore continues to use the core's bounded validation.
- Restore rebuilds window-management policy state only. It does not start
  applications or reconnect restored records to future Wayland surfaces.

## 2026-11-20 — Window identity and profile cycling

- The compositor now forwards the app id and title a client announces on its
  first commit. `Wm::set_window_identity` stores them and applies the matching
  window rules exactly once (opacity, blur, floating, scratchpad, workspace
  routing, layout), so rules stop being inert in a real session.
- `veyronctl state` (and the JSON report) now includes each window's app id,
  title and rule opacity; `veyronctl` prints the identity above the title.
- Added `profile next` / `profile prev` plus a default `super+p` chord, which
  closes the gap where no chord could select a profile whose name the user has
  not bound by hand. `next` and `prev` are reserved profile names.
- Kept the control protocol at version 3: the new state fields are additive, so
  existing clients keep working.
- Verified in this environment: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings` and
  `cargo test --workspace --locked` (112 core tests, 5 window-feature, 9
  multi-output, 10 profile, 1 README-config, 8 IPC and 6 headless Wayland-client
  integration tests). The compositor crate now links and runs here because the
  Wayland and XKB runtime libraries were extracted into a local prefix; the
  earlier note that it could not be compiled is superseded. No GPU/nested GLES
  session or visual check was performed.

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

## 2026-11-20 — Phase 7 snapping, groups and recall

- Added `snap.rs`: `SnapZone` (halves, quadrants, thirds, center, maximize) and
  `PipCorner` geometry, both clamped into the viewport, plus the `snap <zone>`
  and `pip [corner]` actions.
- Added `groups.rs`: exclusive window groups with adjacency in the strip
  (`ScrollLayout::reorder`), `group` / `group next` / `group prev` and pruning of
  closed windows.
- Added smart focus: a bounded most-recently-used history behind `focus
  previous`, recorded on every focus change.
- Added `session.rs` (Veyron Recall): validated TOML capture and restore of
  workspaces, windows and floating geometry through `Wm::create_window_in` and
  `Wm::restore_session`.
- `PlacementInfo` now exposes `pip`, and the default keymap gains `super+tab`,
  `super+shift+g`, `super+shift+n`, `super+shift+p` and `super+ctrl+alt+arrows`.
- Verified locally: 107 core tests, 5 window-feature integration tests and 8 IPC
  tests pass; Clippy with warnings denied and `cargo fmt --check` are clean. The
  Smithay crate still cannot build here (missing Wayland/XKB development
  libraries), so no nested runtime or visual check was performed.

## 2026-11-20 — Phase 7 multi-output policy

- `Wm` now manages outputs at runtime: `add_output`, `remove_output` (windows
  migrate to the surviving output, floating geometry and picture-in-picture are
  re-clamped, the last output is never removed), `focus_output`,
  `adjacent_output` (edge-based monitor adjacency) and `set_output_rect`.
- Every output owns its workspaces and its own viewport: configuration reloads,
  clamping and re-tiling are per output instead of following the active one.
- New actions `output <direction>`, `move-output <direction>` and
  `workspace-output <direction>`, with default bindings on `super+comma`,
  `super+period` (plus `shift` and `ctrl` variants); the action grammar and IPC
  accept them without a protocol bump because the payload shape is unchanged.
- `Snapshot` gained `output_name`/`output_index`, and `Wm::snapshots()` returns
  one frame description per output.
- Veyron Recall now captures the active output only, so restoring on a single
  output cannot collide on duplicated workspace names.
- Verified locally: 108 core tests, 5 window-feature, 9 multi-output and 8 IPC
  tests pass; Clippy with warnings denied and `cargo fmt --check` are clean. The
  backend still binds one output, so hotplug remains unverified at runtime.

## 2026-11-20 — Phase 7 gesture policy

- Added `gestures.rs`: `GestureConfig` (`enabled`, `swipe_threshold`,
  `axis_ratio`) and `SwipeTracker`, which accumulates swipe deltas, requires the
  dominant axis to win by the configured ratio and emits one `Direction` per
  threshold while keeping the remainder.
- `Config` gained a validated `[gestures]` table; it round-trips through
  `--print-config`.
- Four gesture unit tests plus a configuration test cover repeated steps,
  dominance and diagonal rejection (noise is dropped, never accumulated),
  disabled/invalid input with cancellation, and validation. The nested input
  layer still does not forward gestures, so nothing reacts at runtime yet.

## 2026-11-20 — Phase 7 workspace overview policy

- Added `overview.rs`: `OverviewConfig`, `OverviewEntry` and `Overview`, which
  lays an output's workspaces out in a paged grid ordered by sparse coordinate,
  reports per-workspace window counts, maps a click back to a workspace and
  exposes page navigation. `Overview::build_page` clamps the page index so a
  workspace closing cannot leave the grid pointing at a stale page.
- Entries distinguish `active` (the workspace its output shows) from `focused`
  (the output that has focus), which matters with several monitors.
- Added six integration tests: grid fill and non-overlap, hit-testing, spatial
  ordering, paging, window counts and per-output isolation.
- The compositor does not draw the overview yet; no action or binding opens it.

## 2026-11-20 — Phase 7 transition set for the render loop

- Added `motion::Transitions`: a per-window `RectAnimator` set driven by the
  caller's clock. `sync` follows a frame of placements (animating from in-flight
  or last-drawn geometry), `tick(dt)` advances and reports whether another frame
  is needed, and windows that leave the frame are dropped.
- Turning motion off or enabling reduced motion clears the set, so the renderer
  draws real target geometry immediately.
- Three new unit tests cover following placements, snapping when motion turns
  off and retarget/forget bookkeeping. The nested renderer still does not drive
  the set, so nothing animates at runtime yet.

## 2026-11-20 — Phase 7 Veyron Profiles

- Added `profiles.rs`: `LayoutPatch`/`MotionPatch` with optional fields, so a
  profile only overrides what it lists, plus `Profile`/`Profiles` with name,
  duplicate and whitespace validation.
- `Config` gained `[[profiles]]` and root `default_profile`; an unknown default
  profile is a configuration error and both round-trip through `--print-config`.
- `Wm::apply_profile` re-validates the patched layout and motion against the
  current configuration, reflows every workspace on every output, selects the
  patch's layout when it sets `mode`, and records `active_profile`; `Wm::new`
  applies `default_profile`.
- New action `profile <name>` (also `apply-profile`), usable from bindings and
  IPC without a protocol change.
- Verified locally: 109 core tests, 5 window-feature, 9 multi-output, 8 profile
  and 8 IPC tests pass; Clippy with warnings denied and `cargo fmt --check` are
  clean. The renderer still ignores motion settings changed by a profile.
- Verified locally: 109 core tests, 5 window-feature, 9 multi-output, 9 profile
  and 8 IPC tests pass; Clippy with warnings denied and `cargo fmt --check` are
  clean.

## 2026-11-20 — Phase 7 cleanup: drop unconsumed policy modules

- Removed `overview.rs`, `gestures.rs` and `motion::Transitions`: none of them
  had a consumer. No action, binding, IPC command or renderer ever opened an
  overview, forwarded a swipe or called `Transitions::sync`/`tick`, so they were
  speculative geometry and bookkeeping kept alive only by their own tests.
  Gestures and an exposé overview stay listed as unimplemented work in
  `ROADMAP.md` and `KNOWN_ISSUES.md`.
- Dropped the `[gestures]` configuration table with it, and the unused parts of
  the profile API: `Profile::description` (a note nothing read), `Profile::new`,
  `LayoutPatch::is_empty` and the `Wm::profiles()` accessor that duplicated
  `Wm::config().profiles`.
- `Profile::validate` now rejects whitespace with a single check; the separate
  leading/trailing-whitespace branch could never fire after it.
