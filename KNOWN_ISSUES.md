# Known issues

- The nested GLES compositor has not yet been visually exercised in the target
  Plasma Wayland session. Headless integration tests do not validate GPU
  presentation.
- The spatial desktop currently expands horizontally only. Vertical movement,
  2D coordinates and a visible spatial indicator are required by the master
  prompt and remain open.
- Rendering redraws only when marked dirty, but each rendered frame currently
  damages the full output. Nested calloop waits are bounded and the scene does
  not yet use Smithay's `OutputDamageTracker`.
- Animations, transparency, blur and per-effect configuration are absent.
- XWayland, portal services, a stable external shell API, plugin isolation,
  direct DRM/KMS, session management and multi-output hotplug are not supported.
- No target-machine runtime baseline for RAM, CPU, frame pacing, startup or
  input latency has been measured. Do not make performance claims.
- IPC is local to the user's runtime directory and has message bounds and
  versioning. External plugins are not implemented; spawned commands intentionally
  execute with the user's own privileges.
