# Project status

Last verified: 2026-11-19

## Current state

- Repository: initial commit only before this Phase 0 documentation.
- Current activity: Phase 1 nested compositor implementation and staged Phase 2/3 integration.
- Status: Phase 0 is `VERIFIED`; Phase 1 is `IMPLEMENTING` (Cargo type-check passes; executable build and runtime are not verified); Phase 2 and Phase 3 models pass standalone tests but are not integrated.

## Development environment inspected

- NixOS 26.11, Linux 6.18.63, x86_64.
- Intel Core i3-2370M, 2 cores / 4 threads, 4 GiB RAM, approximately 1.5 GiB available at inspection.
- PCI display device `8086:0116`, consistent with Intel HD Graphics 3000 (Sandy Bridge); this identification comes from the PCI ID and is not a live GPU renderer/driver query.
- Current session is KDE Plasma on Wayland. KWin and XWayland are running.
- `nix` 2.35.6 is available. `rustc`, `cargo`, `rustup`, `pkg-config`, `clang`, `lspci`, and `wayland-info` were not available on PATH at inspection.
- Rust 1.99.1, Cargo 1.99.0, pkg-config, and a C compiler are available through temporary Nix shells; no system configuration was changed.
- Cargo resolved 216 packages. A temporary Nix toolchain with GCC and serial compilation successfully ran `cargo check` under a 900 MiB memory / 64 MiB swap cgroup cap. The check validated the Smithay 0.7 application after correcting a Winit name collision, render-element inference, seat mutability, and a borrow conflict. `rustfmt --check` also passes.
- A full `cargo build` was attempted under the same cap. The host reached about 32 MiB free swap during native code generation, so the build was stopped to protect the desktop. The executable has not been produced or run; Phase 1 is therefore still unverified. Partial build artifacts are retained in `target/`.
- A second attempt used one codegen unit with a 700 MiB RAM / 96 MiB swap cap. It was stopped when the compiler reached about 676 MiB RSS and host available memory fell below 400 MiB. This did not complete the build.
- The protocol-independent layout, window-manager, and spatial-desktop modules have 25 standalone Rust unit tests passing with warnings denied. These tests do not validate the Smithay compositor binary or its Wayland behavior.

## Constraints and next action

Keep compilation serial and avoid debug-heavy builds on the 4 GiB machine. Use temporary Nix shells; do not install a global toolchain. `cargo check` succeeds, but two guarded executable-build attempts caused severe host memory pressure. Phase 1 must not be considered functional until the binary builds and displays a real client surface. Further full-build attempts should wait for more host headroom or a demonstrably lower-memory build path. Do not move to direct DRM/KMS until the nested path is validated.

## Evidence required to change status

Phase 0 is `VERIFIED`: the host inspection, dependency/backend assessment, architecture, risks, and phase acceptance plan are documented. The first successful build baseline belongs to Phase 1. Phase 1 requires a reproducible build and an exercised compositor session displaying a real client surface; a scaffold or successful `cargo check` alone is not completion.
