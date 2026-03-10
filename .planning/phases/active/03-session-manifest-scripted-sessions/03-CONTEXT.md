# Phase 3: Session Manifest & Scripted Sessions - Context

**Gathered:** 2026-03-09
**Status:** Ready for research

<domain>
## Phase Boundary

Define the session manifest format (what each agent should work on) and implement the scripted/simulated session backend. Scripted sessions are the primary testing mechanism for the entire merge pipeline. This phase does NOT include merge operations, real agent sessions, orchestration plan execution, or Assay integration.

</domain>

<decisions>
## Implementation Decisions

### Manifest format & structure
- Single manifest file defining all sessions in a group (not per-session files) — captures relationships, shared base branch, and coordination context
- Format: TOML (consistent with existing `.smelt/` state files)
- Task descriptions: inline for short descriptions, `task_file` reference for longer prompts; external takes precedence if both specified
- File scope: glob patterns and exact paths supported per session (e.g., `file_scope = ["src/auth/**", "src/lib.rs"]`)
- Worktree config per session: session name required, `base_ref` defaults to HEAD if omitted, worktree/branch names auto-derived from session name (overridable)
- Schema designed for extensibility (future Assay backend, real agent config) but only scripted backend implemented

### Script definition language
- Declarative specification, not imperative shell scripts — predictable, reproducible test scenarios
- Scripts specify individual commits: message, files changed, file content
- Explicit conflict generation — scripts target specific files and regions to guarantee deterministic conflicts between sessions
- Commit granularity: specify individual commits, ranges, or branch-level final state
- First-class failure simulation in the script format:
  - `exit_after: N` — crash after N commits (non-zero exit)
  - `simulate_failure: "crash" | "hang" | "partial"` — failure mode variants
  - Keeps test scenarios self-contained and readable

### Session lifecycle & completion signaling
- Two-signal completion detection (works for both scripted and real agents):
  - Primary: process exit monitoring (orchestrator watches child process)
  - Secondary: branch state verification (new commits since session creation)
  - Mismatch detection: clean exit + no commits = suspicious, flagged to orchestrator
- Status transitions driven by the orchestrator, not the session process itself
  - Session process cannot update its own TOML state (crash-safe: orchestrator handles all transitions)
  - WorktreeState transitions: Created → Running → Completed/Failed
- Configurable timeout per session in the manifest
  - Timeout exceeded: orchestrator kills (SIGTERM → SIGKILL escalation after grace period), marks Failed
  - Autonomous operation assumed — minimal human intervention on timeout
- Session result captures: steps completed, failure reason, whether branch has commits
  - Retry/restart policy deferred to Phase 8 (Orchestration Plan)

### Process management & cleanup
- Process group isolation: each session runs in its own process group (`setsid`/process groups via `Command::pre_exec`)
  - Prevents zombies if orchestrator is killed
  - Enables cascading SIGTERM to all session processes
- Orchestrator shutdown sequence on SIGINT/SIGTERM:
  1. Signal all running session processes to stop
  2. Wait for graceful shutdown (configurable grace period)
  3. Force kill (SIGKILL) after timeout
  4. Update state files to reflect interruption
- Execution modes: sequential (default) and parallel (tokio tasks spawning processes)
  - Both modes implemented in Phase 3
  - Sequential for straightforward testing, parallel for realistic multi-agent scenarios
- Environment isolation per session:
  - Each session runs in its worktree directory
  - Sessions inherit from a default env/PATH/state
  - Per-session environment variable overrides supported in manifest

### Claude's Discretion
- Exact manifest schema field names and nesting
- Script DSL syntax details (TOML tables vs arrays of tables)
- Grace period duration defaults for timeout/shutdown
- Log output format during session execution
- How branch state verification is reported (warnings vs errors)

</decisions>

<specifics>
## Specific Ideas

- Failure simulation should cover the three known modes: crash (non-zero exit), hang (exceeds timeout), partial (exits early with some commits)
- Manifest should be extensible for future `backend = "claude-code"` sessions without breaking changes

</specifics>

<deferred>
## Deferred Ideas

- Assay worktree integration — Assay has built-in worktree management; could serve as an alternative backend in v0.2.0
- Retry/restart policy for failed sessions — deferred to Phase 8 (Orchestration Plan & Task Graph)
- Per-session file manifests — revisit if single-file manifests become unwieldy at scale

</deferred>

---

*Phase: 03-session-manifest-scripted-sessions*
*Context gathered: 2026-03-09*
