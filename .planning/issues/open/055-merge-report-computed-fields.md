# Make MergeReport helper methods derived from data

**Source:** PR #17 review (suggestion)
**Area:** smelt-core/merge
**Phase:** 4

## Description

`MergeReport` has `has_skipped()`, `has_resolved()`, `has_conflict_skipped()` methods that check corresponding Vec fields. Consider whether `sessions_resolved` and `sessions_conflict_skipped` should be computed from `sessions_merged` data rather than maintained as separate lists, reducing the risk of inconsistency.
