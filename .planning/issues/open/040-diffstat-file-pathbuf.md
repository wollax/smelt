---
id: "040"
area: smelt-core
severity: suggestion
source: pr-review-phase-04
---
# DiffStat.file could be PathBuf instead of String

Using `PathBuf` would be more idiomatic and consistent with `WorktreeState.worktree_path`. Trade-off: git can return paths with encoding issues where `String` is more honest.
