# Worktree State Update Error Handling After Session Completion

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Suggestion
**File:** `crates/smelt-core/src/orchestrate/executor.rs` (update_worktree_state)

## Description

`update_worktree_state` is called after session completion but failures are only warned, not propagated. If the update fails, worktree state may be out-of-sync with run state, potentially causing issues on resume. Consider whether this impacts correctness.
