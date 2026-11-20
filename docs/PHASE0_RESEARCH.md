# Phase 0 — Environment and technology research

Research snapshot: 2026-11-19. This is an initial engineering assessment, not a compatibility certification. It records the initial implementation decision; current feature and phase status is maintained in [ROADMAP.md](../ROADMAP.md) and [PROJECT_STATUS.md](../PROJECT_STATUS.md).

## Host findings

| Area | Observed | Consequence |
|---|---|---|
| OS/kernel | NixOS 26.11, Linux 6.18.63, x86_64 | Prefer reproducible Nix-based build instructions; avoid global system mutations. |
| CPU | Intel Core i3-2370M, 2 cores / 4 threads | Serial builds and bounded work; avoid unnecessary runtime threads. |
| Memory | 4 GiB total; about 1.5 GiB available at inspection | Use `cargo check`/test serially and reduce debug info; full compositor builds may pressure RAM. |
| Graphics | PCI vendor/device `8086:0116`; i915 loaded; `/dev/dri/card0` and `renderD128` present | Strongly indicates Intel HD Graphics 3000. Mesa/EGL version and actual renderer were not queried, so acceleration is not yet verified. |
| Session | KDE Plasma / KWin on Wayland; XWayland active | A nested backend can be exercised without displacing the active session. |
| Toolchain | `nix` 2.35.6; Rust, Cargo, pkg-config, Clang, lspci and wayland-info absent from PATH | No compile/test baseline exists. Do not claim code compiles. |

An ephemeral Nix shell for Rust, Cargo, Clang, and pkg-config reported 617.9 MB download and 2.5 GB unpacked, and it was interrupted before completion to avoid an unexpectedly large fetch on this machine. A Rust/Cargo-only retry still estimated 328.7 MB download and 1.1 GB unpacked. No global packages or NixOS configuration were changed.

## Candidate assessment

| Candidate | Maturity/fit | Cost and risk | Decision |
|---|---|---|---|
| Smithay | Rust framework for Wayland compositor backends, protocol helpers, input/session and render abstractions; has nested and DRM-related backends. Used by several real compositors. | API evolves; feature/native dependency graph is large. Requires deliberate feature selection and pinned version. | **Provisional foundation.** Use a released version (0.7 candidate) and keep Veyron policy in project modules. |
| wayland-rs / wayland-server directly | Rust implementation of Wayland protocol bindings; small conceptual surface and direct control. | Leaves more lifecycle, compositor protocol, input, backend and rendering integration to Veyron before basic behavior can be exercised. | Keep as transitive building block via Smithay; direct use only where a concrete gap justifies it. |
| wlroots | Mature compositor ecosystem and useful protocol/backend reference. | Main implementation is C; direct use would put core behavior outside the required Rust implementation and add FFI boundaries. | Reference only; do not adopt as Veyron's core. |
| Smithay nested winit backend | Enables development and testing inside the existing graphical session; current session already provides a running Wayland compositor. | Does not validate direct DRM/KMS, seat/session handoff, VT switching, or standalone startup. | Phase 1 development target, subject to dependency/build verification. |
| Smithay DRM/KMS + udev/session backends | Required path for a standalone compositor session and hardware output management. | Higher integration and recovery risk; capabilities and direct-seat behavior are unverified on target machine. | Later milestone after nested protocol and window behavior are validated. |
| GLES renderer | Smithay exposes GLES rendering; Mesa/i915 is available at device level. Likely the lowest-complexity hardware path for the identified Intel generation. | Actual EGL/GLES support is unqueried. | Initial rendering candidate; gate on runtime query and real surface test. |
| Pixman/software rendering | Useful fallback and deterministic development path on some configurations. | Likely CPU-intensive on this low-power CPU; no target-device measurement. | Keep as a fallback option, not default until measured. |
| Vulkan | Smithay has a Vulkan backend/renderer path. | More dependency and driver complexity than needed for the first visible surface; target support unverified. | Defer unless measured benefits justify it. |
| Tokio/multithreading | Widely used Rust async/runtime options. | No async I/O or parallel rendering requirement identified; extra memory, scheduling, and ownership complexity. | Do not add initially; one event loop is enough. |

## Initial dependency policy

- Pin Smithay to a released version and keep Cargo features limited to the selected nested backend, Wayland frontend, and renderer.
- Avoid Tokio, plugin loaders, Vulkan, and shell crates until a concrete feature needs them.
- Record transitive/native dependency footprint once Cargo metadata is available.
- Use Nix development shells rather than changing the global NixOS configuration.
- Use a single Cargo build job and low debug information on this 4 GiB host. Record build peak memory before deciding whether local full builds are practical.

## Architecture recommendation

Use Smithay for protocol/backend/render plumbing, and keep Veyron's window model independent of Wayland object types. Start with one binary and internal modules. Use one synchronous event loop with explicit state transitions. First build a nested xdg-shell compositor that displays a real client surface. Introduce a protocol-independent layout crate/module only when implementing actual layout behavior; use a sparse representation for the future spatial desktop. Direct DRM/KMS, XWayland, plugins, IPC, shell, animations and multi-monitor support remain later phases.

## Risks and unresolved research

1. **Build tool resource cost:** Rust/Cargo are absent and the compiler closure is large for the machine. Choose an acceptable temporary toolchain acquisition and measure peak compilation memory before compiling all Smithay dependencies.
2. **Intel rendering path:** PCI ID and module suggest HD Graphics 3000, but no GL/EGL query was available. Confirm Mesa driver and GLES context using a functioning diagnostic tool or the first nested renderer initialization.
3. **Smithay release/API choice:** official docs describe release 0.7 and evolving unreleased APIs. Confirm exact required features and example compatibility against the pinned release before implementation.
4. **Nested backend portability:** verify that its winit dependency and Wayland/X11 features compile cleanly on NixOS; do not conflate nested success with direct DRM readiness.
5. **Memory/runtime baseline:** Veyron is not built, so all RAM, CPU and latency goals remain unmeasured.

## Phase 1 entry criteria at the research snapshot

Phase 0 exit criteria were met: environment findings, technology comparison, initial architecture, risks, dependencies to verify, and the roadmap were documented. At that point Phase 1 had a pinned initial Smithay dependency set, temporary toolchain access, and serial builds configured. That prototype was later superseded by the workspace on the current feature branch.

## Sources

- Smithay project overview, scope, system dependencies and compositor users: [README](https://github.com/Smithay/smithay/blob/master/README.md)
- Smithay's compositor design and guidance on release vs Git API: [Getting started](https://github.com/Smithay/smithay/blob/master/GETTING_STARTED.md)
- Rust Wayland tooling overview: [Smithay Handbook](https://smithay.github.io/book/)
- Smithay backend categories and nested/DRM rendering facilities: [backend docs](https://docs.rs/smithay/latest/smithay/backend/)
- Smithay core scope: [crate docs](https://docs.rs/smithay/latest/smithay/)
