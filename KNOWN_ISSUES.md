# Known issues and open questions

- The Smithay nested compositor passes `cargo check`, but a full executable build hit host memory pressure and was stopped. No runtime test or real Wayland client presentation is verified.
- The protocol-independent layout/window manager/spatial desktop passes 25 standalone `rustc --test` cases; each workspace owns independent tiling/floating state. These models are not integrated with the compositor's xdg-shell surfaces yet.
- Rust/Cargo and build-time Wayland tools are not in the system PATH; temporary Nix shells provide the Rust compiler and Cargo.
- A full one-job `cargo build` under a 900 MiB RAM / 64 MiB swap process cap drove host free swap down to about 32 MiB during code generation. A second attempt with one codegen unit and a 700 MiB RAM cap reached about 676 MiB compiler RSS while host memory availability fell below 400 MiB. Both were stopped to protect the desktop; partial artifacts remain under `target/`. Wait for more host headroom or find a demonstrably lower-memory build path before retrying.
- GPU was identified only by PCI ID `8086:0116`; actual Mesa driver, OpenGL/EGL version, device node access, and render path have not been queried.
- Smithay is evolving. Version 0.7.0 is pinned; an update policy still needs to be defined.
- Smithay's nested winit backend has utility for development; the final direct DRM/KMS lifecycle and supported-session behavior remain to be designed and tested.
- No performance baseline exists. Do not make RAM, latency, or frame-rate claims yet.
- The initial compositor does not advertise outputs, implement popup placement, clipboard/data-device selection, window rules, or direct DRM/KMS.
