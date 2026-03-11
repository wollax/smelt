# Test Resume Flow in Integration Tests

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Suggestion
**File:** `crates/smelt-cli/tests/cli_orchestrate.rs`

## Description

No integration test exercises the resume flow — validating that resume correctly skips already-completed sessions and continues from the right point. Resume detection logic exists in orchestrate.rs but has no CLI-level test coverage.
