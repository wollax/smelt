# Phase 08 Plan 01: Orchestration Foundation Summary

**One-liner:** DAG-based orchestration types, manifest dependency extensions, and petgraph-backed session graph with cycle detection and ready-set computation.

## Frontmatter

- **Phase:** 08-orchestration-plan-task-graph
- **Plan:** 01
- **Subsystem:** orchestration
- **Tags:** petgraph, DAG, manifest, dependency-graph, run-state, serde
- **Completed:** 2026-03-10
- **Duration:** ~11 minutes

### Dependencies

- **Requires:** Phases 01-07 (core lib, session manifest, merge types)
- **Provides:** Orchestration types, DAG builder, manifest dependency/failure semantics, run state persistence
- **Affects:** 08-02 (parallel executor), 08-03 (CLI + dashboard)

### Tech Stack

- **Added:** petgraph 0.7, indicatif 0.17, tokio-util 0.7
- **Patterns:** DiGraph for session DAG, BFS for dependent propagation, serde-tagged enums for run state

### Key Files

**Created:**
- `crates/smelt-core/src/orchestrate/mod.rs` — module declaration and re-exports
- `crates/smelt-core/src/orchestrate/types.rs` — FailurePolicy, SessionRunState, RunState, RunPhase, OrchestrationReport, OrchestrationOpts, MergeProgress
- `crates/smelt-core/src/orchestrate/dag.rs` — build_dag(), ready_set(), mark_skipped_dependents(), node_by_name()

**Modified:**
- `Cargo.toml` — workspace deps (petgraph, indicatif, tokio-util), tokio signal feature
- `crates/smelt-core/Cargo.toml` — petgraph, tokio-util
- `crates/smelt-cli/Cargo.toml` — indicatif, tokio-util
- `crates/smelt-core/src/error.rs` — Orchestration and DependencyCycle variants
- `crates/smelt-core/src/session/manifest.rs` — depends_on, parallel_by_default, on_failure fields + validation + 8 tests
- `crates/smelt-core/src/session/runner.rs` — added depends_on: None to test constructors
- `crates/smelt-core/src/merge/mod.rs` — added new ManifestMeta/SessionDef fields to test constructor
- `crates/smelt-core/src/lib.rs` — pub mod orchestrate + re-exports

### Decisions

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Cycle detection via petgraph in both manifest validate() and build_dag() | Belt and suspenders — validate catches it early with user-friendly error, build_dag is the authoritative check |
| 2 | ready_set treats skipped deps as satisfied | Allows independent sessions to proceed when SkipDependents policy is active |
| 3 | RunState persists as state.json (not TOML) | JSON matches serde tagged enum serialization naturally; TOML struggles with enum variants |
| 4 | FailurePolicy::from(Option<&str>) defaults to SkipDependents for unknown values | Safer default — unknown policies should not abort |
| 5 | node_by_name() is O(n) linear scan | Acceptable for realistic session counts (<100); HashMap lookup would be premature optimization |

## Metrics

| Metric | Value |
|--------|-------|
| Tasks completed | 2/2 |
| Tests added | 40 (8 manifest + 13 DAG + 19 types) |
| Total tests passing | 188 |
| Files created | 3 |
| Files modified | 7 |

## Commits

| Hash | Description |
|------|-------------|
| 59d78d0 | feat(08-01): workspace deps, error variants, manifest extensions |
| a486074 | feat(08-01): orchestration types, DAG builder, run state persistence |

## Deviations from Plan

None — plan executed exactly as written.

## Next Phase Readiness

The orchestration foundation is complete. Plan 08-02 can build the parallel executor using:
- `build_dag()` to construct the session dependency graph
- `ready_set()` to find executable sessions at each step
- `mark_skipped_dependents()` for failure propagation
- `RunState` for crash-recovery persistence
- `FailurePolicy` and `SessionRunState` for execution lifecycle
- `OrchestrationOpts` and `OrchestrationReport` for input/output

No blockers or concerns.
