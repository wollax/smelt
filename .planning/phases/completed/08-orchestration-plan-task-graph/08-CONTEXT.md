# Phase 8: Orchestration Plan & Task Graph - Context

**Gathered:** 2026-03-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Enable the user to define a complete orchestration plan — a task graph specifying which sessions to run, their dependencies, and how to merge the results. The orchestrator executes the plan: creates worktrees, launches sessions, waits for completion, merges in order, resolves conflicts, and reports results. Plan execution can be interrupted and resumed via git-native state.

</domain>

<decisions>
## Implementation Decisions

### Dependency model
- Both implicit and explicit modes: sessions default behavior controlled by manifest-level `parallel_by_default` setting (true/false)
- Explicit `depends_on = ["session-a", "session-b"]` to add dependency constraints
- No depends_on + parallel_by_default=true → run concurrently; parallel_by_default=false → run in manifest order
- Cycles detected and rejected at plan validation time

### Execution visibility
- Live-updating status dashboard during execution (like docker-compose up) showing each session's state (pending/running/done/failed) with elapsed time
- Dashboard covers the full lifecycle — transitions from session execution phase to merge phase, showing merge progress (per-session merge, conflicts, resolution)
- Individual session stdout/stderr hidden by default, captured to log files; --verbose to stream
- Post-completion summary: comfy-table by default, JSON with --json flag (matches existing merge plan convention)

### Failure & partial completion
- Failure policy is configurable at manifest level: `on_failure = "skip-dependents" | "abort"`
- "skip-dependents": dependents of a failed session are marked as skipped, independent sessions continue
- "abort": any session failure stops the entire orchestration
- Merge phase proceeds with successful sessions only — failed/skipped sessions excluded automatically
- Failed session state preserved for debugging (worktrees persist, matching current behavior)

### Interrupt & resume
- Automatic detection: `smelt orchestrate run` detects incomplete state from a previous run and offers to resume ("Previous run found. Resume? [Y/n]")
- Step-level granularity: resume can pick up mid-lifecycle (e.g., sessions completed but merge didn't happen yet; or merge was partially done)
- State persisted for crash recovery — orchestration progress tracked through lifecycle steps

### Claude's Discretion
- DAG complexity limits (whether to warn on large graphs)
- Dependency outcome semantics (completion-only vs outcome-aware) — pick simpler approach for v0.1.0
- Signal handling approach for interrupt (graceful vs immediate Ctrl-C)
- Merge failure behavior (rollback vs keep partial) — align with existing MergeRunner rollback patterns
- State storage location — align with existing .smelt/ conventions
- Debug state preservation details for failed sessions

</decisions>

<specifics>
## Specific Ideas

- Dashboard should feel like docker-compose up — live-updating, multi-line, showing all sessions at once
- --json flag convention already established by merge plan command — reuse consistently
- comfy-table already in use for merge plan output — reuse for orchestration summary

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 08-orchestration-plan-task-graph*
*Context gathered: 2026-03-10*
