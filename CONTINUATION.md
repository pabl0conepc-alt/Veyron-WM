# Veyron-WM project handoff

Last updated: 2026-11-19

This file is the durable continuation note for resuming the user's Veyron-WM
work if the chat context or token budget ends. Read it together with the
attached master prompt at `/home/pablo/.codex/attachments/9a1b77f4-c88c-45f2-9ee2-e1609ff0763c/Texto colado.txt`.

## User instructions

- Work only on **Veyron-WM**.
- Follow the attached prompt and its phases as written. Preserve the low-resource
  design because the target netbook has 4 GiB RAM; do not add unrelated features.
- Continue the remaining phases. Make meaningful commits and prepare a PR; do
  not merge it without the user's request.
- The user said **do not execute agents**. Continue directly without spawning
  sub-agents.
- Keep this handoff, `ROADMAP.md`, `PROJECT_STATUS.md`, and `DEVELOPMENT_LOG.md`
  current as work advances.

## Repository and git state

- GitHub repository: `pabl0conepc-alt/Veyron-WM`
- Active worktree: `/home/pablo/Documents/Codex/2026-11-19/lsi/outputs/Veyron-WM-finish`
- Active branch: `codex/finish-roadmap`
- Draft PR: [#1](https://github.com/pabl0conepc-alt/Veyron-WM/pull/1), base
  `main`; it is attached to this Codex task. Keep it updated while the remaining
  phases advance; do not merge without the user's request.
- The old scaffold at `/home/pablo/Documents/Codex/2026-11-19/lsi/outputs/Veyron-WM`
  is preserved and is not the active development tree.
- Git identity is set to `Pablo Arraes` and the GitHub-linked noreply address
  `327252850+pabl0conepc-alt@users.noreply.github.com`.

Recent commits on the active branch:

- `9c1776e docs: preserve initial host and backend research`
- `68f4379 docs: map Veyron against phased acceptance criteria`
- `6bcc15a perf: make nested rendering event and damage driven`
- `1fcd322 feat: add sparse 2D workspace navigation`
- `92caf72 docs: record 2D workspace CI result`
- `97491c2 feat: add bounded floating window resize actions`
- `92e5b27 style: apply Rust formatting to resize actions`
- `e632b52 test: cover floating resize maximum height`
- `c97fb8d docs: record XWayland integration boundary`
- `17d8562 security: restrict control socket permissions`
- `cf2157f feat: add bounded floating window move actions`
- `5ce2aa4 style: format floating move actions`

The branch is pushed to `origin`. CI run
[44649598532](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44649598532)
for `1fcd322` passed format, Clippy, build and tests. CI for `6bcc15a` also
passed these checks. CI run
[44650727970](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44650727970)
also passed format, Clippy, build and tests for floating resize at `e632b52`.
CI run
[44651444722](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44651444722)
passed the same checks for owner-only socket permissions at `17d8562`.
CI run
[44651966135](https://github.com/pabl0conepc-alt/Veyron-WM/actions/runs/44651966135)
passed format, Clippy, build and tests for floating-window movement at `5ce2aa4`.

## Project design and machine constraints

- Rust workspace: `veyron-core`, `veyron-ipc`, `veyron`; Smithay 0.7 and calloop.
- Pure window-management policy lives in `veyron-core`; the compositor and real
  Wayland protocol integration live in `veyron`.
- Target: NixOS / KDE Plasma Wayland, Intel Core i3-2370M, 4 GiB RAM, likely
  Intel HD Graphics 3000. Full Smithay builds on the laptop previously caused
  memory pressure. Prefer serial, scoped local checks; use GitHub Actions for
  full build/test/clippy/fmt validation. Do not launch uncontrolled parallel
  builds.
- At the time of the last work session, `cargo`/`rustfmt` were absent from this
  shell's PATH. `git diff --check` was available and passed before the latest
  commit. Check the current toolchain before deciding local commands.

## Phase status and remaining acceptance work

- **Phase 0 — Environment/research: VERIFIED.** See `docs/PHASE0_RESEARCH.md`
  and `docs/decisions/0001-compositor-foundation.md`.
- **Phase 1 — Foundation: TESTING.** Smithay nested compositor and headless
  real-client tests exist. Still run the nested GLES compositor in the target
  Plasma session and verify a visible real app surface.
- **Phase 2 — Window management: TESTING.** Layout, focus, floating/fullscreen,
  sizing, floating keyboard/IPC move+resize and protocol-level
  configure/activation are implemented. Both preserve floating size/constraints
  and stay within the viewport. CI passed for resize at `e632b52` and for the
  combined move/resize path at `5ce2aa4`. Still exercise move/resize, popup
  interactions and floating geometry with real clients in the nested session;
  pointer-driven move/resize is not implemented.
- **Phase 3 — Infinite Desktop: TESTING.** Commit `1fcd322` adds sparse signed
  2D workspace coordinates, four-way navigation, lazy cell creation, window
  transfer, bindings, IPC coordinates (protocol version 2), CLI output and a
  nested window-title coordinate indicator. CI passed including core and
  headless real-client tests. Still validate the nested runtime; do not mark
  verified until its acceptance evidence exists.
- **Phase 4 — Rendering/animations: IMPLEMENTING.** Latest previous commit
  registers the Wayland display fd in calloop, removes nested periodic polling,
  and uses Smithay output damage tracking with safe full-redraw fallback. CI
  passed on `6bcc15a`. Still validate partial damage and idle wakeups in nested
  runtime, measure idle/client-load CPU, RSS and frames on target hardware, then
  consider only optional disableable animations if justified.
- **Phase 5 — Configuration/ecosystem: TESTING.** Strict TOML config/hot reload,
  versioned bounded Unix IPC, actions/state/reload/ping/shutdown exist. Still
  integrate XWayland following [ADR 0002](docs/decisions/0002-xwayland-integration-boundary.md),
  document portal boundary and external shell API/compatibility, and keep
  dynamic in-process plugins deferred until safe isolation/permissions are
  designed.
- **Phase 6 — Stabilization: IMPLEMENTING.** CI format/clippy/build/tests and
  headless integration exist. Still add a nested smoke where practical, target
  hardware measurements, broader security/dependency/license review,
  compatibility and unsupported-feature docs, and reproducible install/package
  path after runtime validation. The IPC socket now has owner-only `0600`
  permissions with a passing filesystem-mode regression test.

Full acceptance language is in `ROADMAP.md`; keep its statuses truthful.
Headless protocol tests do not count as visible nested-runtime validation.

## Immediate next actions

1. Continue Phase 5 by implementing the XWayland/XWM adapter only after checking
   that the required Smithay 0.7 APIs can be built and tested without changing
   the low-memory default runtime path.
2. Refresh `PROJECT_STATUS.md`, `ROADMAP.md`, `DEVELOPMENT_LOG.md` and this file
   after each meaningful phase milestone.
3. Continue with the smallest prompt-required phase deliverable that can be
   validated without stressing the laptop. Prefer meaningful commits; avoid
   artificial commit splitting.
4. Keep the PR updated as phases advance. Do not claim GitHub contribution
   credit before merge.

## Evidence and project files

- `ROADMAP.md` — phase-by-phase acceptance criteria and current state.
- `PROJECT_STATUS.md` — concise project/machine snapshot.
- `DEVELOPMENT_LOG.md` — dated implementation and validation history.
- `KNOWN_ISSUES.md` — limitations and open issues.
- `PERFORMANCE.md` — measurement plan; do not invent benchmark numbers.
- `docs/ARCHITECTURE.md` — implementation map and boundaries.
