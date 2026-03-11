# Signal Handler Race Condition on Double Ctrl-C

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Important
**File:** `crates/smelt-cli/src/commands/orchestrate.rs` (signal handler spawn)

## Description

`tokio::spawn` closure for signal handling calls `cancel()` on CancellationToken. If a signal arrives during shutdown, `cancel()` might be called multiple times or race with the shutdown path. The `.ok()` on `ctrl_c()` suppresses errors silently — if signal registration fails, orchestration continues without Ctrl-C support.
