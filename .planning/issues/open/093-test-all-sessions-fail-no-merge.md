# Test All Sessions Fail — Merge Phase Skipped

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Suggestion
**File:** `crates/smelt-cli/tests/cli_orchestrate.rs`

## Description

No test for a run where all sessions fail or are skipped, verifying that the merge phase is correctly skipped and `merge_report` is `None` in the output. Should test the edge case where zero sessions complete successfully.
