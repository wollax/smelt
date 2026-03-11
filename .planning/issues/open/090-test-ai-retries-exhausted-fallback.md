# Test AI Retries Exhausted Falls Back to Manual

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Suggestion
**File:** `crates/smelt-cli/src/commands/orchestrate.rs` (retry loop)

## Description

The retry loop exhausts `max_retries` and falls back to manual resolution, but no test validates this path. Should verify that when AI retries are exhausted, the fallback to interactive manual resolution activates correctly.
