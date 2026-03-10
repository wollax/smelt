# Phase 5 Plan 3: CLI Subcommands & Plan Display Summary

Restructured CLI from `smelt merge <manifest>` to `smelt merge run|plan` subcommands. Added `merge plan` for dry-run order preview with table/JSON output. Wired `--strategy` flag into both subcommands.

## Dependency Graph

```
05-01 (types/traits) ──► 05-02 (ordering + integration) ──► 05-03 (CLI + display) ✓
```

## What Was Built

### CLI restructure (main.rs)
- `Commands::Merge` now delegates to `MergeCommands` subcommand enum (Run, Plan)
- Match arm dispatches to `execute_merge_run()` and `execute_merge_plan()`

### commands/merge.rs (rewritten)
- `MergeCommands` enum: `Run` and `Plan` subcommands with clap derive
- `parse_strategy()` — parses CLI string to `MergeOrderStrategy` (completion-time, file-overlap)
- `execute_merge_run()` — renamed from `execute_merge`, accepts `--strategy` flag, prints strategy used after merge
- `execute_merge_plan()` — loads manifest, calls `MergeRunner::plan()`, outputs table or JSON
- `format_plan_table()` — comfy-table UTF8_FULL/UTF8_ROUND_CORNERS output with:
  - Merge Order table: #, Session, Files Changed, Strategy Note
  - Pairwise File Overlap table (file-overlap strategy only)
  - Per-session file list (truncated at 10 files)
  - Fallback note when strategy could not differentiate

### smelt-core types.rs (extended)
- `MergeOpts::new()` constructor for non-exhaustive struct
- `Deserialize` added to `MergePlan`, `SessionPlanEntry`, `PairwiseOverlap` for JSON round-trip

### smelt-core merge/mod.rs (extended)
- `MergeRunner::plan()` — dry-run analysis: validates .smelt/, collect_sessions, resolve strategy, order_sessions; returns MergePlan without creating branches/worktrees

## Tests Added

### smelt-core (4 new integration tests)
- `test_plan_returns_merge_plan` — verifies plan() returns correct MergePlan and is read-only
- `test_plan_file_overlap_strategy` — verifies greedy reordering with 3 overlapping sessions
- `test_plan_strategy_from_manifest` — verifies manifest `merge_strategy` is picked up
- `test_plan_cli_overrides_manifest` — verifies CLI `--strategy` overrides manifest setting

### smelt-cli (6 unit tests)
- `test_parse_strategy_valid` / `test_parse_strategy_invalid` — strategy parser
- `test_format_plan_table_renders` — table output contains sessions, overlaps, sections
- `test_format_plan_table_fallback_note` — fallback message when fell_back is true
- `test_format_plan_table_many_files_truncated` — truncation at 10 files
- `test_format_plan_json_round_trip` — serialize/deserialize MergePlan via serde_json

### CLI integration tests (7 updated)
- All `cli_merge.rs` tests updated: `smelt merge <manifest>` → `smelt merge run <manifest>`

## Deviations

1. **MergeOpts non-exhaustive** — Could not use struct expression from smelt-cli (different crate). Added `MergeOpts::new(target_branch, strategy)` constructor. Also fixed existing `test_merge_custom_target_branch` to use `with_target_branch()`.

## File Tracking

| File | Action | Lines |
|------|--------|-------|
| crates/smelt-cli/src/main.rs | modified | ~170 |
| crates/smelt-cli/src/commands/merge.rs | rewritten | ~310 |
| crates/smelt-core/src/merge/types.rs | modified | ~130 |
| crates/smelt-core/src/merge/mod.rs | modified | ~690 |
| crates/smelt-cli/tests/cli_merge.rs | modified | ~440 |

## Metrics

| Metric | Value |
|--------|-------|
| Tasks completed | 2/2 |
| Tests added | 10 |
| Tests updated | 7 |
| Total tests passing | 104 (core) + 35 (cli) |
| Duration | ~7 minutes |
| Deviations | 1 (auto-fixed) |
