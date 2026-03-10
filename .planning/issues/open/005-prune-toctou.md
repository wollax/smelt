---
id: "005"
area: smelt-core
severity: important
source: pr-review-phase-02
---

# prune() has TOCTOU window with detect_orphans

`prune()` calls `detect_orphans()` which reads state files and git worktree list, then calls `remove()` which re-reads the same state. Between detection and removal, state could change. Consider passing the already-loaded state to remove or using a single-pass approach.

**File:** `crates/smelt-core/src/worktree/mod.rs`
