# ADR 0002: XWayland integration boundary

- Status: accepted as an implementation plan; XWayland support is not yet
  implemented.
- Date: 2026-11-19

## Context

The Phase 5 roadmap requires X11 application compatibility. The current
compositor only manages Wayland `xdg_toplevel` windows. Smithay 0.7 exposes
XWayland helpers behind its `xwayland` Cargo feature, but the feature alone is
not an X11 window manager.

Smithay's `XWayland` helper launches and tracks the external `Xwayland` process,
and returns a Wayland client plus an X11 socket after readiness. The compositor
must provide the separate X11 WM implementation using `X11Wm`/`XwmHandler`, then
integrate X11 surfaces into its scene and lifecycle. This entails new map,
configure, focus, move/resize, destroy and selection paths. The nested and
headless Wayland tests alone cannot establish X11 compatibility.

## Decision

- Keep XWayland support explicitly unsupported until the X11 WM lifecycle is
  implemented and tested with a real X11 client.
- Integrate it as an optional capability around Smithay's XWayland helpers,
  preserving the current Wayland-only baseline and avoiding X11 dependencies
  for users/builds that do not request the capability.
- Reuse Veyron's core window identity and layout policy for X11 surfaces, but
  keep X11 protocol operations in the compositor crate behind an adapter.
- Run XWayland only as the user's unprivileged compositor child process. Route
  its readiness, XWM events and shutdown through the existing calloop loop.
- Do not advertise X11 app support until automated map/configure/focus/destroy
  coverage and a nested real-X11-client smoke test both pass.

## Consequences

- XWayland integration is a bounded follow-up rather than a one-line Cargo
  feature change.
- The eventual build needs the external `Xwayland` executable at runtime. CI
  will need an Xvfb or nested X11 display plus that executable for an actual
  X11-client smoke test; compiling the optional adapter is a separate check.
- Clipboard/selection forwarding, override-redirect windows and X11-specific
  focus behavior need explicit policy and tests. They must not be silently
  treated as ordinary xdg-shell surfaces.

## References

- [Smithay 0.7 XWayland module](https://docs.rs/smithay/0.7.0/smithay/xwayland/)
- [Smithay 0.7 XWayland process helper](https://docs.rs/smithay/0.7.0/smithay/xwayland/struct.XWayland.html)
- [Smithay 0.7 XWM handler](https://docs.rs/smithay/0.7.0/smithay/xwayland/xwm/trait.XwmHandler.html)
