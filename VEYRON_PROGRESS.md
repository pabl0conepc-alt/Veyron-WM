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

## Debug pass (branch `claude/debug-pass`)

A correctness review of the branch at `c18a061`, prioritising the last six
commits, then a sweep for panics and leaks on untrusted input. Baseline
before any change: `cargo fmt --check` clean, `clippy -D warnings` clean,
190 tests passing. After: fmt and clippy clean, 201 tests passing (11 new
regression tests, each seen failing before its fix). CI for `c18a061`
already passed both jobs, including `reproducible Nix package`, so
`flake.nix` was not changed.

Bugs fixed (file:line after the fix, symptom, fix, test):

| Where | Symptom | Fix | Test |
|---|---|---|---|
| `veyron/src/state.rs:1470` | xdg popups never got an initial configure, so menus never appeared | configure unconfigured popups on commit | `headless::popups_are_configured_and_receive_frame_callbacks` |
| `veyron/src/state.rs:1406` | popups got no frame callbacks (separate surface trees) | send frames to each toplevel's and layer's popups | same headless test |
| `veyron/src/state.rs:1496` | a toplevel opened behind a fullscreen window had no placement, so it was never configured and could not draw | initial configure on first commit when still unsent | `headless::a_window_opened_behind_a_fullscreen_window_is_still_configured` |
| `veyron/src/state.rs:888` | floating windows were never configured: `resize`, snap, PiP and fullscreen of a floating window changed the model only | configure floating windows when their rect size changes | `headless::floating_geometry_changes_reach_the_client` |
| `veyron/src/state.rs:1357` | wheel `v120` was sent as a notch count (1 instead of 120 per notch) | pass the raw v120 value | `state::tests::wheel_notches_are_reported_in_v120_units` |
| `veyron/src/state.rs:1338` | raw-symbol matching broke chords written with the typed symbol (`super+shift+plus`) | fall back to the modified symbol | `state::tests::bindings_match_the_unshifted_key_and_fall_back_to_the_typed_symbol` |
| `veyron/src/state.rs:1372`, `apps.rs:120` | spawned/launched children were never waited for and stayed zombies | reap each child on a 64 KiB-stack thread | `state::tests::spawned_commands_do_not_linger_as_zombies` |
| `veyron/src/state.rs:453` | clicking an overlay/top layer surface focused/raised (or super-dragged) the window underneath | skip window focus and drag when a layer surface takes the click | none (no input injection in the harness) |
| `veyron/src/state.rs:445` | any button release ended a super+drag and was swallowed, leaving buttons pressed during the drag stuck in the client | only the left button ends the drag | none (same reason) |
| `veyron-core/src/wm.rs:1738`, `:2027` | `workspace 99999999` (or `move-to-workspace`) created every workspace up to it, quadratically: hang then OOM | refuse non-existing indexes past 1024 | `wm::tests::huge_workspace_indexes_are_refused_without_creating_workspaces` |
| `veyron-core/src/session.rs:163` | a client title over 1024 bytes (or app id over 256) made every `session save` fail | truncate on a char boundary at capture | `session::tests::capture_survives_identities_longer_than_the_session_limits` |
| `veyron-core/src/layout.rs:73` | unbounded `gap`/`outer_margin` overflowed i32 geometry (debug panic, garbage in release) | validate `0..=4096` | `config::tests::absurd_gaps_and_margins_are_rejected` |
| `veyron-core/src/theme.rs:596` | a failed export left `.<name>.tmp` behind | remove the temporary file on error | `theme::tests::a_failed_export_leaves_no_temporary_file_behind` |
| `veyron-ipc/src/lib.rs:378` | a response over the 64 KiB line limit (e.g. `state` with many long titles) dropped the connection with no reply | answer with an error response | `tests::an_oversized_response_is_reported_instead_of_dropping_the_connection` |

Behaviour change to be aware of: a window floated by a rule is now
configured to its floating rectangle (remembered geometry or the default
3/5-viewport size) instead of keeping the tiled size it first received.

Reviewed and found correct: `surface_under`/`layer_surface_under` origins,
`move_floating_to`/`cycle_focus`/`move_focused_to_workspace_index`/
`adjust_gap`/`adjust_opacity`/`apply_theme_op`, the `[theme]` validation,
the IPC `Theme` request, `Chrome::forget_hidden`, scene element order and
alpha (translucent surface elements report no opaque regions), and the
`frag_map` maths: with `Flipped180`, output row = `height - gl_FragCoord.y`,
which lands on pixel centres of `clip_rect`.

Noted, not changed (doubtful or feature-sized):

- The window viewport ignores layer-shell exclusive zones, so a bar such as
  waybar overlaps tiled windows.
- A drag restarts the position animation on every motion event, so the
  window trails the pointer instead of tracking it; pointer focus is not
  refreshed when a drag ends until the next motion. If the dragged window
  closes mid-drag, motion stays swallowed until the button is released.
- Animators of windows on hidden workspaces persist, so the workspace
  slide-in only plays the first time a workspace is shown.
- Subsurface commits do not call `Window::on_commit` on the root window, so
  a desynchronised subsurface can leave the window's bounding box stale.
- `new_layer_surface` sends the first configure before the client's initial
  commit (anvil sends it on that commit); fine with Smithay, unverified with
  gtk-layer-shell clients.
- IPC: the socket is chmod-ed to 0600 after `bind` (a brief window with
  umask permissions, inside the 0700 runtime dir); one thread per
  connection is unbounded; `launch` scans desktop files on the compositor
  thread.
- Pointer coordinates are divided by the winit scale factor while the
  output renders at scale 1; HiDPI hosts are untested.
- Dwindle recursion depth equals the window count (a stack overflow needs
  tens of thousands of windows).
- Restored session windows have no surfaces but take layout slots (already
  documented in `KNOWN_ISSUES.md`).

Unverified: nothing was run on screen; the two input fixes and all
rendering paths still need the nested-session check.

### Fourth batch: on-screen check, UX and installation

- **First on-screen validation** (KDE Plasma, Intel HD Graphics 3000, nested,
  foot clients; screenshots in `docs/screenshots/`): rounded corner clipping,
  rounded borders, focused-border colour, neon glow, glass dimming, light
  mode, master-stack/dwindle/scroll layouts and runtime `theme` switching all
  render as intended. The `frag_map` row flip is correct.
- Found on screen and fixed: clients drew their own title bars (now
  server-side xdg-decoration); `--wayland-display` and `--output-size` were
  ignored in nested mode. Still open: a faint dotted dithering pattern in
  shadow edges on this GPU (`flat` avoids it), and KWin may keep the nested
  window at a remembered size.
- UX: config file watched and auto-reloaded (headless test), `veyron
  --check-config`, `terminal` action on super+return picking `$TERMINAL` or an
  installed terminal, `veyronctl watch [--waybar]` state stream with
  `contrib/waybar/` config and Material style.
- Installation: `install.sh` for NixOS, Arch, Debian/Ubuntu and Fedora
  (shellcheck clean; only `--help`/option parsing exercised, not a full run).
- README rewritten with a gallery, highlights, quick start and cheat sheet;
  the README test now validates every TOML block.

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
