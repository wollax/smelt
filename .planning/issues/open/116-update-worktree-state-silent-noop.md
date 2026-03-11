# update_worktree_state silently no-ops when state file absent

**Area:** orchestrate/executor
**Priority:** Low
**Source:** PR review (Phase 10)

If the worktree state file is missing (e.g., from a failed partial run where the file was never written), the session outcome is never persisted to disk. MergeRunner relies on the state file, so a completed session may be silently excluded from merge. Should at minimum `warn!()`.

**File:** `crates/smelt-core/src/orchestrate/executor.rs:854-856`
