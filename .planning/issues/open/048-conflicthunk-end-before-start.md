---
id: "048"
area: smelt-core
severity: suggestion
source: pr-review-phase-06
---
# ConflictHunk allows end_line < start_line

The struct has no invariant enforcement preventing `end_line` from being less than `start_line`. Add a validated constructor or a `debug_assert` to catch invalid ranges.

File: `conflict.rs`
