---
id: "007"
area: smelt-core
severity: important
source: pr-review-phase-02
---

# Several untested error paths

Missing test coverage for:
- `prune()` with actual orphans (end-to-end)
- Custom `dir_name` in `CreateWorktreeOpts`
- `BranchUnmerged` error path in `remove()` (non-force)
- State file IO failures during remove

**Files:** `crates/smelt-core/src/worktree/mod.rs`
