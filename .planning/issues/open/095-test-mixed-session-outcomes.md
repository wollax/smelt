# Test Mixed Session Outcomes in Single Run

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Suggestion
**File:** `crates/smelt-cli/tests/cli_orchestrate.rs`

## Description

CLI tests validate happy path and single failures but don't test a mix of completed, failed, and skipped sessions in a single run with complex dependency topology. Diamond dependency test is close but all sessions succeed. Add a test with mixed outcomes.
