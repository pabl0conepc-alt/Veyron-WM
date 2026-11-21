# Veyron WM implementation plan

Plan target: updated repository baseline `ed18b75` and active work branch
`codex/current-main-audit` through `85e5754`. This worktree is based on the updated three-crate
workspace. The older flat-prototype branch is preserved separately and is not
the implementation base.

The phases below follow dependency order. Existing features are retained and
extended; policy modules are not rewritten merely because they have feature
names. Each item stays open until its behavior has appropriate integration
evidence.

| Order | Work package | Current baseline | Required work / acceptance evidence | Priority |
|---|---|---|---|---|
| 0 | Repository discovery and feature audit | Three crates, Smithay 0.7/calloop, docs and tests inventoried. | Keep audit, 187-row matrix, plan and progress aligned with updated `origin/main`; distinguish policy, protocol, visual and hardware evidence. | Active |
| 1 | Core architecture | Protocol-free Wm/config/action/session plus compositor adapter exist. | Smart affinity now updates after manual workspace/output moves and late identity; standard CI passes. Still bound overview arithmetic and add edge tests; keep single source of truth for window/output state. | In progress |
| 2 | Wayland protocol infrastructure | xdg-shell, SHM, seat, output globals, layer shell, data device, headless real clients. Exclusive zones reserve usable area (`85e5754`, CI `44937377591`); a client fullscreen test is red (`44939749187`). | Finish xdg fullscreen model/configure integration, then route input to child/popup/layer surfaces and implement popup grabs/reposition with real-client regressions. | Highest integration priority |
| 3 | Rendering infrastructure | Nested GLES scene graph, damage tracking, buffer-age fallback, event-driven redraw. | Verify resize/damage scheduling and idle wakeups; expose runtime evidence; apply alpha/opacity policy; only add GPU effects after a tested path. Keep headless renderer explicitly separate. | High |
| 4 | Window management | Core supports strip, layouts, floating, rules, snap/PiP, groups, scratchpad, focus history; adapter configures snapshots. | Add client-level tests for more policies, client fullscreen/modal behavior, and input-driven move/resize; preserve constraints and focus invariants. | High |
| 5 | Workspace management | Named and sparse 2D workspaces, transfer, output policy, overview geometry and CLI report. | Test named/persistent workspace lifecycle and resize; define visible overview interaction; verify output migration against backend events. | High |
| 6 | Input and gestures | Keyboard actions and pointer forwarding work; no gesture recognizer. | Correct subsurface/popup/layer hit testing first. Then define a small configurable gesture recognizer for actual supported touchpad/touch event sources; avoid calling a CLI Canvas action a gesture. | Medium |
| 7 | Animation engine | Spring/easing/rect motion and workspace slide are connected to nested render scheduling, disabled by default. | Add deterministic frame/timing tests and nested visual evidence; add event types only when the state transition is integrated. | Medium |
| 8 | Visual effects | Rule state carries opacity/blur, but rendering ignores both. Rounded corners/shadows absent. | Implement opacity before blur only if Smithay/GLES scene elements provide a stable low-overhead path; test pixels/render path; measure target cost. Do not mark effects supported from policy alone. | Medium / hardware gated |
| 9 | Configuration system | Strict TOML config, defaults, validation and reload-retains-last-valid behavior. | Make reload IPC return the actual parse/apply result; add regression; document schema/defaults and migration semantics. | High |
| 10 | IPC and command system | Local owner-only socket, bounded JSON frames, CLI, actions/state/session/overview/launch. | Put protocol version on wire with compatibility behavior; cap accepted connections/backpressure; return structured errors for reload/restore; test peer bounds and version mismatch. | High |
| 11 | Persistence and Recall | Manual TOML save/restore of validated model records. | Avoid ghost windows: model restore must reconcile app identity with future clients or clearly separate pending restored records; capture workspace/output/layout/profile state; add real-client reconciliation tests. | High |
| 12 | Intelligent features | App rules, geometry memory, focus history and opt-in app affinity are available. | Fix affinity to learn after moves, identity routing and output transfer; test tie-breaking and stale-entry pruning using actual metadata. Keep heuristics deterministic. | Medium |
| 13 | Native desktop components | XDG app resolver/launcher exposed through CLI; layer-shell supports external clients. | Prefer integration with established external desktop services/components over building costly UI in this low-RAM compositor. Define a narrow contract for panel, notifications, OSD, screenshot/media/clipboard tools; no demo UI. | Later |
| 14 | Veyron Studio | No graphical configuration application. | Only begin after config validation/reload and stable IPC schemas. Build an external client editing real TOML and applying validated reloads; preserve rollback and export/import tests. | Later |
| 15 | Advanced Wayland features | Nested Winit only; multi-output policy exists in core, one backend output. | Select features by Smithay/backend support and hardware: XWayland, output hotplug/scaling, screencopy/portal, tablet/touch, color/HDR/VRR. Record unsupported boundaries; do not promise direct DRM until target GPU path is chosen. | Later / capability gated |
| 16 | Integration and optimization | Standard CI and Nix package build/tests pass at `f9f2105`; no target nested visual/performance baseline. | Validate real-client nested behavior; capture RAM/CPU/frame/startup/input data on target before tuning; audit security/dependencies/licenses; verify installed runtime and document installation. | Required before release |

## Immediate execution queue

1. **Done:** fix smart workspace affinity after manual workspace/output moves
   and late identity routing; standard CI passed at `de011c3`.
2. **Done:** fix Nix runtime library discovery, SHM path collisions and install
   hook; standard and Nix jobs passed at `f9f2105`.
3. **Done:** add core output work areas and synchronize Smithay layer-shell
   exclusive zones. The real mapped-panel regression and both CI jobs pass at
   `85e5754` (`44937377591`).
4. **In progress:** finish client xdg fullscreen. Test `aa361d7` failed in Nix
   as expected (`44939749187`); `Wm::set_window_fullscreen` is drafted locally,
   but the xdg callbacks/configure state are not integrated or verified.
5. Review and refine all 187 feature-matrix rows against concrete source/tests.
6. Improve pointer routing for subsurface/popup/layer surfaces; implement popup
   grabs/reposition with focused real-client tests.
7. Fix IPC reload errors, on-wire versioning and peer limits; reconcile Recall
   with future clients; fix overview arithmetic bounds.
8. Continue remaining phases. Keep nested visual/performance evidence open
   until measured on the target netbook.

## Project constraints

- Target machine is documented as a 4 GiB NixOS netbook with older Intel GPU.
- Keep Rust/Cargo builds serial on that machine; use CI for the full build.
- The current shell has Nix, not Cargo/rustc. Avoid downloading a large Nix
  compiler closure to the target just to repeat checks already performed by CI.
- No agents: the user explicitly said to continue without them.
- Work only in the Veyron-WM repository. Keep the PR draft and do not merge.
