# Log Merge Phase Transitions

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Suggestion
**File:** `crates/smelt-core/src/orchestrate/executor.rs` (merge phase)

## Description

Merge phase transitions (Sessions → Merging → Complete) are saved to state but not logged via tracing. User doesn't see when merge phase starts unless verbose/dashboard is on. Add `info!()` logs at phase transitions for observability.
