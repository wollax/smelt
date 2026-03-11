# Non-TTY AI conflict resolution fallback returns hard error

**Area:** cli/orchestrate
**Priority:** Low
**Source:** PR review (Phase 10)

In non-TTY environments, the AI conflict resolution fallback path returns `Err(SmeltError::MergeConflict)` rather than a graceful non-interactive policy. Consider adding a `NonInteractiveFallbackHandler` that aborts or accepts-theirs for CI contexts.

**File:** `crates/smelt-cli/src/commands/orchestrate.rs:395-406`
