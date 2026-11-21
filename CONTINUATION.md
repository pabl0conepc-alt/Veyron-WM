# Veyron WM durable continuation

Last updated: 2026-11-21. Read this file together with the user's current
engineering prompt and `VEYRON_PROGRESS.md` before resuming.

## User instructions

- Work only on **Veyron-WM**.
- The repository was updated by another agent; inspect and preserve the latest
  tree. Do not assume it matches prior work.
- Follow the complete audit/development/implementation prompt. Do not stop at
  documents; continue implementing, testing, and integrating.
- The target netbook has 4 GiB RAM; keep work resource-conscious.
- **No agents.** The user explicitly corrected this. Continue solo.
- Save progress in the repository and state exactly where work stopped.
- Prepare a draft PR when work is ready; do not merge without the user's request.

## Authoritative repository

- Active checkout: `/home/pablo/Documents/Codex/2026-11-19/lsi/outputs/Veyron-WM-current-main`
- Branch: `codex/current-main-audit`
- Base inspected: fetched `origin/main` commit `ed18b75`, a three-crate Rust
  workspace (`veyron-core`, `veyron-ipc`, `veyron`).
- This is the current updated project. Continue here; do not transplant old flat
  `src/main.rs` source or its unfinished output scaffold from
  `/home/pablo/Documents/Codex/2026-11-19/lsi/outputs/Veyron-WM`.
- Older checkouts remain preserved: `/outputs/Veyron-WM` on
  `codex/engineering-audit`, and `/outputs/Veyron-WM-finish` on
  `codex/finish-roadmap`. Do not reset, merge, or copy them into this tree.
- Git remote: `https://github.com/pabl0conepc-alt/Veyron-WM.git`.

## Branch and CI

- Current active branch's latest pushed code commit: `85e5754`, which reserves
  layer-shell exclusive zones in the usable output work area.
- Nix runtime library lookup was fixed in `253a901`. Its package tests then
  reached all headless clients, where a parallel filename collision occurred.
- Test-only commit `0eaa47d` added a deterministic repeated-clock regression;
  its standard test step failed as expected. Commit `bf79323` adds an atomic
  process-unique suffix. Run `44931933845` passed the full standard CI job; Nix
  passed headless tests but failed at the custom `target/release/veyron` install
  path. Commit `f9f2105` switches to Cargo's install hook; run `44932470919`
  passed both standard CI and the Nix package job.
- Layer-shell test scaffolding is in `e46bc78`, `70bc7d1` and `353eb1f`.
  Run `44937098387` proved the bug: a mapped 24 px top exclusive zone changed
  the toplevel height by 0 px. Commit `85e5754` adds a separate core output
  work area and synchronizes it from Smithay's `non_exclusive_zone()` after
  layer changes and output resizing. Run `44937377591` passes both standard
  and Nix jobs, including the real-client regression.
- No PR for `codex/current-main-audit` has been created yet. Once implementation
  and checks are reviewed/green, create a draft PR and leave it unmerged.

## Current implementation checkpoint (`85e5754`, CI green)

- Audit artifacts are being prepared in the active worktree:
  `VEYRON_AUDIT.md`, `VEYRON_FEATURE_MATRIX.md` (187 prompt rows),
  `VEYRON_IMPLEMENTATION_PLAN.md`, `VEYRON_PROGRESS.md`.
- `PROJECT_STATUS.md`, `ROADMAP.md`, `DEVELOPMENT_LOG.md`, and `KNOWN_ISSUES.md`
  are being kept aligned with the latest source and CI evidence.
- The latest feature branch has a real integrated Smithay compositor, headless
  Wayland client tests, CLI/IPC, layer shell, smart workspace, canvas action,
  profiles, motion, overview reporting and session export/import.
- Known gaps are listed in `VEYRON_AUDIT.md` and `VEYRON_PROGRESS.md`. Highest
  code priorities are: improve popup/subsurface/layer input and client fullscreen;
  reconcile session restore to actual clients; fix reload error response and
  on-wire IPC versioning/concurrency cap.

## Constraints and verification

- Shell currently has Nix but not Cargo or rustc. Do not fetch a large compiler
  closure onto the netbook; use GitHub Actions for full build/test/Clippy/fmt.
- Avoid parallel local builds and unmeasured claims about CPU/RAM/GPU/frame rate.
- Target documented as NixOS, x86_64, Plasma Wayland, i3-2370M, 4 GiB RAM,
  likely Intel HD Graphics 3000. The exact EGL/driver capability is unverified.
- Headless Wayland-client integration proves protocol/state behavior, not nested
  GLES presentation. No target nested visual smoke/performance measurement is
  recorded.

## Exact next steps

1. Review all 187 feature rows against actual source and tests; refine generic
   file/test/next-action columns before committing the matrix.
2. Continue phases in `VEYRON_IMPLEMENTATION_PLAN.md`; first address compositor
   input routing for popup/subsurface/layer surfaces and client fullscreen,
   adding focused real-client regressions for each behavior.
3. Fix IPC reload error reporting, wire-level version negotiation and per-peer
   resource limits, with tests.
4. Reconcile Recall restore state with later Wayland client surfaces; test
   actual client matching and document application limitations.
5. Fix sparse overview coordinate/size overflow with boundary tests.
6. Do not call project complete while runtime, performance, or listed
   integration work is open.
7. Commit/push coherent increments on `codex/current-main-audit`; prepare a
   draft PR after CI is green. Never merge without explicit user request.
