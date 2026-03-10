# Phase 8: Orchestration Plan & Task Graph - Research

**Researched:** 2026-03-10
**Domain:** DAG-based task orchestration, parallel async execution, terminal dashboards
**Confidence:** HIGH

## Summary

Phase 8 transforms Smelt from a sequential session runner into a DAG-based parallel orchestrator. The existing codebase provides all the building blocks: `SessionRunner` for session execution, `MergeRunner` for merging, `WorktreeManager` for worktree lifecycle, and `WorktreeState` for per-session state files in `.smelt/worktrees/`. The work is primarily composition — building a new `Orchestrator` that coordinates these existing components according to a dependency graph.

The standard approach uses `petgraph` for DAG representation/validation, `tokio::task::JoinSet` for parallel session execution, `tokio_util::sync::CancellationToken` for graceful shutdown, and `indicatif::MultiProgress` for the live status dashboard. All are mature, well-documented Rust crates.

**Primary recommendation:** Build a thin orchestration layer that owns the DAG, dispatches ready tasks via JoinSet, and delegates all actual work to existing `SessionRunner` / `MergeRunner` / `WorktreeManager` types. Keep the orchestrator stateless between lifecycle steps by persisting all progress to `.smelt/runs/<run-id>/`.

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
| --- | --- | --- | --- |
| petgraph | 0.7 | DAG representation, cycle detection, topological sort | De facto Rust graph library; 100M+ downloads; provides `toposort()`, `is_cyclic_directed()` out of the box |
| tokio | 1.x (already in workspace) | JoinSet for parallel task dispatch, select! for cancellation | Already the project's async runtime; JoinSet is the idiomatic tool for spawning N tasks and collecting results |
| tokio-util | 0.7 | CancellationToken for cooperative graceful shutdown | Standard companion to tokio for cancellation patterns; child tokens for per-session cancellation |
| indicatif | 0.17 | MultiProgress for live terminal dashboard | De facto Rust progress bar library; MultiProgress handles multi-line concurrent display exactly like docker-compose |

### Supporting

| Library | Version | Purpose | When to Use |
| --- | --- | --- | --- |
| console | 0.16 (already in workspace) | Terminal capability detection, color support | Already used; pair with indicatif for TTY detection |
| comfy-table | 7 (already in workspace) | Post-completion summary table | Already used for merge plan output; reuse for orchestration summary |
| serde_json | 1 (already in workspace) | --json output for orchestration results | Already used; reuse for structured output |
| chrono | 0.4 (already in workspace) | Timestamps for run state, elapsed time | Already used in WorktreeState |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
| --- | --- | --- |
| petgraph | Hand-rolled adjacency list | petgraph is ~50KB; hand-rolling saves a dep but loses proven toposort/cycle detection — not worth it |
| indicatif MultiProgress | Raw crossterm/console cursor manipulation | Much more code for the same result; indicatif handles terminal resize, non-TTY fallback, refresh rate limiting |
| tokio-util CancellationToken | Manual AtomicBool + Notify | CancellationToken has child token support and is cancel-safe; AtomicBool lacks hierarchical cancellation |

**Installation:**
```toml
# Add to [workspace.dependencies]
petgraph = "0.7"
indicatif = "0.17"
tokio-util = { version = "0.7", features = ["rt"] }

# Add tokio "signal" feature (for ctrl-c handling)
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "signal"] }
```

## Architecture Patterns

### Recommended Module Structure
```
crates/smelt-core/src/
├── orchestrate/
│   ├── mod.rs           # Orchestrator struct, public API
│   ├── dag.rs           # DAG construction from manifest, validation, ready-set computation
│   ├── state.rs         # Run state persistence (.smelt/runs/<id>/)
│   ├── executor.rs      # Parallel session dispatch via JoinSet
│   └── types.rs         # OrchestrationPlan, RunState, SessionNode, FailurePolicy, etc.
├── session/
│   └── manifest.rs      # Extended with depends_on, parallel_by_default, on_failure
└── ...existing modules unchanged...

crates/smelt-cli/src/commands/
├── orchestrate.rs       # CLI command: `smelt orchestrate run <manifest>`
└── ...existing commands unchanged...
```

### Pattern 1: DAG Construction from Manifest

**What:** Parse manifest sessions into a petgraph `DiGraph`, validate (cycle detection, dangling refs), compute execution layers.

**When to use:** At plan validation time, before any execution begins.

**Example:**
```rust
use petgraph::graph::DiGraph;
use petgraph::algo::{toposort, is_cyclic_directed};
use std::collections::HashMap;

struct SessionNode {
    session_name: String,
    // index into manifest sessions
}

fn build_dag(manifest: &Manifest) -> Result<DiGraph<SessionNode, ()>> {
    let mut graph = DiGraph::new();
    let mut name_to_idx = HashMap::new();

    // Add nodes
    for session in &manifest.sessions {
        let idx = graph.add_node(SessionNode {
            session_name: session.name.clone(),
        });
        name_to_idx.insert(session.name.clone(), idx);
    }

    // Add edges from depends_on
    for session in &manifest.sessions {
        if let Some(ref deps) = session.depends_on {
            let to = name_to_idx[&session.name];
            for dep in deps {
                let from = name_to_idx.get(dep)
                    .ok_or_else(|| SmeltError::ManifestParse(
                        format!("session '{}' depends on unknown session '{}'", session.name, dep)
                    ))?;
                graph.add_edge(*from, to, ());
            }
        }
    }

    // If parallel_by_default=false and no explicit depends_on,
    // add sequential chain edges
    if !manifest.manifest.parallel_by_default.unwrap_or(true) {
        // Sessions without explicit depends_on get implicit chain
        // session[0] -> session[1] -> session[2] ...
        let no_deps: Vec<_> = manifest.sessions.iter()
            .filter(|s| s.depends_on.is_none())
            .collect();
        for window in no_deps.windows(2) {
            let from = name_to_idx[&window[0].name];
            let to = name_to_idx[&window[1].name];
            graph.add_edge(from, to, ());
        }
    }

    // Validate: no cycles
    if is_cyclic_directed(&graph) {
        return Err(SmeltError::ManifestParse("dependency cycle detected".into()));
    }

    Ok(graph)
}
```

### Pattern 2: Ready-Set Execution with JoinSet

**What:** Iteratively find sessions with all dependencies satisfied, spawn them into a JoinSet, collect results, repeat until all done or failure.

**When to use:** The core execution loop of the orchestrator.

**Example:**
```rust
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

async fn execute_dag(
    dag: &DiGraph<SessionNode, ()>,
    cancel: CancellationToken,
) -> Vec<SessionResult> {
    let mut completed: HashSet<NodeIndex> = HashSet::new();
    let mut results = Vec::new();
    let mut join_set = JoinSet::new();
    let mut in_flight: HashMap<AbortHandle, NodeIndex> = HashMap::new();

    loop {
        // Find ready nodes: all predecessors completed
        let ready: Vec<NodeIndex> = dag.node_indices()
            .filter(|&n| !completed.contains(&n) && !in_flight.values().any(|v| v == &n))
            .filter(|&n| dag.neighbors_directed(n, petgraph::Direction::Incoming)
                .all(|pred| completed.contains(&pred)))
            .collect();

        // Spawn ready sessions
        for node_idx in ready {
            let session = dag[node_idx].clone();
            let child_cancel = cancel.child_token();
            let handle = join_set.spawn(async move {
                // Run session with cancellation support
                tokio::select! {
                    result = run_single_session(&session) => (node_idx, result),
                    _ = child_cancel.cancelled() => (node_idx, SessionResult::cancelled()),
                }
            });
            in_flight.insert(handle, node_idx);
        }

        if join_set.is_empty() {
            break; // All done
        }

        // Wait for next completion
        tokio::select! {
            Some(result) = join_set.join_next() => {
                let (node_idx, session_result) = result.unwrap();
                completed.insert(node_idx);
                results.push(session_result);
                // Handle failure policy...
            }
            _ = cancel.cancelled() => {
                // Abort all in-flight tasks
                join_set.abort_all();
                break;
            }
        }
    }
    results
}
```

### Pattern 3: Run State Persistence for Resume

**What:** Persist orchestration progress to `.smelt/runs/<run-id>/state.json` after each lifecycle step (session completion, merge step). On startup, detect incomplete runs and offer resume.

**When to use:** Crash recovery and interrupt-resume.

**State file structure:**
```
.smelt/runs/<run-id>/
├── state.json          # Current orchestration state
├── manifest.toml       # Copy of the manifest used
└── logs/
    ├── session-a.log   # stdout/stderr capture per session
    └── session-b.log
```

**State schema:**
```rust
#[derive(Serialize, Deserialize)]
struct RunState {
    run_id: String,
    manifest_name: String,
    started_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    phase: RunPhase,  // Sessions | Merging | Complete | Failed
    sessions: HashMap<String, SessionRunState>,
    merge_progress: Option<MergeProgress>,
    failure_policy: FailurePolicy,
}

#[derive(Serialize, Deserialize)]
enum SessionRunState {
    Pending,
    Running,
    Completed { duration_secs: f64 },
    Failed { reason: String },
    Skipped { reason: String },
    Cancelled,
}
```

### Pattern 4: Live Dashboard with indicatif

**What:** MultiProgress with one ProgressBar per session, transitioning through states.

**When to use:** During execution, when stdout is a TTY.

**Example:**
```rust
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

let mp = MultiProgress::new();
let style_pending = ProgressStyle::with_template("  {prefix:.dim} {msg}").unwrap();
let style_running = ProgressStyle::with_template("  {spinner:.green} {prefix} {msg} ({elapsed})").unwrap();
let style_done = ProgressStyle::with_template("  {prefix:.green} {msg} ({elapsed})").unwrap();
let style_fail = ProgressStyle::with_template("  {prefix:.red} {msg}").unwrap();

// Create a bar per session
for session in &sessions {
    let pb = mp.add(ProgressBar::new_spinner());
    pb.set_prefix(&session.name);
    pb.set_style(style_pending.clone());
    pb.set_message("pending");
    // Store pb handle, update style/message on state transitions
}
```

### Anti-Patterns to Avoid

- **Spawning one OS thread per session:** Use tokio tasks, not threads. Sessions are I/O-bound (git operations), not CPU-bound.
- **Polling the DAG on a timer:** Use event-driven JoinSet completion to trigger next-ready computation. No busy loops.
- **Storing run state in memory only:** Always persist to disk after each state transition. This is the entire basis for resume capability.
- **Coupling the dashboard to the orchestrator:** The dashboard should observe state changes via a channel or shared state, not be embedded in execution logic.
- **Re-implementing topological sort:** Use petgraph's `toposort()`. It handles edge cases (disconnected subgraphs, self-loops) correctly.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
| --- | --- | --- | --- |
| Cycle detection in dependency graph | DFS with visited set | `petgraph::algo::is_cyclic_directed` | Edge cases with disconnected components, self-loops |
| Topological ordering | Manual Kahn's algorithm | `petgraph::algo::toposort` | Returns `Err(Cycle)` on cyclic input, handles all graph shapes |
| Parallel task collection | Manual Vec of JoinHandles | `tokio::task::JoinSet` | JoinSet handles abort-on-drop, unordered completion, panic propagation |
| Cooperative cancellation | AtomicBool + manual checking | `tokio_util::sync::CancellationToken` | Hierarchical (child tokens), cancel-safe, works with `select!` |
| Multi-line terminal progress | Raw ANSI escape sequences | `indicatif::MultiProgress` | Handles terminal resize, non-TTY detection, refresh rate limiting |

**Key insight:** The orchestrator's value is in the composition and state management, not in reimplementing graph algorithms or terminal rendering.

## Common Pitfalls

### Pitfall 1: JoinSet task panic propagation
**What goes wrong:** A spawned task panics, and `join_next()` returns `Err(JoinError)` which is not a `SessionResult`. The orchestrator crashes or loses track of which session failed.
**Why it happens:** `JoinSet::join_next()` returns `Result<T, JoinError>` where `JoinError` can be either cancellation or panic.
**How to avoid:** Always match on `JoinError::is_cancelled()` vs `JoinError::is_panic()`. Map panics to `SessionOutcome::Failed` with the panic message. Never `unwrap()` join results.
**Warning signs:** Tests that never test session panics.

### Pitfall 2: Deadlock from dependency on failed session
**What goes wrong:** Session A fails, Session B depends on A, but the executor keeps waiting for B to become ready (it never will).
**Why it happens:** The ready-set computation only checks "predecessors completed" but doesn't distinguish success from failure.
**How to avoid:** When a session fails under `skip-dependents` policy, mark all transitive dependents as `Skipped` immediately. Under `abort` policy, cancel everything.
**Warning signs:** Integration tests that hang.

### Pitfall 3: Worktree contention during parallel creation
**What goes wrong:** Two sessions created simultaneously race on `git worktree add`, causing index lock contention.
**Why it happens:** Git uses `.git/index.lock` which is process-wide. Parallel `git worktree add` calls can collide.
**How to avoid:** Serialize worktree creation (create all worktrees before spawning parallel sessions). This is a brief sequential phase — the parallelism is in session execution, not worktree setup.
**Warning signs:** Intermittent "Unable to create '.git/index.lock': File exists" errors in CI.

### Pitfall 4: Dashboard rendering on non-TTY
**What goes wrong:** indicatif progress bars produce garbage output when piped or in CI.
**Why it happens:** MultiProgress tries to use terminal control sequences that don't work in non-TTY contexts.
**How to avoid:** Check `console::Term::stderr().is_term()` before creating the dashboard. Fall back to simple line-by-line status messages for non-TTY (e.g., `[session-a] completed in 3.2s`).
**Warning signs:** Broken output in `cargo test` or CI logs.

### Pitfall 5: Resume detection false positives
**What goes wrong:** A stale `.smelt/runs/` entry from a previous run causes the orchestrator to incorrectly offer resume.
**Why it happens:** Run state wasn't cleaned up after successful completion, or manifest changed between runs.
**How to avoid:** (1) Clean up run state on successful completion. (2) When detecting incomplete runs, verify the manifest hash matches. (3) Include the manifest content hash in run state.
**Warning signs:** "Previous run found. Resume?" appearing unexpectedly.

### Pitfall 6: Merge phase after partial session completion
**What goes wrong:** The merge phase tries to merge sessions that were skipped or failed, producing confusing errors.
**Why it happens:** The orchestrator passes all sessions to MergeRunner instead of only successful ones.
**How to avoid:** Filter to `SessionRunState::Completed` sessions before entering merge phase. The existing MergeRunner already handles this (it reads WorktreeState and skips non-Completed), but the orchestrator should also pre-filter for clarity.
**Warning signs:** Merge errors about missing branches for skipped sessions.

## Code Examples

### Manifest Extension for Dependencies

```toml
[manifest]
name = "feature-rollout"
base_ref = "main"
parallel_by_default = true
on_failure = "skip-dependents"

[[session]]
name = "database-migration"
task = "Add user_preferences table"
file_scope = ["migrations/**"]

[[session]]
name = "backend-api"
task = "Add preferences API endpoints"
file_scope = ["src/api/**"]
depends_on = ["database-migration"]

[[session]]
name = "frontend-ui"
task = "Add preferences settings page"
file_scope = ["src/ui/**"]
depends_on = ["backend-api"]

[[session]]
name = "documentation"
task = "Update API docs"
file_scope = ["docs/**"]
# No depends_on + parallel_by_default=true → runs concurrently with everything
```

### Graceful Shutdown with CancellationToken + ctrl_c

```rust
use tokio_util::sync::CancellationToken;

async fn orchestrate_with_shutdown(manifest: &Manifest) -> Result<OrchestrationReport> {
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    // Spawn signal handler
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Ctrl-C received, initiating graceful shutdown...");
        cancel_clone.cancel();
    });

    let orchestrator = Orchestrator::new(git, repo_root);
    orchestrator.run(manifest, cancel).await
}
```

### Serialized Worktree Creation, Parallel Execution

```rust
// Phase 1: Create all worktrees sequentially (avoids git index lock contention)
let mut worktree_map = HashMap::new();
for session in &manifest.sessions {
    let info = manager.create(CreateWorktreeOpts { ... }).await?;
    worktree_map.insert(session.name.clone(), info);
}

// Phase 2: Execute sessions in parallel according to DAG
let results = executor.run_dag(&dag, &worktree_map, cancel).await;

// Phase 3: Merge completed sessions sequentially (existing MergeRunner)
let merge_report = merge_runner.run(&filtered_manifest, opts, handler).await?;
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
| --- | --- | --- | --- |
| Manual JoinHandle Vec | `tokio::task::JoinSet` | tokio 1.19 (2022) | Cleaner API for spawn-N-collect-any pattern; abort-on-drop safety |
| `Arc<AtomicBool>` for shutdown | `CancellationToken` from tokio-util | tokio-util 0.7 (2022) | Hierarchical cancellation, cancel-safe futures, child tokens |
| petgraph 0.6 | petgraph 0.7 | 2024 | New `Acyclic<G>` wrapper type, improved topo sort; 0.6 API still works |

**Deprecated/outdated:**
- `tokio::spawn` + manual `Vec<JoinHandle>`: Still works but JoinSet is strictly better for the collect-any-result pattern.
- `petgraph::algo::scc`: Renamed to `kosaraju_scc` — use the new name.

## Open Questions

1. **Run ID generation scheme**
   - What we know: Needs to be unique, sortable, human-readable for `.smelt/runs/<id>/`
   - What's unclear: Timestamp-based (e.g., `20260310-143022`) vs UUID vs manifest-name + counter
   - Recommendation: Use `<manifest-name>-<YYYYMMDD-HHMMSS>` for readability. Collision risk is negligible (sub-second orchestration starts are implausible).

2. **DAG complexity limits**
   - What we know: petgraph handles large graphs fine; performance is not a concern for realistic session counts (< 100)
   - What's unclear: Should we warn on e.g., > 50 sessions? Or just let it run?
   - Recommendation: Skip complexity warnings for v0.1.0. Add if users report confusion with large graphs.

3. **Dependency outcome semantics**
   - What we know: CONTEXT.md says "pick simpler approach for v0.1.0"
   - What's unclear: Should `depends_on` mean "waits for completion regardless of success/failure" or "waits for successful completion only"?
   - Recommendation: **Completion-only** (simpler). A dependency is "satisfied" when the session finishes, regardless of outcome. The `on_failure` policy then governs what happens: `skip-dependents` marks dependents of *failed* sessions as skipped; `abort` stops everything. This avoids needing `depends_on` to encode outcome awareness.

4. **Merge failure behavior**
   - What we know: CONTEXT.md says "align with existing MergeRunner rollback patterns"
   - What's unclear: Does a merge conflict during the merge phase abort the entire orchestration, or just skip that session?
   - Recommendation: Delegate to the existing `ConflictHandler` trait. The orchestrator doesn't add new merge-phase behavior — it just calls `MergeRunner::run()` with the same handler the CLI currently passes. This keeps merge behavior consistent whether run standalone or via orchestrator.

## Sources

### Primary (HIGH confidence)
- Context7: `/websites/rs_tokio_1_49_0` — JoinSet API, spawn, join_next, abort, signal::ctrl_c
- Context7: `/websites/rs_petgraph` — toposort, is_cyclic_directed, DiGraph API, DAG adjacency list conversion
- Context7: `/websites/rs_indicatif` — MultiProgress, ProgressBar, ProgressStyle, add/insert
- Context7: `/websites/rs_tokio-util` — CancellationToken, child_token, cancelled(), cancel()
- Codebase: `smelt-core/src/session/runner.rs` — existing SessionRunner sequential execution
- Codebase: `smelt-core/src/merge/mod.rs` — existing MergeRunner with ConflictHandler trait
- Codebase: `smelt-core/src/worktree/state.rs` — WorktreeState persistence pattern
- Codebase: `smelt-core/src/session/process.rs` — ProcessGroup SIGTERM pattern

### Secondary (MEDIUM confidence)
- petgraph 0.7 release (verified via Context7 docs showing Acyclic wrapper and current API)

### Tertiary (LOW confidence)
- None — all critical claims verified through Context7 or codebase inspection

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all libraries verified via Context7 with current API docs
- Architecture: HIGH — patterns derived from existing codebase conventions and verified tokio/petgraph APIs
- Pitfalls: HIGH — git index lock contention is a well-documented issue; JoinSet panic handling is documented in tokio API; other pitfalls derived from code inspection

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable ecosystem, 30-day validity)
