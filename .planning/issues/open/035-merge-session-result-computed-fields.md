---
id: "035"
area: smelt-core
severity: suggestion
source: pr-review-phase-04
---
# MergeSessionResult has redundant computed fields

`files_changed`, `insertions`, `deletions` are derivable from `diff_stats`. Replace with computed methods to avoid inconsistency risk. Same applies to `MergeReport` totals.
