# Known issues

- The nested GLES compositor has not yet been visually exercised in the target
  Plasma Wayland session. Headless integration tests do not validate GPU
  presentation.
- Sparse 2D workspace navigation, transfer and state coordinates now have
  passing CI coverage. The visible nested title indicator and navigation still
  need validation in the target Plasma session.
- Floating windows can be moved and resized by keyboard or IPC, and moved with
  `super` + left-button drag (clamped to the viewport, sharing
  `Wm::move_floating_to` with the keyboard path). Pointer resize, client-
  initiated `xdg_toplevel.move/resize`, popup grabs and drag-and-drop are not
  wired; the drag has not been runtime-tested in a nested session.
- Rendering now uses Smithay's output damage tracker and buffer age when
  available. The nested implementation has not yet been runtime-checked on the
  target EGL driver, and must fall back to full redraw if age is unavailable.
- Layouts (master-stack, dwindle, auto), smart gaps, window rules, scratchpad
  and per-app floating geometry memory exist in the core and are unit-tested.
  Rules now match the real app id and title the client announces on its first
  commit, but they have not been visually validated in a nested session.
- The nested scene graph animates window positions between layouts and slides a
  newly active workspace in from the direction of travel, with a per-rule
  `animation = false` opt-out. Client sizes still configure at their target,
  opening and closing are not animated, and no visible nested-session check has
  been performed.
- The renderer draws each window with the opacity its rules request (scene
  elements are built per window instead of with one space-wide alpha). Blur
  reaches the snapshot but is not drawn; rounded corners and shadows are
  absent. Opacity has compiled and passed headless tests only, with no visual
  check on the target GPU.
- Snap zones, picture-in-picture, window groups and smart focus are implemented
  and tested in the core, and reach the compositor through the snapshot, but
  they have not been exercised in a nested session.
- Touchpad gestures and an exposé-style workspace overview are not implemented:
  the input layer forwards no swipe/hold events and there is no overview grid.
- Veyron Profiles are parsed, validated and applied by the core and accepted by
  the action/IPC grammar; `super+p` cycles them with `profile next`. The
  renderer still ignores the motion settings a profile can change.
- The launcher resolves XDG desktop entries by name or id and starts the command
  directly. It refuses `Terminal=true` entries, ignores desktop actions and does
  not use D-Bus activation, so applications relying on either need an explicit
  `spawn` binding.
- `zwlr_layer_shell_v1` surfaces render with their exclusive zones and receive
  frame callbacks, and now receive pointer input: overlay/top layers above
  windows, bottom/background layers where no window is. Keyboard
  interactivity on layer surfaces is not implemented yet.
- Pointer focus previously passed a surface-relative point where Smithay
  expects the surface's global origin, so clients saw wrong local coordinates;
  this is fixed and subsurfaces/popups under the pointer are now resolved.
- The workspace overview is geometry plus IPC (`veyronctl overview`); the
  compositor does not draw an overview surface, and no gesture input is wired.
- Smart workspace placement is off by default (`behavior.smart_workspace`). It
  learns from windows the core itself places and focuses, keeps one entry per
  app id (no title granularity), and never overrides a rule that pins a
  workspace.
- `canvas <dx> <dy>` only moves floating windows of the active workspace; tiled
  windows are untouched and each window stops at the output edge, so a canvas
  with windows already against both edges does not move.
- Veyron Recall saves and restores validated TOML through `veyronctl session
  save|restore`. Restore recreates policy state in the core; it does not launch
  applications or reconnect restored records to future client surfaces.
  Capture records the app id and title a client announces, so a saved session
  identifies windows by application.
- Multi-output policy exists in the core (runtime add/remove, per-output
  workspaces and viewports, monitor-directed focus and window/workspace moves)
  but the nested backend still creates a single output, so real hotplug and
  per-monitor rendering are untested.
- XWayland, portal services, a stable external shell API, plugin isolation,
  direct DRM/KMS, session management and backend multi-output hotplug are not
  supported.
- No target-machine runtime baseline for RAM, CPU, frame pacing, startup or
  input latency has been measured. Do not make performance claims.
- IPC is local to the user's runtime directory and has message bounds and
  versioning, and its control socket is restricted to the current user.
  External plugins are not implemented; spawned commands intentionally execute
  with the user's own privileges.
- Veyron Theme generates Material You schemes and draws solid window borders;
  theme files are exported only in a real (non-headless) session. Borders are
  square (no rounded corners) and have not been visually checked on the
  target GPU. Runtime `theme`, `gaps` and `opacity` changes are not written
  back to `veyron.toml`, so `reload` restores the file's values.
