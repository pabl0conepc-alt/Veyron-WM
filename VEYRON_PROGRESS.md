# Veyron progress and feature matrix

Last updated: 2026-11-21. Read together with `CONTINUATION.md`, `ROADMAP.md`
and `KNOWN_ISSUES.md`. States follow the master prompt: IMPLEMENTED requires
runtime validation, which no feature has yet in a visible nested session, so
code that compiles and passes tests is at most PARTIAL or UNVERIFIED.

## This session (branch `claude/render-input-wiring`)

Tooling: `nix develop` provides cargo 1.99 but neither clippy nor rustfmt
(rustfmt came from `nix shell nixpkgs#rustfmt`). The headless tests need
`LD_LIBRARY_PATH` pointing at nixpkgs `wayland` and `libxkbcommon` libs.
Builds ran with `-j1` on the 4 GiB netbook (about 7 minutes for a cold build of
`veyron`).

Changes:

- **Per-window opacity is rendered.** `state.rs::scene_elements` builds render
  elements per window with the rule opacity, in stacking order, sandwiched
  between layer-shell layers (overlay/top above, bottom/background below).
- **Layer-shell surfaces receive pointer input** (`layer_surface_under`).
- **Pointer focus bug fixed.** `surface_under` passed a surface-relative point
  where Smithay expects the surface's global origin; clients got wrong local
  coordinates. It now also resolves subsurfaces and popups.
- **Super + left-drag moves floating windows.** The core gained
  `Wm::move_floating_to`, now shared by the keyboard/IPC `MoveFloating` path
  (deduplicated clamp logic) and covered by a new unit test.

Tests run: `cargo test -p veyron-core` (126 unit and all integration tests
passed); `cargo test -p veyron` (2 unit tests and 7 headless real-client tests
passed). Clippy was not available locally; CI runs it.

## Feature matrix (summary)

| Area | Feature | State | Where / next action |
|---|---|---|---|
| WM | Scroll, master-stack, dwindle, auto layouts | PARTIAL | `layout.rs`, `tiling.rs`; nested visual check |
| WM | Floating, fullscreen, smart gaps, rules, scratchpad | PARTIAL | `wm.rs`, `rules.rs`; nested check |
| WM | Groups, PiP, snap zones, smart focus | PARTIAL | `groups.rs`, `snap.rs`; nested check |
| WM | Pointer move of floating windows | PARTIAL | `state.rs::begin_drag`; runtime test |
| WM | Pointer resize, client move/resize requests | MISSING | wire `XdgShellHandler::move_request/resize_request` |
| WM | Minimize | MISSING | needs core state + taskbar/foreign-toplevel |
| Motion | Position animations, workspace slide, per-window opt-out | PARTIAL | `motion.rs`; open/close anims missing |
| Motion | Spring/physics, reduced motion | PARTIAL | `motion.rs`; renderer ignores profile motion |
| Glass | Per-window opacity | PARTIAL | rendered now; needs visual check |
| Glass | Blur, rounded corners, shadows | MISSING | need GLES shader elements; heavy for HD 3000 |
| Spaces | Dynamic/named/persistent/special workspaces, 2D grid | PARTIAL | `wm.rs`; nested check |
| Spaces | Overview | PARTIAL | IPC geometry only; no drawn overview |
| Gestures | Touchpad/touch gestures | MISSING | winit backend gives no swipe events; needs libinput/udev backend |
| Recall | Session save/restore | PARTIAL | `session.rs`; no app relaunch |
| Command | IPC, veyronctl, structured responses | PARTIAL | `veyron-ipc`; owner-only socket |
| Profiles | Layout/motion profiles, cycling | PARTIAL | `profiles.rs` |
| Smart | Smart workspace, app discovery, launcher | PARTIAL | `smart.rs`, `launcher.rs`, `apps.rs` |
| Ecosystem | Layer-shell (bars, OSD, notifications) | PARTIAL | pointer now works; keyboard interactivity missing |
| Ecosystem | Native panel, notifications, OSD, clipboard, screenshot, recorder, studio | MISSING | separate clients over layer-shell + IPC |
| Wayland | Screencopy, screen sharing, XWayland | MISSING | see ADR 0002 |
| Wayland | Multi-output, hotplug | PARTIAL | core only; backend has one output |
| Wayland | HDR, VRR, multi-GPU, fractional scaling, color mgmt | BLOCKED | need DRM/KMS backend; HD 3000 lacks HDR |

## Next task

1. Let CI run clippy/fmt on this branch and fix anything it reports.
2. Run the nested compositor in the Plasma session and check opacity rules,
   super+drag and clicking a layer-shell bar (e.g. `waybar`).
3. Then wire `move_request`/`resize_request` and pointer resize, reusing
   `begin_drag`.
