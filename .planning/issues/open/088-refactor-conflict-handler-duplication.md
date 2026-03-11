# Refactor Conflict Handler Fallback Duplication

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Suggestion
**File:** `crates/smelt-cli/src/commands/orchestrate.rs` (AiInteractiveConflictHandler)

## Description

`AiInteractiveConflictHandler.handle_conflict` repeats the fallback setup logic multiple times across the accept/edit/reject paths. Extract into a helper method to reduce duplication and improve maintainability.
