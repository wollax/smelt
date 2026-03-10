# Phase 04 Plan 03: CLI Merge Command + Integration Tests Summary

**One-liner:** `smelt merge <manifest>` CLI command with progress/summary output + 7 end-to-end integration tests proving the full session-run-then-merge pipeline.

## Frontmatter

- **Phase:** 04-sequential-merge
- **Plan:** 03
- **Subsystem:** cli-merge
- **Tags:** cli, merge, integration-tests, end-to-end
- **Completed:** 2026-03-10
- **Duration:** ~5 minutes

### Dependencies

- **Requires:** 04-02 (MergeRunner), 04-01 (git merge primitives)
- **Provides:** `smelt merge` CLI command, end-to-end integration test suite
- **Affects:** Phase 5+ (merge order intelligence, conflict resolution)

### Tech Stack

- **Added:** None
- **Patterns:** CLI follows session.rs pattern — load manifest, run engine, format output, return exit code

### Key Files

- **Created:**
  - `crates/smelt-cli/src/commands/merge.rs` — CLI merge command handler (98 lines)
  - `crates/smelt-cli/tests/cli_merge.rs` — 7 integration tests (332 lines)
- **Modified:**
  - `crates/smelt-cli/src/commands/mod.rs` — `pub mod merge;` declaration
  - `crates/smelt-cli/src/main.rs` — Merge variant in Commands enum + match arm
  - `crates/smelt-core/src/session/runner.rs` — State file updates after session execution

### Decisions

| Decision | Rationale |
|----------|-----------|
| Post-hoc progress from MergeReport | MergeRunner has no callback mechanism; printing after completion is simplest for Phase 4 |
| Pattern-match specific SmeltError variants | Each merge error type gets tailored user-facing output (conflict shows file list, etc.) |
| SessionRunner updates WorktreeState status | MergeRunner depends on Completed status to filter sessions; was missing from Phase 3 |

## Tasks Completed

### Task 1: Add `smelt merge` CLI command

- Created `merge.rs` with `execute_merge()` function
- Progress output to stderr: `[1/N] Merged '<session>'`, `Merged N session(s) into '<branch>'`
- Summary output to stdout: per-session diff stats (files changed, insertions, deletions)
- Error handling: MergeConflict (file list), MergeTargetExists, NoCompletedSessions, NotInitialized, SessionError
- Exit code 0 on success, 1 on merge-related errors
- Added `pub mod merge;` to commands/mod.rs
- Added `Merge { manifest, target }` variant to Commands enum with `--target` long arg
- **Commit:** 5cfff5a

### Task 2: End-to-end integration tests

- 7 integration tests covering the full pipeline (session run + merge):
  - `test_merge_clean_two_sessions` — clean merge of 2 non-overlapping sessions
  - `test_merge_conflict_exits_with_error` — conflict detection with rollback
  - `test_merge_with_custom_target` — `--target` flag overrides branch name
  - `test_merge_target_exists_error` — pre-existing target branch rejected
  - `test_merge_no_sessions_run` — no state files → no completed sessions error
  - `test_merge_manifest_not_found` — nonexistent manifest file → error
  - `test_merge_three_sessions_one_failed` — 2 clean + 1 crashed → merges 2, skips 1
- **Commit:** 4dcb5af

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical Functionality] SessionRunner not updating WorktreeState status**

- **Found during:** Task 2 (all merge tests failed — sessions stuck in `Created` status)
- **Issue:** `SessionRunner::run_manifest()` never updated the `.smelt/worktrees/<name>.toml` state file after session execution. Status remained `Created`, causing MergeRunner to skip all sessions.
- **Fix:** Added state file update logic after each session: maps `SessionOutcome::Completed` → `SessionStatus::Completed`, all others → `SessionStatus::Failed`. Uses warn-level logging on failures.
- **Files modified:** `crates/smelt-core/src/session/runner.rs`
- **Commit:** 4dcb5af

## Verification

- [x] `cargo build --workspace` compiles cleanly
- [x] `cargo test --workspace` — 118 tests pass
- [x] `cargo clippy --workspace -- -D warnings` — clean
- [x] `smelt merge --help` displays correct usage
- [x] End-to-end: `smelt session run` then `smelt merge` produces merged branch
- [x] Conflict scenario: merge exits 1, target branch cleaned up
- [x] Summary output shows per-session diff stats
- [x] 7 integration tests covering happy path and error cases
- [x] `--target` flag works correctly
- [x] Skipped sessions reported in stderr

## Metrics

| Metric | Value |
|--------|-------|
| Tasks | 2/2 |
| Tests added | 7 |
| Tests total | 118 |
| Lines added (merge.rs) | 98 |
| Lines added (cli_merge.rs) | 332 |
| Lines added (mod.rs) | 1 |
| Lines added (main.rs) | 12 |
| Lines modified (runner.rs) | 25 |
| Artifact min_lines met | Yes (98/80, 5/5, 153/145, 332/150) |

## Phase 4 Completion

Phase 04 (Sequential Merge) is now complete. All 3 plans delivered:
- **04-01:** Git primitives (merge_base, merge_squash, branch_create, etc.)
- **04-02:** MergeRunner core engine with rollback and cleanup
- **04-03:** CLI command + end-to-end integration tests

The full workflow is operational: `smelt init` → `smelt session run manifest.toml` → `smelt merge manifest.toml`.
