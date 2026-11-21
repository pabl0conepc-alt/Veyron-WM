# Veyron WM persistent progress

Last updated: 2026-11-21

## Current task state

- **Current prompt:** complete engineering audit/development/implementation prompt supplied in the attachment referenced by this task.
- **Current repository:** `/home/pablo/Documents/Codex/2026-11-19/lsi/outputs/Veyron-WM`, branch `codex/engineering-audit`, commit `4577bfd` (`test: fix desktop overflow test lint`), pushed to `origin`. Draft PR [#2](https://github.com/pabl0conepc-alt/Veyron-WM/pull/2).
- **Do not confuse with:** `/home/pablo/Documents/Codex/2026-11-19/lsi/outputs/Veyron-WM-finish`, older branch `codex/finish-roadmap` at `70cdf85` and PR #1. Do not copy/reset/merge it into this worktree automatically.
- **Working tree preservation:** the pre-existing README/source/docs from the updated checkout were captured in commit `c3963c2`; do not clean/reset or replace this branch with the older checkout.
- **Last measured task action:** source/docs inventory and matrix generation. A test invocation tried `nix shell nixpkgs#rustc nixpkgs#gcc`; Nix estimated 558.5 MiB download and 1.9 GiB unpacked, so the command was interrupted before compilation to protect the 4 GiB netbook.

## Current phase

**Phase 2 — Wayland output/protocol foundation.** Discovery, audit, matrix (187 feature rows), and dependency plan are recorded. GitHub Actions run 44922775244 passed format, Clippy, all 25 tests, and a release build. Implement the nested virtual output with a valid `wl_output` global/mode/scale and resize updates, then map xdg toplevels into the window/workspace model.

## Implemented / evidence-backed

- Pure split-tree layout, tiled/floating state manager, and sparse signed 2D workspace model exist in `src/window/`.
- Historical project docs report 10 layout + 6 window-manager + 9 workspace tests passed (25 total), and Smithay `cargo check` succeeded in an earlier execution.
- A nested Smithay/winit/GLES code path and xdg-shell/SHM/seat setup are present in `src/main.rs`; source presence alone does not establish successful build/runtime.
- Phase 0 machine/backend research and ADR 0001 exist.
- Nothing in the 187-item feature matrix is marked `IMPLEMENTED` in this snapshot because real compositor integration and proof are absent.

## In development / incomplete

- `src/main.rs` creates/configures xdg toplevel surfaces but does not map them into `WindowManager` or `SpatialDesktop`.
- No `wl_output` global/output model, window placement/configure adapter, workspace actions, functioning focus/pointer delivery, config, IPC, persistence or package found in this tree. CI workflow was added in this task.
- Render loop redraws the whole output and waits synchronously each iteration; no performance measurements exist.
- Executable build and nested real-client smoke remain unverified; two earlier builds were stopped under memory pressure according to project records.

## Test/validation ledger

| Check | Result | Scope / caveat |
|---|---|---|
| Historical `cargo check` | Passed per current `PROJECT_STATUS.md` | Prior source state; not rerun against this audit snapshot. |
| Historical standalone pure Rust tests | 25 passed per current `PROJECT_STATUS.md` | Model-only; not compositor integration. |
| Current standalone tests | Not run | Nix toolchain fetch estimated 558.5 MiB download/1.9 GiB unpacked and was interrupted before rustc started. |
| Full executable build | Not verified | Prior attempts were stopped due memory/swap pressure. |
| Nested compositor / real client | Not verified | No executable/runtime evidence. |
| CI run 44921522852 | Failed at Clippy | `clippy::while-let-loop` in `src/main.rs:188`; format passed; test and release build steps were skipped. Fixed in `73c18c2`. |
| CI run 44921786860 | Failed at Format | Rustfmt requested standard indentation after the `while let` loop conversion; Clippy/tests/build were skipped. Formatting correction is in progress. |
| CI run 44921896407 | Failed at Clippy | `field-reassign-with-default` in the overflow test at `src/window/desktop.rs:458`; format passed; test and release build were skipped. Test fixture correction is in progress. |
| CI run 44922775244 | Passed | Format, Clippy, `cargo test --locked --all-targets` (25 tests), and `cargo build --locked --release`; GitHub-hosted Ubuntu runner. No nested runtime evidence. |

## Changed/created this task

- Created `VEYRON_AUDIT.md` with repository/architecture evidence and integration risks.
- Created `VEYRON_FEATURE_MATRIX.md` with 187 requested features and explicit states/gaps/tests/actions.
- Created `VEYRON_IMPLEMENTATION_PLAN.md` with prerequisite-ordered work packages.
- This file records the stop point and resumption instructions.
- Preserved the updated current tree, audit docs and CI in `c3963c2`; fixed CI lint/format issues in `73c18c2`, `b76474b`, and `4577bfd`; created draft PR #2.

## Known blockers/constraints

- Low-memory host and large Rust/Nix toolchain closure; local full codegen can jeopardize desktop stability.
- Current environment PATH has `nix` but not `cargo` or `rustc`; an attempted Nix shell was stopped before download completion.
- Smithay surface/runtime APIs must be verified against exactly pinned 0.7.0 source/API.
- Actual target GLES/EGL, output mode, input-device and multi-monitor capabilities are not queried.
- The current baseline is now captured on a feature branch; keep changes on this branch and do not merge without review.

## Exact next executable work

1. Add a real Smithay virtual `Output` and advertise the nested backend's current output mode/scale using Smithay 0.7's `OutputManagerState`/`delegate_output!`; synchronize mode on resize.
2. Add test coverage for output state and, if feasible without a display, a Wayland client protocol test; verify with GitHub Actions.
3. Integrate `ToplevelSurface` lifecycle with stable `WindowId`s and model geometry; configure mapped clients from the layout and clean up on destruction.
4. Add nested smoke evidence from the target desktop after an executable is available; do not claim Phase 1 complete before a real surface is seen.
5. Keep feature statuses truthful and update all four persistent docs at each milestone. Keep separate from older PR #1; do not merge without review.

- Added `.github/workflows/ci.yml` on branch `codex/engineering-audit` as the build-safety gate. Run 44922775244 passed formatting, Clippy, all 25 model tests, and a release build on Ubuntu. The two lint failures and one formatter failure in preceding runs were corrected without disabling checks.
