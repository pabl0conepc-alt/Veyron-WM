# Veyron WM persistent progress

Last updated: 2026-11-21

## Current task state

- **Current prompt:** complete engineering audit/development/implementation prompt supplied in the attachment referenced by this task.
- **Current repository:** `/home/pablo/Documents/Codex/2026-11-19/lsi/outputs/Veyron-WM`, branch `main`, commit `1338ebb` (`Initial commit`). This is the current user-selected tree.
- **Do not confuse with:** `/home/pablo/Documents/Codex/2026-11-19/lsi/outputs/Veyron-WM-finish`, older branch `codex/finish-roadmap` at `70cdf85` and PR #1. Do not copy/reset/merge it into this worktree automatically.
- **Working tree preservation:** README.md is modified; `.cargo/`, `.gitignore`, Cargo.toml/lock, source and project docs are untracked in the repository. These are existing changes supplied with the updated repo. Preserve them and do not clean/reset.
- **Last measured task action:** source/docs inventory and matrix generation. A test invocation tried `nix shell nixpkgs#rustc nixpkgs#gcc`; Nix estimated 558.5 MiB download and 1.9 GiB unpacked, so the command was interrupted before compilation to protect the 4 GiB netbook.

## Current phase

**Phase 1 — Repository discovery, feature matrix and implementation plan.** The current code and documentation audit is recorded in `VEYRON_AUDIT.md`; all 187 requested feature rows are in `VEYRON_FEATURE_MATRIX.md`; dependency ordering is in `VEYRON_IMPLEMENTATION_PLAN.md`. Continue next with a lightweight model test using an already-present Rust toolchain if one exists, or CI/remote build, then start the core surface/output integration.

## Implemented / evidence-backed

- Pure split-tree layout, tiled/floating state manager, and sparse signed 2D workspace model exist in `src/window/`.
- Historical project docs report 10 layout + 6 window-manager + 9 workspace tests passed (25 total), and Smithay `cargo check` succeeded in an earlier execution.
- A nested Smithay/winit/GLES code path and xdg-shell/SHM/seat setup are present in `src/main.rs`; source presence alone does not establish successful build/runtime.
- Phase 0 machine/backend research and ADR 0001 exist.
- Nothing in the 187-item feature matrix is marked `IMPLEMENTED` in this snapshot because real compositor integration and proof are absent.

## In development / incomplete

- `src/main.rs` creates/configures xdg toplevel surfaces but does not map them into `WindowManager` or `SpatialDesktop`.
- No `wl_output` global/output model, window placement/configure adapter, workspace actions, functioning focus/pointer delivery, config, IPC, persistence or CI/package found in this tree.
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

## Changed/created this task

- Created `VEYRON_AUDIT.md` with repository/architecture evidence and integration risks.
- Created `VEYRON_FEATURE_MATRIX.md` with 187 requested features and explicit states/gaps/tests/actions.
- Created `VEYRON_IMPLEMENTATION_PLAN.md` with prerequisite-ordered work packages.
- This file records the stop point and resumption instructions.
- Existing code/doc files were not edited by this audit step.

## Known blockers/constraints

- Low-memory host and large Rust/Nix toolchain closure; local full codegen can jeopardize desktop stability.
- Current environment PATH has `nix` but not `cargo` or `rustc`; an attempted Nix shell was stopped before download completion.
- Smithay surface/runtime APIs must be verified against exactly pinned 0.7.0 source/API.
- Actual target GLES/EGL, output mode, input-device and multi-monitor capabilities are not queried.
- No committed baseline for the current untracked source/doc tree; reviewability/backup is needed before any destructive Git action.

## Exact next executable work

1. Check whether a Rust compiler/toolchain is already present in `/nix/store` without fetching new dependencies. If not, use a remote CI/build route instead of another large local shell fetch.
2. Verify the 25 pure model tests and formatting on the current files; record the exact commands/results and update this log.
3. Verify the current Smithay 0.7 API and implement the first integrated foundation: an output description plus stable registry connecting mapped `ToplevelSurface`s to model IDs, arrangement/configure, and surface teardown. Add real-client integration evidence only when the executable builds.
4. Keep feature statuses truthful and update the audit/matrix/plan/progress together after each milestone.
5. Do not merge the older PR into this worktree without reconciling the two diverged trees and preserving all current uncommitted files.

- Added `.github/workflows/ci.yml` on branch `codex/engineering-audit` as the first build-safety increment. GitHub-hosted CI will run formatting, Clippy, all-target tests, and a release build outside the memory-constrained netbook. It has not run yet because this branch has not been pushed.
