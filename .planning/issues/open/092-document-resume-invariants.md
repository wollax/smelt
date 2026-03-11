# Document Resume Method Invariants and Assumptions

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Suggestion
**File:** `crates/smelt-core/src/orchestrate/executor.rs` (resume method)

## Description

Resume method assumes worktrees still exist and session defs are unchanged. No documentation of invariants or guarantees. If a worktree was deleted manually between runs, resume silently fails the session. Should document assumptions clearly in doc comments.
