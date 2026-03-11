# AI Retry Swallows Provider Errors

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Important
**File:** `crates/smelt-cli/src/commands/orchestrate.rs` (retry_with_feedback)

## Description

If the AI provider fails during retry, the method swallows the error with only an `eprintln`. The user expects feedback to be processed but the system silently falls back to manual without explaining why the retry failed. Should return `Err` or at least capture the failure reason in a structured way.
