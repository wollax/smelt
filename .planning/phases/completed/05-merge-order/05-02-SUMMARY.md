# Phase 5 Plan 2: Ordering Algorithms & MergeRunner Integration Summary

Implemented the greedy file-overlap ordering algorithm and completion-time identity ordering, integrated both into MergeRunner with strategy resolution (CLI > manifest > default).

## Dependency Graph

```
05-01 (types/traits) ──► 05-02 (ordering + integration) ──► 05-03 (display)
```

## What Was Built

### ordering.rs (new module)
- `order_sessions()` — dispatches on `MergeOrderStrategy` enum
- `completion_time_order()` — identity function preserving manifest order
- `file_overlap_order()` — greedy algorithm: each round picks the session with minimum overlap against the already-merged file set; tiebreaks by `original_index`
- `compute_pairwise_overlaps()` — computes file overlap for all (i, j) pairs
- Fallback detection: when all pairwise overlaps are equal (including all-zero), falls back to manifest order with `fell_back: true`

### types.rs (extended)
- `MergePlan` — strategy, fell_back flag, ordered session list, pairwise overlaps
- `SessionPlanEntry` — per-session name, branch, changed files, file count, original index
- `PairwiseOverlap` — session pair with overlapping file list and count
- `MergeReport.plan: Option<MergePlan>` — populated on successful merge

### mod.rs (MergeRunner integration)
- `CompletedSession` promoted to `pub(crate)` with `changed_files: HashSet<String>` and `original_index: usize`
- `collect_sessions()` now async — calls `diff_name_only(base_ref, branch)` per session
- `run()` resolves strategy precedence: `opts.strategy > manifest.merge_strategy > Default`
- `run()` calls `ordering::order_sessions()` between collect and merge phases

### lib.rs
- Re-exports `MergePlan` from workspace root

## Test Coverage

8 new unit tests in `ordering.rs`:
- `completion_time_preserves_input_order`
- `file_overlap_no_overlaps_falls_back`
- `file_overlap_reorders_correctly` (A→C→B ordering verified)
- `empty_changed_files_have_zero_overlap`
- `tiebreak_by_original_index`
- `tiebreak_by_original_index_when_overlaps_differ`
- `single_session_preserves_order`
- `pairwise_overlaps_computed_for_all_pairs`

All 100 existing + new tests pass. Clippy clean with `-D warnings`.

## File Tracking

| File | Action | Lines |
|------|--------|-------|
| `crates/smelt-core/src/merge/ordering.rs` | Created | ~270 |
| `crates/smelt-core/src/merge/types.rs` | Extended | ~115 |
| `crates/smelt-core/src/merge/mod.rs` | Modified | ~340 |
| `crates/smelt-core/src/lib.rs` | Modified | 16 |

## Decisions

- `CompletedSession` is `pub(crate)` (not public) — ordering module needs access but external consumers don't
- Fallback detection uses equality check on all pairwise overlap counts — simple and correct for the two-strategy model
- `diff_name_only` failures log a warning and default to empty file set (graceful degradation)
- The non_exhaustive wildcard arm was removed from `order_sessions` match since clippy flags it as unreachable — future variants will produce a compile error, which is preferable

## Metrics

- **Started:** 2026-03-10T13:51:57Z
- **Completed:** 2026-03-10T14:03:43Z
- **Duration:** ~12 minutes
- **Tests:** 100 passed, 0 failed
- **Clippy:** Clean
