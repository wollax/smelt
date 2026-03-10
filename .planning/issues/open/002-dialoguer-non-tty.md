---
id: "002"
area: smelt-cli
severity: important
source: pr-review-phase-02
---

# dialoguer::Confirm panics in non-TTY context

`dialoguer::Confirm` in the worktree remove command has no fallback when stdin isn't a terminal. Running `smelt worktree remove <name>` in a non-interactive context (CI, piped input) will panic.

**File:** `crates/smelt-cli/src/commands/worktree.rs`
