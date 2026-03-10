---
id: "008"
area: smelt-core
severity: important
source: pr-review-phase-02
---

# No mock GitOps tests for WorktreeManager

All WorktreeManager tests use real git repos via `GitCli`. Adding mock `GitOps` implementations would enable testing error paths and edge cases that are hard to reproduce with real git, and would run faster.

**File:** `crates/smelt-core/src/worktree/mod.rs`
