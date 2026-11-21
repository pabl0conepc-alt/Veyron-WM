# Veyron WM

Veyron is an independent Wayland compositor written in Rust. Its planned identity is a lightweight compositor with its own window-management model and an expandable spatial desktop.

> **Status:** Phase 0 research is documented. The Phase 1 Smithay source passes `cargo check`, but building and running the executable has not been verified. Do not treat Veyron as a usable compositor yet.

## Development

Read [project status](PROJECT_STATUS.md), [architecture](ARCHITECTURE.md), and the [roadmap](ROADMAP.md) before changing implementation direction. The full requested feature inventory is in [VEYRON_FEATURE_MATRIX.md](VEYRON_FEATURE_MATRIX.md), the source audit in [VEYRON_AUDIT.md](VEYRON_AUDIT.md), the dependency-ordered [implementation plan](VEYRON_IMPLEMENTATION_PLAN.md), and the durable [progress handoff](VEYRON_PROGRESS.md). Architecture decisions are recorded in [`docs/decisions/`](docs/decisions/).

The first implementation target is a nested Wayland compositor for development and functional validation. Direct DRM/KMS support is a later milestone. This keeps early iterations inside the existing desktop session and avoids risking the user's only graphical session.

## Development build

The initial target is a nested Wayland session. On NixOS, a temporary toolchain shell can be used without changing the system profile:

```sh
nix shell nixpkgs#rustc nixpkgs#cargo nixpkgs#pkg-config nixpkgs#gcc -c cargo check
nix shell nixpkgs#rustc nixpkgs#cargo nixpkgs#pkg-config nixpkgs#gcc -c cargo run
```

The Cargo configuration limits builds to one job because the inspected development machine has 4 GiB RAM. `cargo check` passed with a temporary Nix toolchain. A full `cargo build` reached severe host memory pressure and was stopped, so `cargo run` is not yet a verified recipe.

The compositor uses socket `veyron-0` by default. To connect a client from another terminal while it runs, set `WAYLAND_DISPLAY=veyron-0` for that client. Set `VEYRON_WAYLAND_DISPLAY` before launch to choose another socket name. The nested compositor itself uses the current desktop session to open its window.

The protocol-independent layout, window, and spatial desktop models have a small standalone test target that avoids building Smithay while the netbook is memory constrained:

```sh
nix shell nixpkgs#rustc nixpkgs#gcc -c sh -c 'rustc --edition=2024 --test src/window/mod.rs -o /tmp/veyron-window-tests && /tmp/veyron-window-tests'
```

## Scope

Veyron's product goals include an independent window manager, tiling and floating layouts, spatial desktop navigation, configurable behavior, and precise compatibility documentation. These are goals, not claims of current support. See [roadmap](ROADMAP.md) for state and acceptance criteria.
