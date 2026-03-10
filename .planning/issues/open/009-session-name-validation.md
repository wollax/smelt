---
id: "009"
area: smelt-core
severity: suggestion
source: pr-review-phase-02
---

# Session name validation missing

No validation on session names passed to `WorktreeManager::create()`. Empty strings, slashes, special characters, and very long names could create invalid branch names or filesystem paths. Add validation early in `create()`.

**File:** `crates/smelt-core/src/worktree/mod.rs`
