---
id: "037"
area: smelt-core
severity: suggestion
source: pr-review-phase-04
---
# Re-exports in lib.rs incomplete

`MergeRunner`, `MergeSessionResult`, and `DiffStat` are not re-exported from `lib.rs`. Inconsistent with how `SessionRunner` and `WorktreeManager` are re-exported.
