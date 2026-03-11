# Sanitize task/session_name in agent CLI prompt

**Area:** session/agent
**Priority:** Low
**Source:** PR review (Phase 10)

The `task` and `session_name` strings are spliced verbatim into the prompt passed as `-p` to the `claude` CLI. If a manifest is loaded from an untrusted source, crafted values could contain prompt injection payloads. The `.claude/settings.json` deny list is the primary enforcement mechanism, but defense-in-depth sanitization would be prudent.

**File:** `crates/smelt-core/src/session/agent.rs:59-80`
