# Document FailurePolicy Default Rationale

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Suggestion
**File:** `crates/smelt-core/src/orchestrate/types.rs` (FailurePolicy enum)

## Description

`FailurePolicy::SkipDependents` is the default but the reason or design rationale is not documented. Should clarify in doc comments why `SkipDependents` is the default (maximizes completed work) and when `Abort` is preferred (strict correctness requirements).
