# 10-02 Summary — Wire AgentExecutor into Orchestrator and SessionRunner

## Result: PASS

All tasks completed. All verification criteria met.

## Tasks

| # | Task | Status | Commit |
|---|------|--------|--------|
| 1 | Wire AgentExecutor into Orchestrator.execute_sessions() | Done | `ad0f6ea` |
| 2 | Wire AgentExecutor into SessionRunner + CLI preflight message | Done | `0949271` |

## Changes

### Task 1: Orchestrator dispatch
- **executor.rs**: Added `check_agent_preflight()` method that scans manifest for agent sessions and resolves `claude` binary before worktree creation
- **executor.rs**: Replaced the immediate-complete `else` branch in `execute_sessions()` with full AgentExecutor dispatch — handles task/task_file resolution, timeout conversion, cancellation token forwarding, and SessionResult-to-SessionRunState mapping
- **executor.rs**: Updated `execute_sessions()` signature to accept `Option<&PathBuf>` for the claude binary; both `run()` and `resume()` pass the preflight result through
- **lib.rs**: Re-exported `AgentExecutor` and `resolve_claude_binary` for cross-crate access

### Task 2: SessionRunner and CLI
- **runner.rs**: Added `try_agent_session()` helper that attempts agent execution for script=None sessions, returning `Some(result)` on success or `None` for graceful degradation
- **runner.rs**: Graceful degradation covers: binary not found, task_file read failure, missing task+task_file, agent execution failure — all fall back to Completed with no commits and a `warn!()` log
- **orchestrate.rs (CLI)**: Added agent session detection message ("Detected N agent session(s) — using Claude Code backend") and early exit with install instructions when `claude` is not on PATH

## Verification

- `cargo build --workspace` — pass
- `cargo clippy --workspace -- -D warnings` — pass
- `cargo test --workspace` — 227 passed, 0 failed, 1 ignored (all existing tests preserved)
- Scripted session behavior completely unchanged
- The existing test `run_manifest_session_without_script_returns_completed` passes without modification via graceful degradation

## Design Decisions

- **SessionRunner graceful degradation**: When claude is not found OR agent execution fails, SessionRunner returns Completed with no commits. This preserves backward compatibility for the standalone `smelt session run` path. The orchestrator path (execute_sessions) handles failures strictly — preflight prevents binary-not-found, and execution failures are mapped to Failed state.
- **Preflight in both run() and resume()**: Both paths resolve the claude binary before dispatching sessions, preventing cryptic errors mid-execution.
- **Task file resolution before spawn boundary**: In the orchestrator, task_file is cloned before the spawn boundary and read inside the async closure, avoiding lifetime issues with session_def references.
