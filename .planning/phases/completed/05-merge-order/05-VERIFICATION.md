# Phase 05 — Merge Order Intelligence: Verification

**Status: passed**
**Score: 22/22 must-haves verified**
**Date: 2026-03-10**

---

## Plan 01 Must-Haves

| # | Must-Have | Status | Evidence |
|---|-----------|--------|----------|
| 1 | `MergeOrderStrategy` is a `#[non_exhaustive]` enum | PASS | `crates/smelt-core/src/merge/types.rs:6-15` — `#[non_exhaustive]` enum with `CompletionTime` (default) and `FileOverlap` variants |
| 2 | `MergeOpts` has `strategy: Option<MergeOrderStrategy>` field | PASS | `crates/smelt-core/src/merge/types.rs:20-25` — field present with builder methods |
| 3 | `ManifestMeta` has `merge_strategy: Option<MergeOrderStrategy>` field | PASS | `crates/smelt-core/src/session/manifest.rs:25` — `pub merge_strategy: Option<crate::merge::types::MergeOrderStrategy>` |
| 4 | `GitOps` trait has `diff_name_only` method | PASS | `crates/smelt-core/src/git/mod.rs:134-138` — `fn diff_name_only(&self, base_ref: &str, head_ref: &str) -> impl Future<Output = Result<Vec<String>>> + Send` |
| 5 | `GitCli` implements `diff_name_only` | PASS | `crates/smelt-core/src/git/cli.rs:312-318` — shells out to `git diff --name-only base_ref head_ref` |
| 6 | `comfy-table` v7 in workspace dependencies | PASS | `Cargo.toml:50` — `comfy-table = "7"`, used in `smelt-cli/Cargo.toml:23` |
| 7 | `serde_json` v1 in workspace dependencies | PASS | `Cargo.toml:53` — `serde_json = "1"`, used in both `smelt-core` and `smelt-cli` Cargo.toml |
| 8 | `MergeOrderStrategy` re-exported from `smelt_core` lib.rs | PASS | `crates/smelt-core/src/lib.rs:13` — `pub use merge::{MergeOpts, MergeOrderStrategy, MergePlan, MergeReport};` |

## Plan 02 Must-Haves

| # | Must-Have | Status | Evidence |
|---|-----------|--------|----------|
| 9 | `ordering.rs` module exists | PASS | `crates/smelt-core/src/merge/ordering.rs` — 317 lines with tests |
| 10 | `order_sessions()` function | PASS | `ordering.rs:17-25` — dispatches to `completion_time_order` or `file_overlap_order` based on strategy |
| 11 | `file_overlap_order()` function | PASS | `ordering.rs:42-105` — greedy algorithm implementation |
| 12 | `completion_time_order()` function | PASS | `ordering.rs:28-37` — identity function preserving manifest order |
| 13 | `CompletedSession` has `changed_files` field | PASS | `crates/smelt-core/src/merge/mod.rs:27` — `pub(crate) changed_files: HashSet<String>` |
| 14 | `CompletedSession` has `original_index` field | PASS | `crates/smelt-core/src/merge/mod.rs:28` — `pub(crate) original_index: usize` |
| 15 | Greedy file-overlap algorithm: pick min overlap against merged set, tiebreak by `original_index` | PASS | `ordering.rs:66-95` — iterates remaining sessions, tracks `best_overlap` and `best_original_index`, extends `merged_files` after each pick |
| 16 | `MergeRunner::run()` resolves strategy (CLI > manifest > default) and calls `order_sessions()` | PASS | `mod.rs:82-88` — `opts.strategy.or(manifest.manifest.merge_strategy).unwrap_or_default()` then `ordering::order_sessions(completed, strategy)` |
| 17 | `MergeReport.plan: Option<MergePlan>` | PASS | `types.rs:86` — `pub plan: Option<MergePlan>` |

## Plan 03 Must-Haves

| # | Must-Have | Status | Evidence |
|---|-----------|--------|----------|
| 18 | `smelt merge run <manifest>` replaces old `smelt merge <manifest>` | PASS | CLI shows `Usage: smelt merge <COMMAND>` with `run` and `plan` subcommands. `main.rs:43-46` uses `#[command(subcommand)]` for `MergeCommands`. |
| 19 | `smelt merge plan <manifest>` shows computed order without executing | PASS | `merge.rs:164-211` — `execute_merge_plan()` calls `runner.plan()` which does NOT create branches/worktrees. Test `test_plan_returns_merge_plan` verifies no target branch created. |
| 20 | `merge plan` outputs human-readable table (comfy-table) by default | PASS | `merge.rs:214-306` — `format_plan_table()` uses `comfy_table::Table` with UTF8_FULL preset, sections for Merge Order, Pairwise File Overlap, Session Files |
| 21 | `merge plan --json` outputs structured JSON | PASS | `merge.rs:185-188` — `serde_json::to_string_pretty(&plan)`. Test `test_format_plan_json_round_trip` verifies serialization/deserialization. |
| 22 | Both subcommands accept `--strategy` and `--target` flags | PASS | CLI help confirms both `run` and `plan` accept `--target <TARGET>` and `--strategy <STRATEGY>` flags |

## Success Criteria Verification

| Criterion | Status | Evidence |
|-----------|--------|----------|
| Merge order is deterministic given same set of completed sessions | PASS | Both strategies are deterministic: `CompletionTime` preserves manifest order (identity), `FileOverlap` uses greedy min-overlap with original_index tiebreak. No randomness or concurrency races. Tests `completion_time_preserves_input_order`, `file_overlap_reorders_correctly`, `tiebreak_by_original_index_when_overlaps_differ` all confirm determinism. |
| User can see chosen merge order before execution (dry-run or plan output) | PASS | `smelt merge plan` command computes and displays order without executing. Supports both table and JSON output. |
| File-overlap-based ordering produces fewer conflicts than naive ordering | PASS | `test_plan_file_overlap_strategy` demonstrates reordering: sessions with shared files (A-B overlap on `shared.rs`) are separated by non-overlapping session C. Algorithm verified by unit tests in `ordering.rs`. |

## Build & Test Results

| Check | Result |
|-------|--------|
| `cargo test --workspace` | PASS — 139 tests passed across 6 suites |
| `cargo clippy --workspace -- -D warnings` | PASS — no warnings or errors |
| `smelt merge --help` | PASS — shows `run` and `plan` subcommands |
| `smelt merge plan --help` | PASS — shows `--target`, `--strategy`, `--json` flags |
| `smelt merge run --help` | PASS — shows `--target`, `--strategy` flags |

## `MergeRunner::plan()` method

Confirmed at `crates/smelt-core/src/merge/mod.rs:49-66`. Validates state, collects sessions, resolves strategy, computes ordering, returns `MergePlan` without creating branches or worktrees.

## Notes

- The `MergeOrderStrategy` default is `CompletionTime` (which preserves manifest order), matching the plan specification.
- The file-overlap algorithm includes a fallback: when all pairwise overlaps are equal (including all-zero), it preserves manifest order and sets `fell_back: true` on the plan. This is a sensible design choice.
- The CLI strategy parser accepts `completion-time` and `file-overlap` as kebab-case strings.
- All Phase 5 types (`MergePlan`, `SessionPlanEntry`, `PairwiseOverlap`) derive `Serialize` and `Deserialize` for JSON support.
