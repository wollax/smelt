---
phase: 02-worktree-manager
plan: 03
subsystem: core, cli
tags: [worktree-manager, remove, prune, orphan-detection, integration-tests]
requires: [02-02]
provides: [worktree-remove, worktree-prune, orphan-detection, cli-lifecycle]
affects: []
tech-stack:
  added: [dialoguer (cli), which (cli)]
  patterns: [orphan-detection-multi-signal, pid-liveness-posix, confirmation-prompt]
key-files:
  created:
    - crates/smelt-core/src/worktree/orphan.rs
  modified:
    - crates/smelt-core/src/worktree/mod.rs
    - crates/smelt-core/src/lib.rs
    - crates/smelt-cli/Cargo.toml
    - crates/smelt-cli/src/commands/worktree.rs
    - crates/smelt-cli/src/main.rs
    - crates/smelt-cli/tests/cli_integration.rs
decisions: []
metrics:
  duration: ~8m
  completed: 2026-03-09
---

# Phase 02 Plan 03: Remove, Orphan Detection, Prune + Integration Tests Summary

Implemented worktree removal with safety checks, orphan detection via multi-signal analysis (PID liveness, staleness, git cross-reference), and prune operations. Wired up CLI subcommands and added comprehensive integration tests covering the full worktree lifecycle.

## Tasks Completed

### Task 1: Implement remove, orphan detection, and prune in WorktreeManager
- Created `orphan.rs` with `is_pid_alive()` (POSIX `libc::kill(pid, 0)`) and `is_likely_orphan()` with three detection signals: dead PID, stale `updated_at` (24h threshold), and missing git worktree entry
- Only `Running` sessions can become orphaned — `Created`, `Completed`, `Failed`, `Orphaned` are excluded
- Implemented `WorktreeManager::remove()` with full cleanup sequence: check dirty → remove worktree → check merge status → delete branch → remove state file → git worktree prune
- `remove()` returns `RemoveResult` struct tracking what was cleaned up
- `WorktreeDirty` and `BranchUnmerged` errors when `force=false` on dirty/unmerged worktrees
- Implemented `detect_orphans()` reading all state files and cross-referencing with git worktree list
- Implemented `prune()` calling detect_orphans then remove with force=true for each
- Added `pub mod orphan` to mod.rs, exported `RemoveResult` from lib.rs
- 8 unit tests for orphan.rs: PID liveness (current process, large PID), orphan detection for each status and signal combination
- 6 unit tests for remove/detect_orphans in mod.rs: remove lifecycle, dirty without force, dirty with force, nonexistent, orphan detection with dead PID, created sessions ignored

### Task 2: Wire CLI remove/prune and add integration tests
- Added `Remove { name, force, yes }` and `Prune { yes }` variants to `WorktreeCommands`
- `execute_remove()`: handles WorktreeNotFound, WorktreeDirty (with dialoguer confirmation prompt), BranchUnmerged errors; prints removal details on success
- `execute_prune()`: lists orphans, prompts with dialoguer unless `--yes`, prints pruned names
- Added `dialoguer` and `which` as dependencies to smelt-cli
- Wired up Remove and Prune match arms in main.rs
- Added 6 integration tests: create+list, duplicate create, remove lifecycle, wt alias, create-without-init, remove-nonexistent
- All 14 CLI integration tests pass, all 39 unit tests pass (53 total)

## Deviations

- **PID 1 test fix (auto):** Changed `is_pid_alive(1)` test to use `std::process::id()` since signaling PID 1 requires root on macOS.
- **Clippy collapsible-if fix (auto):** Collapsed nested `if let Some(pid) ... if !is_pid_alive(pid)` into single condition with `&&`.

## Verification

- `cargo build --workspace`: clean
- `cargo test --workspace`: 53/53 passed (39 smelt-core, 14 smelt-cli integration)
- `cargo clippy --workspace -- -D warnings`: clean
- Smoke test: `smelt init && smelt wt create test && smelt wt list && smelt wt remove test -y --force && smelt wt list` — full lifecycle works

## Phase Success Criteria

1. **SC-1: Create + list worktrees** — `smelt worktree create test && smelt worktree list` shows worktree (verified in integration test + smoke test)
2. **SC-2: Remove cleans up worktree + branch + state** — `smelt worktree remove test -y --force && smelt worktree list` shows empty (verified in integration test + smoke test)
3. **SC-3: Orphan detection** — Detects orphaned sessions via dead PID, stale timestamp, and git worktree desync (verified in unit tests)
4. **SC-4: Duplicate create error** — `smelt worktree create test && smelt worktree create test` returns clear error (verified in integration test)
