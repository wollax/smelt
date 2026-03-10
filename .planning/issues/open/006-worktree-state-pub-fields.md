---
id: "006"
area: smelt-core
severity: important
source: pr-review-phase-02
---

# WorktreeState has all-pub fields with no validation

`WorktreeState` struct has all public fields, allowing construction of invalid states (e.g., empty session_name, mismatched branch_name). Consider using a builder or constructor that validates invariants.

**File:** `crates/smelt-core/src/worktree/state.rs`
