# Initial architecture

Status: initial design, subject to evidence from the first nested compositor.

## Principles

- Rust owns Veyron policy: window state, layout, spatial navigation, configuration, and IPC.
- Smithay supplies mature Wayland server, protocol, input, session, and rendering building blocks; Veyron does not reimplement those low-level mechanisms without a measured reason.
- Keep window/layout logic independent of Wayland types so it can be tested without a display server.
- Use one event loop and synchronous state transitions initially. Do not add Tokio or worker threads without a blocking workload and a defined ownership boundary.
- Develop nested first; add direct DRM/KMS after core behavior works and can be tested without replacing the user's desktop session.
- Bound externally supplied inputs and handle errors at subsystem boundaries.

## Initial components

The first workspace should remain small: one `veyron` binary, with Rust modules for application state, Wayland integration, and protocol-independent window/layout state. Split into crates only when independent testing or dependency boundaries justify it.

Initial runtime flow:

1. Parse arguments and initialize structured diagnostics.
2. Create Smithay display and the selected nested backend.
3. Register core Wayland globals required by the first xdg-shell client.
4. Process backend and client events on one event loop.
5. Render client surfaces and deliver frame callbacks.
6. Tear down clients/backend cleanly on exit.

The initial phase does not include direct-seat ownership, XWayland, shell components, plugins, multi-monitor management, or a production configuration/IPC API.

## Rendering direction

Start with Smithay's GLES renderer on a nested backend, using the existing compositor/Mesa stack for presentation. This is a development path, not the final hardware backend decision. Evaluate software rendering as a fallback only if the target driver path is unusable; measure the impact before selecting it. Direct DRM/KMS and buffer/plane optimization belong after the first functional compositor.

## Window model direction

The current prototype uses a split tree for tiled geometry, a separate manager for tiled/floating mode and focus history, and a sparse coordinate map whose workspaces each own their own manager. These pieces are tested independently of Smithay. The workspace map allocates only visited coordinates and bounds navigation history. Compositor integration remains to be implemented.

## Error and security boundaries

- Startup errors should identify the failed subsystem and stop initialization cleanly.
- Client protocol errors should follow Wayland protocol semantics; do not panic on untrusted client input.
- Local IPC, if added, needs socket ownership/permissions, message-size limits, validation, and protocol versioning.
- Do not run the compositor as root.
