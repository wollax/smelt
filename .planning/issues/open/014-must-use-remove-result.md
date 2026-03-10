---
id: "014"
area: smelt-core
severity: suggestion
source: pr-review-phase-02
---

# Add #[must_use] to RemoveResult

`RemoveResult` returned by `WorktreeManager::remove()` should be `#[must_use]` to prevent callers from ignoring the removal status (e.g., whether branch was actually deleted).

**File:** `crates/smelt-core/src/worktree/mod.rs`
