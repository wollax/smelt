# Untested Manifest-to-Executor Failure Policy Coupling

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Important
**File:** `crates/smelt-core/src/orchestrate/executor.rs`, `crates/smelt-core/src/session/manifest.rs`

## Description

The interaction between `manifest.on_failure` and `executor.failure_policy` is not tested end-to-end. Executor uses `manifest.manifest.on_failure.unwrap_or_default()` but there's no unit test validating that a manifest with `on_failure = "abort"` actually produces `FailurePolicy::Abort` in the executor's decision logic.
