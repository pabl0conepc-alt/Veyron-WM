# Project status

Last verified: 2026-11-21

## Current state

- Repository baseline: `main` at `1338ebb`; updated source and documentation preserved on `codex/engineering-audit` through `4577bfd`, draft PR #2.
- Current activity: audit complete; Phase 1 build/test gate passes in GitHub Actions; output and real surface integration remain next.
- Status: Phase 0 is `VERIFIED`; Phase 1 remains `IMPLEMENTING` (format, Clippy, 25 tests and release build pass remotely; nested runtime/client surface unverified); Phase 2 and Phase 3 models exist but remain disconnected from real Wayland surfaces.

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
- GitHub Actions run [44922775244](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44922775244) passed format, Clippy, all-target tests (25 model tests), and `cargo build --locked --release` on the current audited branch. This confirms remote compilation, not target runtime behavior.

## Constraints and next action

Keep compilation serial and avoid debug-heavy builds on the 4 GiB machine. Use temporary Nix shells; do not install a global toolchain. CI now proves the release binary builds remotely. Earlier local full-build attempts caused severe memory pressure. Phase 1 must not be considered functionally verified until Veyron starts nested and displays a real client surface. Do not move to direct DRM/KMS until the nested path is validated.

## Evidence required to change status

Phase 0 is `VERIFIED`: the host inspection, dependency/backend assessment, architecture, risks, and phase acceptance plan are documented. The first successful build baseline belongs to Phase 1. Phase 1 requires a reproducible build and an exercised compositor session displaying a real client surface; a scaffold or successful `cargo check` alone is not completion.
