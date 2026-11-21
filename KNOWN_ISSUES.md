# Known issues

- The nested GLES compositor has not yet been visually exercised in the target
  Plasma Wayland session. Headless integration tests do not validate GPU
  presentation.
- Sparse 2D workspace navigation, transfer and state coordinates now have
  passing CI coverage. The visible nested title indicator and navigation still
  need validation in the target Plasma session.
- Floating windows can be resized by keyboard or IPC; pointer-driven move/resize,
  popup grabs and drag-and-drop are not wired or runtime-tested yet.
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
- Per-window opacity and blur reach the snapshot; the renderer still draws every
  window opaque and unblurred. Rounded corners and shadows are absent.
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
