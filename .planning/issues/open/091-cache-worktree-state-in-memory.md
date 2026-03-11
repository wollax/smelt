# Cache Worktree State Files In Memory

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Suggestion
**File:** `crates/smelt-core/src/orchestrate/executor.rs` (worktree state lookup)

## Description

For each ready session, worktree state file is loaded from disk. No caching — repeated lookups (e.g., during resume) reload from disk. Minor performance concern for large orchestrations with many sessions; could cache state in a HashMap after first load.
