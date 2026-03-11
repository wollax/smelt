# Silent Resume Detection Errors

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Important
**File:** `crates/smelt-cli/src/commands/orchestrate.rs` (resume detection block)

## Description

Resume detection silently catches all errors with only a warning `eprintln`. If `state_manager.find_incomplete_run` fails due to IO errors, permission issues, or corrupted state files, the user proceeds without resuming, potentially losing work context. Should propagate critical errors or provide stronger warnings.
