# Project status

Last reviewed: 2026-11-20.

## Current state

- Current base: `feat/headless-calloop-loop` at `c91d39e`; GitHub Actions passed
  format, Clippy, build and tests for this exact revision.
- Current work branch: `codex/finish-roadmap`.
- Phase 0 is `VERIFIED`; Phase 1 and 2 are `TESTING`; Phase 3 is `TESTING`
  pending nested runtime validation; Phase 4 is `IMPLEMENTING`; Phase 5 and 6
  are `TESTING` and `IMPLEMENTING` respectively.
- A headless compositor and real Wayland client integration tests exist. A
  visible nested GLES smoke test on the target desktop has not been recorded.
- The current branch adds sparse two-dimensional workspaces, four-way
  navigation, window transfer, state IPC coordinates and a nested title
  indicator. CI run [44649598532](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44649598532)
  passed for `1fcd322`; nested runtime behavior still needs validation.
- Floating-window keyboard/IPC resize with minimum-size and viewport bounds
  also passes full CI on `e632b52` (run `44650727970`). Pointer drag, popups and
  nested runtime interaction still need validation.
- Floating-window movement by keyboard/IPC, size-preserving and viewport
  bounded, passes all checks on `5ce2aa4` (run `44651966135`).
- The IPC control socket now enforces owner-only `0600` permissions; CI run
  [44651444722](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44651444722)
  passed all checks for `17d8562`.
- Phase 7 (window management depth) is IMPLEMENTING: master-stack/dwindle/auto
  layouts, smart gaps, window rules, scratchpad, geometry memory, a motion
  core, multi-output policy, named layout/motion profiles with `profile
  next|prev` cycling, and window identity wiring are implemented and tested (112
  core tests plus window-feature, multi-output, profile and headless integration
  tests) with control-protocol version 3. The compositor forwards the app id and
  title a client announces on its first commit, so rules and `veyronctl state`
  use real metadata; the renderer still ignores opacity/blur, the backend still
  creates one output, and no nested runtime visual check exists. Gesture and
  overview policy modules were removed before any consumer existed. See
  [ROADMAP.md](ROADMAP.md).
- `cargo test --workspace` now runs in this environment, including the 6 headless
tests that drive the real compositor with genuine Wayland clients, using Wayland
and XKB runtime libraries extracted to a local prefix (no system packages were
installed). That is protocol-level evidence only: no GPU presentation, nested
GLES session or target-hardware measurement was performed.
- Phase 6 Nix packaging is currently uncommitted: `flake.nix`, `flake.lock`, a
  README usage section and a GitHub Actions package job are in the worktree.
  Local flake evaluation returned `veyron-wm-0.1.0`; no complete local build
  was run due to the 4 GiB limit, and the first package CI build is pending.

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
- Latest feature CI run [44651444722](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44651444722)
  passed format, Clippy, build and tests for IPC socket hardening at `17d8562`;
  the latest interaction run [44651966135](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44651966135)
  also passed these checks for floating-window movement at `5ce2aa4`. Do not
  mark a phase verified without its stated runtime evidence.

See [ROADMAP.md](ROADMAP.md), [architecture](docs/ARCHITECTURE.md), and
[known issues](KNOWN_ISSUES.md).
