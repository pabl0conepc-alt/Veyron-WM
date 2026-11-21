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

### Second batch: Veyron Theme and borrowed WM actions

- **Veyron Theme** (`veyron-core/src/theme.rs`): Material You schemes from a
  seed colour or preset via `material-colors` 0.5 (the generator the user's
  Sheels/cealestia-on-rust daemon already uses), nine variants, contrast,
  dark/light. Exports `colors.css`, `waybar.css`, `colors.scss`,
  `colors.json` to `$XDG_STATE_HOME/veyron/theme/` atomically and only when
  changed; headless runs never export.
- **Window borders** drawn by the compositor as reusable solid-colour
  elements (no damage when unchanged), focused = `primary`, others =
  `outline_variant`, hidden for fullscreen, following window opacity.
- **Runtime theme actions**: `theme toggle|dark|light|next|seed|variant`,
  bound to `super+t`, `super+shift+t`, `super+ctrl+t`.
- **New actions** (ideas from Hyprland/niri): `focus-cycle`, `move-to-workspace
  <n>`, `center-floating`, `gaps <±px>`, `opacity <±%>`, with default chords.
  `move_focused_to_workspace` was refactored into a position-based helper
  shared by direction and index moves.
- **Keybinding fix**: keys are matched by their unshifted Latin symbol, so
  `super+shift+1` and `alt+shift+tab` bindings can fire (they never matched
  before because the modified symbol was `exclam` / `iso_left_tab`).
- **IPC v4**: `Request::Theme` / `Response::Theme`, `veyronctl theme [--json]`.

Tests: core 136 unit tests (10 new) and all integration suites pass; 8
headless real-client tests pass, including a new theme-over-IPC test.
`material-colors` requires Rust 1.97; CI uses stable (1.99 at the time).

### Third batch: designs, rounded corners and shadows

- `crates/veyron/src/decoration.rs`: a texture shader (installed with
  `GlesFrame::override_default_tex_program` while a window's surface tree is
  drawn) clips client content to rounded corners in output pixels via
  `gl_FragCoord`; a pixel shader draws the rounded border ring and drop
  shadow. Both compile lazily once; failure falls back to solid borders.
  Decoration elements are cached per window and only bumped on change; a
  theme change resets the damage tracker for one full redraw.
- Popups are drawn unclipped in front of the clipped surface tree.
- `[theme] design` with five presets and per-field overrides; `theme design
  <name>|next` (`super+alt+t`); `inactive_opacity` dims unfocused windows.
- Nix: the custom `installPhase` looked in `target/release`, but
  buildRustPackage builds into `target/<triple>/release`; the default install
  hook is used now.

UNVERIFIED: nothing here has been seen on screen. The `frag_map` row flip
assumes the nested winit output's `Flipped180` transform; if corners are
clipped at the wrong edge, that is the first thing to check. Shadow offset
direction depends on the pixel shader's `v_coords` orientation.

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
| Glass | Rounded corners, shadows, designs | PARTIAL | `decoration.rs`; compiled, not seen on screen |
| Glass | Blur | MISSING | needs an offscreen pass; likely too heavy for HD 3000 |
| Theme | Material You scheme, presets, variants, runtime actions | PARTIAL | `theme.rs`; IPC-tested, not visually checked |
| Theme | Export to waybar/GTK/SCSS/JSON | PARTIAL | unit-tested export; check with a real waybar |
| Glass | Themed window borders | PARTIAL | `state.rs::scene_elements`; nested visual check |
| WM | Alt-tab cycle, move-to-workspace N, center, live gaps/opacity | PARTIAL | `wm.rs`; unit-tested |
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
4. Point the Sheels daemon (cealestia-on-rust) at Veyron: it already speaks a
   JSON IPC to driftwm; a `subscribe` event stream in `veyron-ipc` plus an
   `ext-workspace` export would let its bar show Veyron workspaces.
5. Rounded corners as a cheap GLES pixel shader on window elements, gated by
   a config switch so the HD 3000 can turn it off.
