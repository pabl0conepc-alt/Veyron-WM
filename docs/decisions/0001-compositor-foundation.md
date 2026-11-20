# ADR 0001: compositor foundation

- Status: accepted for the initial prototype; revisit after Phase 1 measurements.
- Date: 2026-11-19

## Context

Veyron is a new Rust-only Wayland compositor. The repository has no implementation. The development host is NixOS on an i3-2370M with 4 GiB RAM and Intel graphics PCI ID 8086:0116. Rust and Wayland development tools are not installed in the active system profile. The first implementation must be testable without replacing the active graphical session.

## Decision

Use Smithay as the provisional compositor foundation. Build a nested compositor first, using Smithay's Wayland server/protocol abstractions and GLES renderer. Keep window-management and spatial-desktop policy in project-owned Rust modules, independent of Smithay protocol objects. Use one synchronous event loop initially. Begin with the smallest workspace that supports the actual implementation; defer crate splitting until dependency or test boundaries justify it.

Use a released Smithay version (initial candidate 0.7; its crate declares Rust 1.80.1) rather than tracking master. Veyron selects Rust 1.88 for its first toolchain baseline; verify the exact feature set and native dependencies in the first successful build.

## Alternatives considered

- Direct `wayland-server`/wayland-rs: Rust-native, but too low-level for the project's current size; the project would own more protocol, input, and rendering plumbing before it can validate its own window model.
- wlroots: useful as architectural reference and protocol ecosystem, but its implementation is C and direct integration would violate the project's rule that Veyron implementation code be Rust.
- Immediate DRM/KMS backend: necessary for a standalone desktop session later, but raises development and recovery risk before the basic xdg-shell path works. The nested path can exercise real clients safely inside the existing session.
- Tokio/multithreaded architecture: no asynchronous workload has been identified; concurrency would add ownership and memory costs on the target machine.

## Consequences

- Smithay is a significant dependency; keep features narrow and avoid enabling unused backends/protocols.
- Smithay APIs evolve; record the pinned version and update deliberately.
- The nested GLES path does not validate direct DRM/KMS, session switching, hardware cursor planes, or standalone login behavior.
- The project's differentiating window model remains owned and testable independently.
- Resource constraints require serial compilation and measured runtime baselines before enabling expensive effects.

## References

- [Smithay Handbook](https://smithay.github.io/book/)
- [Smithay project guide](https://github.com/Smithay/smithay/blob/master/GETTING_STARTED.md)
- [Smithay README and system dependencies](https://github.com/Smithay/smithay/blob/master/README.md)
- [Smithay backend documentation](https://docs.rs/smithay/latest/smithay/backend/)
- [Smithay crate documentation](https://docs.rs/smithay/latest/smithay/)
