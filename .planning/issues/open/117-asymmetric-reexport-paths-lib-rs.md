# Asymmetric re-export paths for AgentExecutor vs resolve_claude_binary

**Area:** lib
**Priority:** Low
**Source:** PR review (Phase 10)

`AgentExecutor` is re-exported through `session/mod.rs` while `resolve_claude_binary` is directly from `session::agent`. Minor stylistic inconsistency — either both should go through `session`, or both should be direct.

**File:** `crates/smelt-core/src/lib.rs:25-26`
