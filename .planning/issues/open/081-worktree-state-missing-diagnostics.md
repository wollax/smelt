# Insufficient Diagnostics for Missing Worktree State

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Important
**File:** `crates/smelt-core/src/orchestrate/executor.rs` (worktree state lookup)

## Description

When worktree state file is missing, the session fails with a generic reason string without logging which session or what file path was expected. The recovery path then marks dependents as skipped. Insufficient diagnostics for troubleshooting worktree state corruption or race conditions.
