---
phase: "08"
plan: "03"
subsystem: cli-orchestration
tags: [cli, orchestration, indicatif, comfy-table, integration-tests]
requires:
  - "08-01"
  - "08-02"
provides:
  - "smelt orchestrate run CLI command"
  - "live dashboard with per-session spinners"
  - "comfy-table summary output"
  - "JSON output mode"
  - "resume detection with interactive prompt"
  - "graceful Ctrl-C shutdown"
  - "integration tests for orchestrate command"
affects:
  - "09-documentation"
  - "10-release"
tech-stack:
  added: []
  patterns:
    - "indicatif MultiProgress dashboard with per-session ProgressBar spinners"
    - "CancellationToken + tokio::signal::ctrl_c for graceful shutdown"
    - "Conflict handler enum dispatcher (same as merge.rs pattern)"
key-files:
  created:
    - "crates/smelt-cli/src/commands/orchestrate.rs"
    - "crates/smelt-cli/tests/cli_orchestrate.rs"
  modified:
    - "crates/smelt-cli/src/commands/mod.rs"
    - "crates/smelt-cli/src/main.rs"
decisions:
  - "Conflict handler code duplicated from merge.rs rather than shared — keeps modules decoupled and avoids complex trait object gymnastics"
  - "OrchestrateConflictHandler enum named differently from MergeConflictHandler to avoid confusion"
  - "Summary table sorts sessions alphabetically for deterministic output"
  - "Non-TTY dashboard uses eprintln line-by-line fallback"
  - "Resume prompt uses dialoguer::Confirm with default=true"
  - "Visible alias 'orch' for orchestrate subcommand"
metrics:
  duration: "7m"
  completed: "2026-03-10"
---

# Phase 08 Plan 03: CLI Command & Integration Tests Summary

CLI command `smelt orchestrate run` with live indicatif dashboard, comfy-table summary, JSON output, resume detection, and Ctrl-C graceful shutdown; 7 integration tests covering parallel, sequential, diamond DAG, failure policies, and JSON output.

## Tasks Completed

### Task 1: CLI command, dashboard, summary, and signal handling
- **Commit:** `ca9fe59`
- Created `orchestrate.rs` with `OrchestrateCommands` enum and `execute_orchestrate_run()` function
- Live dashboard via `indicatif::MultiProgress` with per-session spinners showing status transitions (pending/running/done/failed/skipped/cancelled)
- Non-TTY fallback to simple `eprintln!` line-by-line output
- Post-completion summary table via comfy-table showing sessions, status, duration, and merge stats
- `--json` flag serializes `OrchestrationReport` to stdout via `serde_json::to_string_pretty`
- Ctrl-C handling: `CancellationToken` + `tokio::signal::ctrl_c` spawn
- Resume detection: `RunStateManager::find_incomplete_run()` + manifest hash validation + `dialoguer::Confirm` prompt
- Conflict handler reuses the same AI+Interactive pattern from merge.rs
- All flags wired: `--target`, `--strategy`, `--verbose`, `--no-ai`, `--json`
- Visible alias `orch` for the orchestrate subcommand

### Task 2: Integration tests for orchestrate command
- **Commit:** `931065a`
- 7 integration tests using `assert_cmd` + `predicates` + `tempfile`:
  - `orchestrate_two_parallel_sessions` — two independent sessions, merge verification with ls-tree
  - `orchestrate_sequential_dependency` — A->B dependency chain
  - `orchestrate_skip_dependents_on_failure` — failed session skips dependents, exit code 1
  - `orchestrate_abort_on_failure` — abort policy stops entire orchestration
  - `orchestrate_json_output` — validates JSON structure with serde_json::Value
  - `orchestrate_diamond_dependency` — A->{B,C}->D DAG with all files verified
  - `orchestrate_implicit_sequential` — parallel_by_default=false runs in manifest order

## Deviations from Plan

None — plan executed exactly as written.

## Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Conflict handler code duplicated from merge.rs | Avoids complex shared trait object plumbing; modules stay decoupled |
| 2 | `#[allow(clippy::too_many_arguments)]` on execute_orchestrate_run | 8 args matches the merge command pattern; refactoring to a struct is future work |
| 3 | Summary table sorts sessions alphabetically | Deterministic output for testing and readability |

## Verification

```
cargo check -p smelt-cli          # pass
cargo clippy -p smelt-cli -- -D warnings  # pass
cargo test -p smelt-cli            # 47 tests pass (7 new orchestrate tests)
cargo test -p smelt-core           # 202 tests pass (no regressions)
smelt orchestrate --help           # shows command
smelt orchestrate run --help       # shows all flags
smelt orch --help                  # alias works
```

## Next Phase Readiness

Phase 08 is now complete. All three plans (types+DAG, executor, CLI) are implemented and tested. The orchestrator is fully usable end-to-end through the CLI.
