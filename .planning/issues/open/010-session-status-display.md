---
id: "010"
area: smelt-core
severity: suggestion
source: pr-review-phase-02
---

# Add Display impl on SessionStatus

`SessionStatus` enum lacks a `Display` implementation. CLI formatting is done ad-hoc in the command handler. A proper `Display` impl would ensure consistent formatting across all output contexts.

**File:** `crates/smelt-core/src/worktree/state.rs`
