# Known issues

- The nested GLES compositor has not yet been visually exercised in the target
  Plasma Wayland session. Headless integration tests do not validate GPU
  presentation.
- The spatial desktop currently expands horizontally only. Vertical movement,
  2D coordinates and a visible spatial indicator are required by the master
  prompt and remain open.
- Rendering now uses Smithay's output damage tracker and buffer age when
  available. The nested implementation has not yet been runtime-checked on the
  target EGL driver, and must fall back to full redraw if age is unavailable.
- Animations, transparency, blur and per-effect configuration are absent.
- XWayland, portal services, a stable external shell API, plugin isolation,
  direct DRM/KMS, session management and multi-output hotplug are not supported.
- No target-machine runtime baseline for RAM, CPU, frame pacing, startup or
  input latency has been measured. Do not make performance claims.
- IPC is local to the user's runtime directory and has message bounds and
  versioning. External plugins are not implemented; spawned commands intentionally
  execute with the user's own privileges.
