---
id: "012"
area: smelt-core
severity: suggestion
source: pr-review-phase-02
---

# resolve_status() is dead code placeholder

`WorktreeManager::resolve_status()` currently returns the stored status without actual cross-referencing. Either implement it or remove the dead code path and the `_git_knows` binding.

**File:** `crates/smelt-core/src/worktree/mod.rs`
