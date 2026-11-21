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

- Current active branch's latest pushed commit: `70bc7d1`, which corrects
  generated layer-shell anchor names in the integration test.
- Nix runtime library lookup was fixed in `253a901`. Its package tests then
  reached all headless clients, where a parallel filename collision occurred.
- Test-only commit `0eaa47d` added a deterministic repeated-clock regression;
  its standard test step failed as expected. Commit `bf79323` adds an atomic
  process-unique suffix. Run `44931933845` passed the full standard CI job; Nix
  passed headless tests but failed at the custom `target/release/veyron` install
  path. Commit `f9f2105` switches to Cargo's install hook; run `44932470919`
  passed both standard CI and the Nix package job.
- Layer-shell test scaffolding is in `e46bc78` and `70bc7d1`. Run
  `44936140199` passes Format, Clippy, and Build, but both standard and Nix test
  jobs fail the new real-client test: a 24 px top exclusive zone changes the
  toplevel height by 0 px. No compositor/core implementation change has been
  made yet. The next step is a core output work-area API synchronized from
  Smithay `LayerMap::non_exclusive_zone()` on layer commit/map/unmap and resize.
- No PR for `codex/current-main-audit` has been created yet. Once implementation
  and checks are reviewed/green, create a draft PR and leave it unmerged.

## Current implementation checkpoint

- Audit artifacts are being prepared in the active worktree:
  `VEYRON_AUDIT.md`, `VEYRON_FEATURE_MATRIX.md` (187 prompt rows),
  `VEYRON_IMPLEMENTATION_PLAN.md`, `VEYRON_PROGRESS.md`.
- `PROJECT_STATUS.md`, `ROADMAP.md`, `DEVELOPMENT_LOG.md`, and `KNOWN_ISSUES.md`
  are being kept aligned with the latest source and CI evidence.
- The latest feature branch has a real integrated Smithay compositor, headless
  Wayland client tests, CLI/IPC, layer shell, smart workspace, canvas action,
  profiles, motion, overview reporting and session export/import.
- Known gaps are listed in `VEYRON_AUDIT.md` and `VEYRON_PROGRESS.md`. Highest
  code priorities are: apply layer exclusive zones to the
  usable viewport; improve popup/subsurface/layer input and client fullscreen;
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

1. Implement the layer-shell output work-area bridge described above. Keep the
   physical output bounds intact and reflow tiled clients inside the usable
   rectangle. Make the failing real-client regression pass in full CI.
2. Review all 187 feature rows against actual source and tests; refine generic
   file/test/next-action columns before committing the matrix.
3. Continue phases in `VEYRON_IMPLEMENTATION_PLAN.md`; do not call project
   complete while runtime, performance, or listed integration work is open.
4. Commit/push coherent increments on `codex/current-main-audit`; prepare a
   draft PR after CI is green. Never merge without explicit request.
