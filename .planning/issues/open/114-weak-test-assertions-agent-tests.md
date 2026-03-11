# Strengthen assertions in agent integration tests

**Area:** tests
**Priority:** Low
**Source:** PR review (Phase 10)

Several integration tests have weak assertions:
- `test_resolve_claude_binary_not_found` never calls the function under test
- `orchestrate_agent_session_without_claude_shows_install_message` doesn't verify claude is actually absent from minimal PATH
- `session_runner_graceful_degradation_no_claude` only checks exit 0, no per-session outcome assertions

**Files:** `crates/smelt-cli/tests/cli_agent.rs`, `crates/smelt-core/src/session/agent.rs:685-697`
