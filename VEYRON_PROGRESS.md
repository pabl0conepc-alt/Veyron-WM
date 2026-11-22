# Veyron progress and feature matrix

Last updated: 2026-11-21. Read together with `CONTINUATION.md`, `ROADMAP.md`
and `KNOWN_ISSUES.md`. States follow the master prompt: IMPLEMENTED requires
runtime validation, which no feature has yet in a visible nested session, so
code that compiles and passes tests is at most PARTIAL or UNVERIFIED.

## One-line installer (branch `claude/curl-installer`, 2026-11-22)

- `install.sh` works piped from curl: banner, detected system, plan, a menu
  read from the tty, spinner, SHA-256 check, atomic replace, uninstall.
- `installer.yml` builds x86_64 and aarch64 archives on glibc 2.35, installs
  them in clean distribution containers and opens the nested compositor on
  Xvfb and headless Weston hosts. Only a `v*` tag publishes a release.
- Packaging fixes found on the way: the .deb lacked `libwayland-egl1` and
  `libwayland-server0`; the Nix package lacked the X11 libraries needed to
  run nested in an X11 session.
- PR #8 was merged before the daily-use batch and research block 1 were
  pushed, so those commits reach `main` with this branch.

## More layouts (branch `claude/more-layouts`)

- **New `LayoutKind`s** in `tiling.rs`: `grid` (rows of `ceil(sqrt(n))`, last
  row stretched), `center-tile` (mango `center_tile` / dwm centeredmaster:
  master centred at `master_ratio`, stack alternating right/left; a lone
  window is centred and the master never moves as windows come and go),
  `deck` (dwm: master plus one visible stack window, the focused or most
  recently focused one) and `monocle` (focused window fills the area).
  `arrange` takes the window to keep visible; monocle/deck give the others no
  placement, and the compositor already unmaps unplaced windows, so
  `state.rs` is unchanged. Focus next/prev wraps in monocle and deck, and
  `focus-cycle` now walks the full tiled order, so hidden windows are
  reachable.
- **`floating` workspace layout** (COSMIC style). The workspace's tiled set
  is drawn at each window's floating geometry; nothing moves between sets,
  so switching back to a tiling layout re-tiles exactly the same windows.
  Entering `floating` seeds every visible tiled window's floating rect from
  the current frame, so windows stay put. New windows open centred or at the
  remembered per-app geometry (also after a late `set_window_identity`).
  Move/resize/center/canvas-pan/pointer drags and geometry memory accept these
  windows (`Wm::floats`); output resizes clamp them. `toggle-floating` moves
  a window to the real floating layer, so it stays floating once the
  workspace tiles again. Fullscreen behaves as in every layout. Rules,
  presets, profiles, `layout <name>` and `layout next` all go through
  `Wm::set_workspace_layout`.
- **`[layout] center_focused_column`** (niri): `never` (default, previous
  behaviour), `always`, `on-overflow` (centre only when the focused column
  does not fit next to the previously focused one; otherwise keep both
  visible). Also patchable from `[profiles.layout]`.
- `layout next` order: scroll, master-stack, dwindle, auto, grid,
  center-tile, deck, monocle, floating (the original four keep their order).
  Names and aliases parse in `mode`, rules, presets, profiles and actions;
  unknown names are rejected. `veyronctl state` reports the new names.

Tests: fmt and clippy `-D warnings` clean; 246 tests pass (178 core unit,
33 core integration, 12 IPC, 7 compositor unit, 16 headless); 27 new core
unit tests (8 `tiling`, 6 `layout`, 10 `wm`, 3 `config`) plus extended
action/tiling tests. Not checked on screen: none of the new layouts or the
centring modes has been seen in a nested session yet.

## Layer-shell input and exclusive zones (branch `claude/layer-shell-input`)

- **Exclusive zones shrink the viewport.** The core gained
  `Wm::set_usable_area` (stored as `Insets` from the output edges, so it
  survives resizes) and `OutputState::usable_area`; tiled and floating
  windows are laid out in it. `state.rs::arrange_layers` re-arranges the
  layer map on every layer commit, unmap/destroy and output resize and hands
  `non_exclusive_zone()` to the core, retiling and reconfiguring on change.
- **Fullscreen covers the whole output** (previously the margin-inset
  viewport), and the top layer is drawn and hit-tested below a fullscreen
  window; the overlay layer stays above.
- **Keyboard interactivity.** `exclusive` overlay/top surfaces take the
  keyboard while mapped (buffer attached) and give it back to the core's
  focused window on unmap/destroy; window focus changes do not steal it and
  compositor bindings keep working. `on_demand` surfaces (any layer) take it
  on click; clicking or focusing a window, or a new window, takes it back.
  `none` never gets it.
- The first layer configure now answers the client's initial commit.

Tests: fmt and clippy `-D warnings` clean; 210 tests pass (146 core unit,
33 core integration, 10 IPC, 7 compositor unit, 14 headless). New:
`geometry::tests::insets_*` (2),
`wm::tests::panels_shrink_the_viewport_but_not_fullscreen`,
`wm::tests::reserved_edges_follow_output_resizes_and_keep_floating_windows_clear`,
`headless::a_panel_exclusive_zone_keeps_tiled_windows_clear_of_it`,
`headless::an_exclusive_keyboard_layer_holds_the_keyboard_until_it_closes`
(the harness client now speaks `zwlr_layer_shell_v1` and tracks
`wl_keyboard` focus). Not tested: `on_demand` click focus (no pointer
injection in the harness). Not visually checked with waybar, rofi or fuzzel.

## Previous session (branch `claude/render-input-wiring`)

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
  waybar overlaps tiled windows. (Fixed on `claude/layer-shell-input`.)
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
  gtk-layer-shell clients. (Fixed on `claude/layer-shell-input`: the first
  configure now answers the initial commit.)
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

### Arch Linux packaging (branch `claude/arch-linux-support`)

- `packaging/arch/PKGBUILD` (`veyron-wm-git`) and a hand-written `.SRCINFO`;
  `packaging/veyron.desktop` session file, which states that only the nested
  backend exists (no DM/TTY session yet). Workspace `rust-version` is now 1.97.
- CI job `arch-linux package`: `makepkg -s` with `check()` in an
  `archlinux:latest` container against the commit under test, plus
  `.SRCINFO` diff against `makepkg --printsrcinfo`, `desktop-file-validate`,
  `namcap` (informational) and a `pacman -U` install smoke test.
  Raising the MSRV enabled clippy's `chunks_exact_to_as_chunks` lint; the
  headless test now uses `as_chunks`.
- Verified in CI only (no local `makepkg`, no real Arch machine): the Arch job
  passes; `.SRCINFO` matches `makepkg --printsrcinfo`, the release build and
  all workspace tests (including the 8 headless real-client tests) pass in
  `check()`, and the installed package runs `veyron --version`/`--help`.
  Not verified: running the nested compositor on an Arch desktop.
- namcap reported a missing license file (fixed by adding LICENSE on
  2026-11-22) and flags the dlopen'ed libraries as "may not be needed",
  which is expected.

### Debian and Ubuntu packaging (branch `claude/debian-support`)

- `cargo deb -p veyron` builds `veyron-wm` from `[package.metadata.deb]` in
  `crates/veyron/Cargo.toml`; the dlopen'ed Wayland, xkbcommon and EGL
  libraries are declared explicitly.
- `.github/workflows/debian.yml` tests, packages and lints on `debian:trixie`
  and `ubuntu:24.04`, installs the package with apt in a clean container and
  smoke-tests the headless compositor over `veyronctl`. All four jobs passed.
  Not verified: the nested compositor on a real Debian or Ubuntu desktop.

## Daily-use batch (2026-11-21)

- **Pointer resize:** `super` + right drag resizes a floating window from the
  edges nearest the pointer (thirds of the window). Core: `ResizeEdges`,
  `Rect::resized`, `Wm::resize_floating_to` (minimum size, viewport clamp).
- **Client requests:** `xdg_toplevel.move/resize` (title bars, borders of
  client-side decorated apps) start the same grabs while the press is held;
  `set_fullscreen`/`unset_fullscreen` toggle the core fullscreen state and
  configures now carry the xdg `Fullscreen` state.
- **`veyronctl subscribe`** (protocol v5): pushed `{changes, state}` events for
  workspace, focus, windows, layout and theme changes; animations send none.
  `veyronctl watch [--waybar]` uses it instead of polling.
- **Open animation:** new windows rise 24 px and fade in over the motion
  duration.
- **Focus follows mouse:** `[behavior] focus_follows_mouse = true`, fully
  visible windows only.
- Tests: core unit tests for resize/fullscreen/config, IPC tests for change
  detection and streaming, headless tests for subscriptions and client
  fullscreen. Pointer grabs and the fade are not runtime-checked on screen.

## Block 1 of the research batch (2026-11-22)

Ideas taken from niri, Hyprland, sway/i3, mango, COSMIC and GNOME:

- Config `include` / `include_optional` (niri 26.04), merged under the
  including file; edits to any included file reload.
- `pin` (`super+o`), `mark` / `focus-mark` / `unmark`, `maximize` (`super+m`,
  and the title bar button through xdg maximize requests).
- `[theme] dim_inactive`, `[behavior] cursor_hide_after`, `[hot_corners]`,
  wheel chords (`super+wheel_up/down` switch workspaces).
- The agent half: grid, center-tile, deck, monocle and floating layouts and
  `center_focused_column` (section at the top).
- All of it is covered by unit or headless tests; none of the visual parts
  (dim, corners, wheel, cursor hiding, new layouts) was checked on screen.

## Feature matrix (summary)

| Area | Feature | State | Where / next action |
|---|---|---|---|
| WM | Scroll, master-stack, dwindle, auto layouts | PARTIAL | `layout.rs`, `tiling.rs`; nested visual check |
| WM | Grid, center-tile, deck, monocle layouts | PARTIAL | `tiling.rs`, `wm.rs::shown_window`; unit-tested, nested visual check |
| WM | Floating workspace layout (COSMIC style) | PARTIAL | `wm.rs::set_workspace_layout`, `Wm::floats`; unit-tested, pointer drag not runtime-checked |
| WM | `center_focused_column` never/always/on-overflow (niri) | PARTIAL | `layout.rs::ensure_focus_visible`; unit-tested, nested visual check |
| WM | Floating, fullscreen, smart gaps, rules, scratchpad | PARTIAL | `wm.rs`, `rules.rs`; nested check |
| WM | Groups, PiP, snap zones, smart focus | PARTIAL | `groups.rs`, `snap.rs`; nested check |
| WM | Pointer move/resize of floating windows (`super` + left/right drag) | PARTIAL | `state.rs::begin_super_grab`, `GrabKind`; core unit-tested, no input injection in the harness |
| WM | Client move/resize requests, client fullscreen (F11) | PARTIAL | `XdgShellHandler::move_request/resize_request/fullscreen_request`; fullscreen headless-tested |
| WM | Focus follows mouse (opt-in) | PARTIAL | `state.rs::focus_under_pointer`; fully visible windows only |
| WM | Pin (sticky), marks, maximize (+ client maximize requests) | PARTIAL | `wm.rs::toggle_pin/carry_sticky/toggle_maximize`, marks; unit-tested |
| WM | Wheel bindings, hot corners, idle cursor hiding | PARTIAL | `state.rs::wheel_binding/check_hot_corner`, `hotcorner.rs`; no pointer injection in tests |
| Glass | Dim inactive windows | PARTIAL | clip-shader `dim` uniform, black layer for square windows; not seen on screen |
| Config | Includes (`include`, `include_optional`) with reload on any file | PARTIAL | `config.rs::load_table`; unit-tested |
| WM | Minimize | MISSING | needs core state + taskbar/foreign-toplevel |
| Motion | Position animations, workspace slide, open (rise + fade), per-window opt-out | PARTIAL | `motion.rs`, `state.rs::opening`; close animation missing |
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
| Command | IPC, veyronctl, structured responses, `subscribe` event stream | PARTIAL | `veyron-ipc` v5; owner-only socket; headless-tested |
| Profiles | Layout/motion profiles, cycling | PARTIAL | `profiles.rs` |
| Smart | Smart workspace, app discovery, launcher | PARTIAL | `smart.rs`, `launcher.rs`, `apps.rs` |
| Ecosystem | Layer-shell (bars, OSD, notifications) | PARTIAL | pointer, exclusive zones and keyboard interactivity wired; zones and exclusive keyboard headless-tested; check waybar/rofi/fuzzel nested |
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
