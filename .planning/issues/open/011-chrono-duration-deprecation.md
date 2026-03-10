---
id: "011"
area: smelt-core
severity: suggestion
source: pr-review-phase-02
---

# chrono::Duration deprecation warning

`chrono::Duration::hours()` is deprecated in favor of `chrono::TimeDelta::hours()`. Update orphan detection to use the non-deprecated API.

**File:** `crates/smelt-core/src/worktree/orphan.rs`
