# Test Abort Policy Parsed from Manifest in CLI

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Important
**File:** `crates/smelt-cli/tests/cli_orchestrate.rs`

## Description

`orchestrate_abort_on_failure` test verifies behavior but doesn't explicitly validate that `on_failure="abort"` in the TOML manifest is parsed and respected end-to-end through the CLI. The test works but doesn't isolate whether the policy was read from the manifest or happens to match a default.
