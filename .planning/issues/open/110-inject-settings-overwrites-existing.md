# inject_settings silently overwrites existing .claude/settings.json

**Area:** session/agent
**Priority:** Low
**Source:** PR review (Phase 10)

If the project repository already has a `.claude/settings.json`, it is silently overwritten on every session execution. This is inconsistent with the `CLAUDE.md` fallback behavior (which checks for existing files). Consider merging with existing settings or logging a warning.

**File:** `crates/smelt-core/src/session/agent.rs:141-163`
