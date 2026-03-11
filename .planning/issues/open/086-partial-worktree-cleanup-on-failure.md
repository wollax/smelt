# No Cleanup for Partially-Created Worktrees on Failure

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Important
**File:** `crates/smelt-core/src/orchestrate/executor.rs` (worktree creation block)

## Description

When worktree creation fails, the session state is updated and dependents are skipped, but there's no attempt to clean up partially-created worktrees. If `WorktreeManager` succeeded partially (e.g., created branch but failed to create working directory), manual cleanup may be needed.
