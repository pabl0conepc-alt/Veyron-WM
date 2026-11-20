# Project status

Last reviewed: 2026-11-19.

## Current state

- Current base: `feat/headless-calloop-loop` at `c91d39e`; GitHub Actions passed
  format, Clippy, build and tests for this exact revision.
- Current work branch: `codex/finish-roadmap`.
- Phase 0 is `VERIFIED`; Phase 1 and 2 are `TESTING`; Phase 3 and 4 are
  `IMPLEMENTING`; Phase 5 and 6 are `TESTING` and `IMPLEMENTING` respectively.
- A headless compositor and real Wayland client integration tests exist. A
  visible nested GLES smoke test on the target desktop has not been recorded.
- The current desktop is an unbounded horizontal strip. Two-dimensional spatial
  workspaces required by the master prompt are not implemented yet.

## Target environment and constraints

- NixOS 26.11, Linux 6.18.63, x86_64; KDE Plasma Wayland session.
- Intel Core i3-2370M (2 cores / 4 threads), 4 GiB RAM; display PCI ID `8086:0116`
  suggests Intel HD Graphics 3000, but Mesa/EGL capabilities have not yet been
  directly queried.
- Keep local builds serial. Previous full Smithay builds in the older prototype
  caused memory pressure. The current branch's full CI build/test succeeded on a
  GitHub-hosted runner; the local compositor still requires a guarded runtime
  validation.

## Evidence and next work

- CI run [44437184079](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44437184079)
  succeeded for `c91d39e`, including `cargo build --workspace --locked`,
  `cargo test --workspace --locked`, Clippy and rustfmt.
- The headless suite uses genuine Wayland clients and checks configure sizes,
  activation, mapping, workspace scrolling and clean shutdown.
- Continue with event-driven nested scheduling and Smithay damage tracking,
  then implement sparse two-dimensional workspaces and their protocol/IPC
  integration. Do not mark a phase verified without its stated runtime evidence.

See [ROADMAP.md](ROADMAP.md), [architecture](docs/ARCHITECTURE.md), and
[known issues](KNOWN_ISSUES.md).
