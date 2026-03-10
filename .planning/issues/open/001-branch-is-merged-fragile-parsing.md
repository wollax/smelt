---
id: "001"
area: smelt-core
severity: important
source: pr-review-phase-02
---

# branch_is_merged parsing is fragile

`branch_is_merged` in `cli.rs` uses `trim_start_matches` with single chars for parsing git output. This is brittle — should use proper prefix handling to avoid misparses on branch names with leading whitespace or `*` in them.

**File:** `crates/smelt-core/src/git/cli.rs`
