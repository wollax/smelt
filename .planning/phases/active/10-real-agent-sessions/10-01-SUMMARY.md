# Phase 10 Plan 01: AgentExecutor Module Summary

AgentExecutor spawns Claude Code CLI in a worktree with CLAUDE.md/settings.json injection, process group isolation, timeout/cancellation via tokio::select!, and log capture.

## What Was Built

### AgentExecutor (`crates/smelt-core/src/session/agent.rs`)

- **Struct**: `AgentExecutor { claude_binary, worktree_path, log_path, timeout }`
- **execute()**: Async method spawning `claude -p "..." --dangerously-skip-permissions --output-format json` with optional `--model` flag
- **CLAUDE.md injection**: Writes session constraints to worktree root; uses `.claude/CLAUDE.md` if root `CLAUDE.md` already exists (avoids overwriting project instructions)
- **settings.json injection**: Writes `.claude/settings.json` with allow/deny permission rules and optional model pin
- **Prompt construction**: `build_prompt()` assembles task + file scope + instructions into a structured prompt
- **Process lifecycle**: `process_group(0)` + `kill_on_drop(true)` + `kill_process_group()` via `libc::kill(-pgid, SIGTERM)`
- **Timeout/cancel**: `tokio::select!` with biased cancellation check, `tokio::time::timeout` for deadline enforcement
- **Log capture**: stdout/stderr written to log file after process exits
- **Exit mapping**: exit 0 = Completed (has_commits=true), non-zero = Failed, timeout = TimedOut, cancel = Killed
- **resolve_claude_binary()**: Public function using `which::which("claude")` with `SmeltError::AgentNotFound`

### Error Variant (`crates/smelt-core/src/error.rs`)

- `SmeltError::AgentNotFound`: "'claude' CLI not found on $PATH. Install Claude Code to use real agent sessions."

### Module Registration (`crates/smelt-core/src/session/mod.rs`)

- `pub mod agent;` + `pub use agent::AgentExecutor;`

## Tests

10 unit tests covering:
1. `test_build_prompt_includes_task` — task appears in prompt
2. `test_build_prompt_includes_file_scope` — file scope patterns appear when provided
3. `test_build_prompt_no_file_scope` — prompt works without file scope
4. `test_inject_claude_md_creates_file` — CLAUDE.md created at worktree root
5. `test_inject_claude_md_uses_dot_claude_when_root_exists` — uses .claude/CLAUDE.md when root exists
6. `test_inject_claude_md_with_file_scope` — file scope patterns in CLAUDE.md
7. `test_inject_settings_creates_file` — settings.json created with valid JSON
8. `test_inject_settings_with_model` — model field present when provided
9. `test_inject_settings_deny_includes_push` — deny list includes git push
10. `test_resolve_claude_binary_not_found` — AgentNotFound error variant works
11. `test_resolve_claude_binary_found` — (ignored, requires claude on PATH)

## Deviations from Plan

None — plan executed exactly as written.

## Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Take stdout/stderr handles before select! instead of wait_with_output() | `wait_with_output()` moves `child`, preventing use in cancellation/timeout arms. Taking handles first allows `child.wait()` (borrow) in all arms. |
| 2 | Tasks 1 and 2 combined in single commit | Tests are in same file (`agent.rs` `#[cfg(test)] mod tests`), naturally co-developed |

## Commits

| Hash | Description |
|------|-------------|
| 5ed4e20 | feat(10-01): AgentExecutor struct + execute method + CLAUDE.md/settings injection |

## Verification

- `cargo check -p smelt-core` — passes
- `cargo clippy -p smelt-core -- -D warnings` — passes
- `cargo test -p smelt-core --lib session::agent` — 10 passed, 1 ignored
- `cargo test -p smelt-core --lib` — 227 passed, 1 ignored (full suite)
- `AgentExecutor` importable via `smelt_core::session::AgentExecutor`

## Duration

~5 minutes

## Next Phase Readiness

Plan 10-02 (orchestrator integration) can proceed. `AgentExecutor` is ready to be wired into `SessionRunner` at the `None` script branch.
