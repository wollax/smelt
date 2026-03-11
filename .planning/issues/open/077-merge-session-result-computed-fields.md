# MergeSessionResult computed fields can diverge from diff_stats

**Source:** PR #17 review round 2 (important - types)
**Area:** smelt-core/merge
**Phase:** 4

## Description

`files_changed`, `insertions`, and `deletions` on `MergeSessionResult` are redundant with `diff_stats: Vec<DiffStat>`. They're computed at construction time but nothing prevents divergence. Consider deriving them on-the-fly via accessor methods.
