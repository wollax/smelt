---
id: "003"
area: smelt-cli
severity: important
source: pr-review-phase-02
---

# execute_list doesn't map errors to user-friendly messages

`execute_list` propagates raw `SmeltError` variants without user-friendly formatting. Should provide contextual error messages similar to other command handlers.

**File:** `crates/smelt-cli/src/commands/worktree.rs`
