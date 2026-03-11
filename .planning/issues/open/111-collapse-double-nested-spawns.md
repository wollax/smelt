# Collapse double-nested spawns in orchestrator execute_sessions

**Area:** orchestrate/executor
**Priority:** Low
**Source:** PR review (Phase 10)

The `join_set.spawn(async move { let handle = tokio::task::spawn(...); handle.await })` pattern adds a wrapper spawn solely to handle JoinError from the inner task. This nesting is unnecessary — the inner task's panic recovery could be handled after `join_set.join_next()` instead, collapsing to a single spawn level.

**File:** `crates/smelt-core/src/orchestrate/executor.rs:462-612`
