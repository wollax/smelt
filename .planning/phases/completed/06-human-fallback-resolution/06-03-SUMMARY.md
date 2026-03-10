# Plan 06-03 Summary: Interactive CLI Conflict Handler

## Status: Complete

## What was done

### Task 1: InteractiveConflictHandler and CLI updates
- Added `InteractiveConflictHandler` struct in `crates/smelt-cli/src/commands/merge.rs` implementing `ConflictHandler` trait
- Handler shows conflict summary on stderr: file paths, hunk line ranges
- Small conflicts (<20 lines): inline display with colored markers (red `<<<<<<<`, yellow `=======`, green `>>>>>>>`)
- Large conflicts: truncated view with line count
- Uses `tokio::task::spawn_blocking` wrapping `dialoguer::Select` on `console::Term::stderr()` for three options: Resolve, Skip, Abort
- Non-TTY fallback: detects `!console::Term::stderr().is_term()` and propagates `MergeConflict` error (preserves Phase 4 behavior in CI/tests)
- Added `--verbose` flag to `MergeCommands::Run` — shows full file contents in worktree when conflicts occur
- Updated `execute_merge_run()` to accept and pass `verbose` parameter
- Updated `MergeOpts` construction to use `verbose` flag
- Added resolution status output: resolved/skipped session counts in merge summary
- Updated `main.rs` dispatch to pass `verbose` from CLI args

### Task 2: Integration tests
- `ResolveConflictHandler` test handler: strips conflict markers, returns `ConflictAction::Resolved`
- `test_merge_conflict_resolve_flow`: two conflicting sessions, resolve handler fixes markers, verifies both merged with correct resolution methods, commit message contains `[resolved: manual]`
- `test_merge_conflict_skip_continues`: three sessions (A clean, B conflicts, C clean), skip handler skips B, verifies A and C merge cleanly while B is recorded as skipped

## Deviation log

- **TTY detection guard**: Added `console::Term::stderr().is_term()` check at the start of `handle_conflict`. When not a terminal, returns `MergeConflict` error instead of attempting interactive prompt. This was necessary because the existing CLI integration test `test_merge_conflict_exits_with_error` runs in a non-TTY environment and would fail with "not a terminal" IO error. This is the correct design — interactive conflict resolution requires a terminal.

## Metrics

| Metric | Value |
|--------|-------|
| Tests added | 2 |
| Tests total (workspace) | 154 |
| Files modified | 3 |
| Lines added | ~385 |
| merge.rs (CLI) | 327 lines (exceeds 280 minimum) |
| Clippy | clean |
| Build | clean |

## Key artifacts

- `crates/smelt-cli/src/commands/merge.rs` — InteractiveConflictHandler, updated execute_merge_run with --verbose and resolution status
- `crates/smelt-cli/src/main.rs` — verbose arg forwarding
- `crates/smelt-core/src/merge/mod.rs` — ResolveConflictHandler test handler, resolve and skip integration tests

## Must-have verification

| Truth | Status |
|-------|--------|
| InteractiveConflictHandler implements ConflictHandler | Done |
| dialoguer::Select with three items on stderr | Done |
| spawn_blocking for async compatibility | Done |
| Conflict summary shows file paths and line ranges | Done |
| Small conflicts (<20 lines) shown inline | Done |
| Large conflicts show truncated view | Done |
| Resolve returns ConflictAction::Resolved | Done |
| --verbose dumps full context | Done |
| CLI passes handler to runner.run() | Done |
| Resolution status in summary output | Done |
| MergeAborted handled with clean message | Done (existing) |
| Integration test for skip flow | Done |
