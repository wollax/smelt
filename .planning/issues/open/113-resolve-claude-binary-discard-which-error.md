# resolve_claude_binary discards which::Error details

**Area:** session/agent
**Priority:** Low
**Source:** PR review (Phase 10)

`which::which("claude").map_err(|_| SmeltError::AgentNotFound)` discards the original `which::Error` (e.g., PATH not set vs. binary not found). Include the source error in `AgentNotFound` or log it before discarding for better diagnostics.

**File:** `crates/smelt-core/src/session/agent.rs:500`
