//! Orchestrator execution engine — parallel session dispatch, failure policy,
//! state persistence, and merge phase integration.

use std::collections::HashSet;
use std::path::PathBuf;

use chrono::Utc;
use petgraph::graph::NodeIndex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::error::{Result, SmeltError};
use crate::git::GitOps;
use crate::merge::{ConflictHandler, MergeOpts, MergeReport, MergeRunner};
use crate::orchestrate::dag::{build_dag, mark_skipped_dependents, node_by_name, ready_set};
use crate::orchestrate::state::{compute_manifest_hash, RunStateManager};
use crate::orchestrate::types::{
    FailurePolicy, OrchestrationOpts, OrchestrationReport, RunPhase, RunState, SessionRunState,
};
use crate::session::manifest::Manifest;
use crate::session::script::ScriptExecutor;
use crate::session::types::SessionOutcome;
use crate::worktree::{CreateWorktreeOpts, WorktreeManager};

/// Orchestrates the full execution lifecycle: DAG validation, worktree creation,
/// parallel session execution, state persistence, and merge phase delegation.
pub struct Orchestrator<G: GitOps + Clone> {
    git: G,
    repo_root: PathBuf,
}

impl<G: GitOps + Clone + Send + Sync + 'static> Orchestrator<G> {
    /// Create a new `Orchestrator`.
    pub fn new(git: G, repo_root: PathBuf) -> Self {
        Self { git, repo_root }
    }

    /// Execute a full orchestration lifecycle.
    ///
    /// 1. Validate and build DAG
    /// 2. Create worktrees (sequential)
    /// 3. Execute sessions (parallel via JoinSet)
    /// 4. Merge completed sessions (sequential via MergeRunner)
    /// 5. Return orchestration report
    pub async fn run<H: ConflictHandler>(
        &self,
        manifest: &Manifest,
        manifest_content: &str,
        opts: &OrchestrationOpts,
        conflict_handler: &H,
        cancel: CancellationToken,
        on_status: impl Fn(&str, &SessionRunState) + Send + Sync,
    ) -> Result<OrchestrationReport> {
        let start = tokio::time::Instant::now();

        // Phase 0: Validate & build DAG
        let dag = build_dag(manifest)?;
        let failure_policy = manifest.manifest.on_failure.unwrap_or_default();

        // Initialize run state
        let manifest_hash = compute_manifest_hash(manifest_content);
        let run_id = RunState::generate_run_id(&manifest.manifest.name);
        let session_names: Vec<String> =
            manifest.sessions.iter().map(|s| s.name.clone()).collect();
        let mut run_state = RunState::new(
            run_id.clone(),
            manifest.manifest.name.clone(),
            manifest_hash,
            failure_policy,
            &session_names,
        );

        let smelt_dir = self.repo_root.join(".smelt");
        let state_manager = RunStateManager::new(&smelt_dir);
        state_manager.save_state(&run_state)?;

        // Phase 1: Create worktrees (sequential)
        let manager = WorktreeManager::new(self.git.clone(), self.repo_root.clone());
        for session_def in &manifest.sessions {
            if cancel.is_cancelled() {
                self.mark_remaining_cancelled(&mut run_state, &on_status);
                state_manager.save_state(&run_state)?;
                return Ok(self.build_report(run_state, None, None, start.elapsed().as_secs_f64()));
            }

            let base_ref = session_def
                .base_ref
                .clone()
                .unwrap_or_else(|| manifest.manifest.base_ref.clone());

            match manager
                .create(CreateWorktreeOpts {
                    session_name: session_def.name.clone(),
                    base: base_ref,
                    dir_name: None,
                    task_description: session_def.task.clone(),
                    file_scope: session_def.file_scope.clone(),
                })
                .await
            {
                Ok(_info) => {}
                Err(e) => {
                    let reason = format!("worktree creation failed: {e}");
                    run_state.sessions.insert(
                        session_def.name.clone(),
                        SessionRunState::Failed {
                            reason: reason.clone(),
                        },
                    );
                    run_state.updated_at = Utc::now();
                    on_status(
                        &session_def.name,
                        &SessionRunState::Failed { reason },
                    );
                    state_manager.save_state(&run_state)?;

                    // Apply failure policy
                    if failure_policy == FailurePolicy::Abort {
                        run_state.phase = RunPhase::Failed;
                        state_manager.save_state(&run_state)?;
                        return Err(SmeltError::Orchestration {
                            message: format!(
                                "worktree creation failed for '{}', aborting",
                                session_def.name
                            ),
                        });
                    }

                    // SkipDependents
                    if let Some(node) = node_by_name(&dag, &session_def.name) {
                        let mut skipped_indices = HashSet::new();
                        mark_skipped_dependents(&dag, node, &mut skipped_indices);
                        for idx in &skipped_indices {
                            let name = &dag[*idx];
                            let skip_state = SessionRunState::Skipped {
                                reason: format!(
                                    "dependency '{}' failed worktree creation",
                                    session_def.name
                                ),
                            };
                            run_state
                                .sessions
                                .insert(name.clone(), skip_state.clone());
                            on_status(name, &skip_state);
                        }
                        run_state.updated_at = Utc::now();
                        state_manager.save_state(&run_state)?;
                    }
                }
            }
        }

        // Phase 2: Execute sessions (parallel via JoinSet)
        self.execute_sessions(
            manifest,
            &dag,
            &mut run_state,
            &state_manager,
            failure_policy,
            &cancel,
            &on_status,
        )
        .await?;

        // Check for abort after sessions
        if cancel.is_cancelled() {
            self.mark_remaining_cancelled(&mut run_state, &on_status);
            run_state.phase = RunPhase::Failed;
            state_manager.save_state(&run_state)?;
            return Ok(self.build_report(run_state, None, None, start.elapsed().as_secs_f64()));
        }

        // Phase 2.5: Collect summary (pre-merge analysis)
        let summary_report = match crate::summary::collect_summary(
            &self.git,
            manifest,
            &run_state.sessions,
            &run_state.run_id,
        )
        .await
        {
            Ok(report) => {
                state_manager.save_summary(&run_state.run_id, &report).ok();
                Some(report)
            }
            Err(e) => {
                warn!("Failed to collect summary: {e}");
                None
            }
        };

        // Phase 3: Merge
        let merge_report = self
            .merge_phase(manifest, &mut run_state, &state_manager, opts, conflict_handler)
            .await?;

        let elapsed = start.elapsed().as_secs_f64();
        state_manager.save_state(&run_state)?;

        Ok(self.build_report(run_state, merge_report, summary_report, elapsed))
    }

    /// Resume an incomplete orchestration run.
    #[allow(clippy::too_many_arguments)]
    pub async fn resume<H: ConflictHandler>(
        &self,
        manifest: &Manifest,
        manifest_content: &str,
        mut run_state: RunState,
        opts: &OrchestrationOpts,
        conflict_handler: &H,
        cancel: CancellationToken,
        on_status: impl Fn(&str, &SessionRunState) + Send + Sync,
    ) -> Result<OrchestrationReport> {
        let start = tokio::time::Instant::now();
        let smelt_dir = self.repo_root.join(".smelt");
        let state_manager = RunStateManager::new(&smelt_dir);

        // Validate manifest hash matches
        let current_hash = compute_manifest_hash(manifest_content);
        if current_hash != run_state.manifest_hash {
            return Err(SmeltError::Orchestration {
                message: "manifest has changed since the previous run — cannot resume".to_string(),
            });
        }

        match run_state.phase {
            RunPhase::Sessions => {
                // Re-execute from Phase 2 for non-terminal sessions
                let dag = build_dag(manifest)?;
                let failure_policy = run_state.failure_policy;

                self.execute_sessions(
                    manifest,
                    &dag,
                    &mut run_state,
                    &state_manager,
                    failure_policy,
                    &cancel,
                    &on_status,
                )
                .await?;

                if cancel.is_cancelled() {
                    self.mark_remaining_cancelled(&mut run_state, &on_status);
                    run_state.phase = RunPhase::Failed;
                    state_manager.save_state(&run_state)?;
                    return Ok(self.build_report(
                        run_state,
                        None,
                        None,
                        start.elapsed().as_secs_f64(),
                    ));
                }

                // Collect summary before merge
                let summary_report = match crate::summary::collect_summary(
                    &self.git,
                    manifest,
                    &run_state.sessions,
                    &run_state.run_id,
                )
                .await
                {
                    Ok(report) => {
                        state_manager.save_summary(&run_state.run_id, &report).ok();
                        Some(report)
                    }
                    Err(e) => {
                        warn!("Failed to collect summary: {e}");
                        None
                    }
                };

                // Proceed to merge
                let merge_report = self
                    .merge_phase(
                        manifest,
                        &mut run_state,
                        &state_manager,
                        opts,
                        conflict_handler,
                    )
                    .await?;

                let elapsed = start.elapsed().as_secs_f64();
                state_manager.save_state(&run_state)?;
                Ok(self.build_report(run_state, merge_report, summary_report, elapsed))
            }
            RunPhase::Merging => {
                // Load previously-persisted summary (if available)
                let summary_report = state_manager.load_summary(&run_state.run_id).ok();

                // Skip directly to merge
                let merge_report = self
                    .merge_phase(
                        manifest,
                        &mut run_state,
                        &state_manager,
                        opts,
                        conflict_handler,
                    )
                    .await?;

                let elapsed = start.elapsed().as_secs_f64();
                state_manager.save_state(&run_state)?;
                Ok(self.build_report(run_state, merge_report, summary_report, elapsed))
            }
            _ => Err(SmeltError::Orchestration {
                message: format!("run is in {:?} phase and cannot be resumed", run_state.phase),
            }),
        }
    }

    /// Execute sessions in parallel via JoinSet according to the DAG.
    #[allow(clippy::too_many_arguments)]
    async fn execute_sessions(
        &self,
        manifest: &Manifest,
        dag: &crate::orchestrate::dag::SessionDag,
        run_state: &mut RunState,
        state_manager: &RunStateManager,
        failure_policy: FailurePolicy,
        cancel: &CancellationToken,
        on_status: &(impl Fn(&str, &SessionRunState) + Send + Sync),
    ) -> Result<()> {
        let smelt_dir = self.repo_root.join(".smelt");

        // Build index sets from run_state
        let mut completed_set: HashSet<NodeIndex> = HashSet::new();
        let mut in_flight: HashSet<NodeIndex> = HashSet::new();
        let mut skipped_set: HashSet<NodeIndex> = HashSet::new();

        // Pre-populate from existing terminal states (for resume)
        for (name, state) in &run_state.sessions {
            if let Some(idx) = node_by_name(dag, name) {
                match state {
                    SessionRunState::Completed { .. } => {
                        completed_set.insert(idx);
                    }
                    SessionRunState::Failed { .. } => {
                        completed_set.insert(idx); // treat failed as "done" for scheduling
                    }
                    SessionRunState::Skipped { .. } => {
                        skipped_set.insert(idx);
                    }
                    SessionRunState::Cancelled => {
                        // Reset cancelled sessions to pending for re-execution
                        // (only relevant if resuming after cancellation somehow, but safe)
                    }
                    _ => {}
                }
            }
        }

        let mut join_set: JoinSet<(String, std::result::Result<SessionRunState, String>)> =
            JoinSet::new();

        loop {
            // Compute ready set
            let ready = ready_set(dag, &completed_set, &in_flight, &skipped_set);

            if ready.is_empty() && join_set.is_empty() {
                break;
            }

            // Spawn ready sessions
            for node_idx in ready {
                let session_name = dag[node_idx].clone();

                // Skip sessions already in terminal state (for resume)
                if let Some(state) = run_state.sessions.get(&session_name)
                    && state.is_terminal()
                {
                    continue;
                }

                in_flight.insert(node_idx);

                // Mark as running
                run_state
                    .sessions
                    .insert(session_name.clone(), SessionRunState::Running);
                run_state.updated_at = Utc::now();
                on_status(&session_name, &SessionRunState::Running);
                state_manager.save_state(run_state)?;

                // Look up session def
                let session_def = manifest
                    .sessions
                    .iter()
                    .find(|s| s.name == session_name)
                    .ok_or_else(|| SmeltError::Orchestration {
                        message: format!("session '{session_name}' not found in manifest"),
                    })?;

                // Resolve worktree path from state file
                let wt_state_file = smelt_dir
                    .join("worktrees")
                    .join(format!("{session_name}.toml"));

                let worktree_path = if wt_state_file.exists() {
                    let wt_state = crate::worktree::state::WorktreeState::load(&wt_state_file)?;
                    self.repo_root.join(&wt_state.worktree_path)
                } else {
                    // Worktree state file missing — session will fail
                    let reason = "worktree state file not found".to_string();
                    let fail_state = SessionRunState::Failed {
                        reason: reason.clone(),
                    };
                    run_state
                        .sessions
                        .insert(session_name.clone(), fail_state.clone());
                    run_state.updated_at = Utc::now();
                    on_status(&session_name, &fail_state);
                    in_flight.remove(&node_idx);
                    completed_set.insert(node_idx);

                    if failure_policy == FailurePolicy::Abort {
                        run_state.phase = RunPhase::Failed;
                        state_manager.save_state(run_state)?;
                        return Err(SmeltError::Orchestration {
                            message: format!("session '{session_name}' failed, aborting"),
                        });
                    }

                    let mut new_skipped = HashSet::new();
                    mark_skipped_dependents(dag, node_idx, &mut new_skipped);
                    for idx in &new_skipped {
                        let dep_name = &dag[*idx];
                        let skip_state = SessionRunState::Skipped {
                            reason: format!("dependency '{session_name}' failed"),
                        };
                        run_state
                            .sessions
                            .insert(dep_name.clone(), skip_state.clone());
                        on_status(dep_name, &skip_state);
                        skipped_set.insert(*idx);
                    }
                    state_manager.save_state(run_state)?;
                    continue;
                };

                let git = self.git.clone();
                let script = session_def.script.clone();
                let name_clone = session_name.clone();
                let child_cancel = cancel.child_token();
                let log_path = state_manager.log_path(&run_state.run_id, &session_name);

                // Clone name for panic-safety: the inner spawn catches panics
                // via JoinError, and the outer closure still has the name.
                let panic_name = name_clone.clone();
                join_set.spawn(async move {
                    // Spawn the session work as a nested task so that if it panics,
                    // the JoinError is caught here while we still have `panic_name`
                    // to identify the session. Without this, a panic loses the
                    // session identity, causing it to leak in the in_flight set.
                    let handle = tokio::task::spawn(async move {
                        let timer = tokio::time::Instant::now();

                        let result = if let Some(ref script_def) = script {
                            let executor = ScriptExecutor::new(&git, worktree_path);
                            tokio::select! {
                                biased;
                                _ = child_cancel.cancelled() => {
                                    Ok(SessionRunState::Cancelled)
                                }
                                res = executor.execute(&name_clone, script_def) => {
                                    match res {
                                        Ok(session_result) => {
                                            let duration = timer.elapsed().as_secs_f64();
                                            match session_result.outcome {
                                                SessionOutcome::Completed => {
                                                    Ok(SessionRunState::Completed { duration_secs: duration })
                                                }
                                                _ => {
                                                    Ok(SessionRunState::Failed {
                                                        reason: session_result
                                                            .failure_reason
                                                            .unwrap_or_else(|| "unknown failure".to_string()),
                                                    })
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            Ok(SessionRunState::Failed {
                                                reason: format!("execution error: {e}"),
                                            })
                                        }
                                    }
                                }
                            }
                        } else {
                            // No script — immediately complete
                            Ok(SessionRunState::Completed {
                                duration_secs: timer.elapsed().as_secs_f64(),
                            })
                        };

                        // Write log file (best effort)
                        if let Some(parent) = log_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let _ = std::fs::write(
                            &log_path,
                            format!("Session '{}' completed\n", name_clone),
                        );

                        match result {
                            Ok(state) => (name_clone, Ok(state)),
                            Err(msg) => (name_clone, Err(msg)),
                        }
                    });

                    match handle.await {
                        Ok(result) => result,
                        Err(join_error) => {
                            let msg = if join_error.is_cancelled() {
                                "task cancelled".to_string()
                            } else if let Ok(payload) = join_error.try_into_panic() {
                                if let Some(s) = payload.downcast_ref::<&str>() {
                                    format!("task panicked: {s}")
                                } else if let Some(s) = payload.downcast_ref::<String>() {
                                    format!("task panicked: {s}")
                                } else {
                                    "task panicked with unknown payload".to_string()
                                }
                            } else {
                                "task failed with unknown JoinError".to_string()
                            };
                            (panic_name, Err(msg))
                        }
                    }
                });
            }

            // Wait for completions or cancellation
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    // Cancel all in-flight
                    join_set.abort_all();
                    // Drain remaining results
                    while let Some(result) = join_set.join_next().await {
                        if let Ok((name, _)) = result {
                            run_state.sessions.insert(name.clone(), SessionRunState::Cancelled);
                            on_status(&name, &SessionRunState::Cancelled);
                        }
                    }
                    self.mark_remaining_cancelled(run_state, on_status);
                    run_state.phase = RunPhase::Failed;
                    state_manager.save_state(run_state)?;
                    return Ok(());
                }
                result = join_set.join_next(), if !join_set.is_empty() => {
                    let Some(result) = result else { continue };

                    let (session_name, session_state) = match result {
                        Ok((name, Ok(state))) => (name, state),
                        Ok((name, Err(msg))) => {
                            (name, SessionRunState::Failed { reason: msg })
                        }
                        Err(join_error) => {
                            // JoinError: task panicked or was cancelled.
                            // The session name is lost because JoinError doesn't carry it.
                            // This path should be unreachable — spawned tasks catch panics
                            // internally via catch_unwind and return them as Failed results.
                            if join_error.is_cancelled() {
                                continue;
                            }
                            let panic_msg = if let Ok(reason) = join_error.try_into_panic() {
                                if let Some(s) = reason.downcast_ref::<&str>() {
                                    format!("task panicked (uncaught): {s}")
                                } else if let Some(s) = reason.downcast_ref::<String>() {
                                    format!("task panicked (uncaught): {s}")
                                } else {
                                    "task panicked (uncaught) with unknown payload".to_string()
                                }
                            } else {
                                "task failed with unknown JoinError".to_string()
                            };
                            warn!("{}", panic_msg);
                            // Defensive: cannot map to a session. Log and continue — the
                            // session will remain in `in_flight` but the catch_unwind wrapper
                            // in the spawned task should prevent this path from being reached.
                            continue;
                        }
                    };

                    // Update node tracking
                    if let Some(node_idx) = node_by_name(dag, &session_name) {
                        in_flight.remove(&node_idx);

                        match &session_state {
                            SessionRunState::Completed { .. } => {
                                completed_set.insert(node_idx);
                            }
                            SessionRunState::Failed { .. } => {
                                completed_set.insert(node_idx);

                                // Apply failure policy
                                if failure_policy == FailurePolicy::Abort {
                                    // Cancel everything
                                    cancel.cancel();
                                    join_set.abort_all();

                                    // Drain
                                    while let Some(res) = join_set.join_next().await {
                                        if let Ok((n, _)) = res {
                                            run_state.sessions.insert(n.clone(), SessionRunState::Cancelled);
                                            on_status(&n, &SessionRunState::Cancelled);
                                        }
                                    }

                                    run_state.sessions.insert(
                                        session_name.clone(),
                                        session_state.clone(),
                                    );
                                    on_status(&session_name, &session_state);
                                    self.mark_remaining_cancelled(run_state, on_status);
                                    run_state.phase = RunPhase::Failed;
                                    run_state.updated_at = Utc::now();
                                    state_manager.save_state(run_state)?;
                                    return Err(SmeltError::Orchestration {
                                        message: format!(
                                            "session '{}' failed, aborting orchestration",
                                            session_name
                                        ),
                                    });
                                }

                                // SkipDependents
                                let mut new_skipped = HashSet::new();
                                mark_skipped_dependents(dag, node_idx, &mut new_skipped);
                                for idx in &new_skipped {
                                    let dep_name = &dag[*idx];
                                    let skip_state = SessionRunState::Skipped {
                                        reason: format!("dependency '{session_name}' failed"),
                                    };
                                    run_state.sessions.insert(
                                        dep_name.clone(),
                                        skip_state.clone(),
                                    );
                                    on_status(dep_name, &skip_state);
                                    skipped_set.insert(*idx);
                                }
                            }
                            SessionRunState::Cancelled => {
                                // Already handled
                            }
                            _ => {}
                        }
                    }

                    // Update worktree state file to reflect session outcome
                    self.update_worktree_state(&session_name, &session_state);

                    run_state.sessions.insert(session_name.clone(), session_state.clone());
                    run_state.updated_at = Utc::now();
                    on_status(&session_name, &session_state);
                    state_manager.save_state(run_state)?;
                }
            }
        }

        Ok(())
    }

    /// Run the merge phase: filter to completed sessions, delegate to MergeRunner.
    async fn merge_phase<H: ConflictHandler>(
        &self,
        manifest: &Manifest,
        run_state: &mut RunState,
        state_manager: &RunStateManager,
        opts: &OrchestrationOpts,
        conflict_handler: &H,
    ) -> Result<Option<MergeReport>> {
        // Filter manifest to completed sessions only
        let completed_names: HashSet<&str> = run_state
            .sessions
            .iter()
            .filter(|(_, s)| s.is_success())
            .map(|(name, _)| name.as_str())
            .collect();

        if completed_names.is_empty() {
            info!("No completed sessions — skipping merge phase");
            run_state.phase = RunPhase::Complete;
            run_state.updated_at = Utc::now();
            state_manager.save_state(run_state)?;
            return Ok(None);
        }

        // Build a filtered manifest with only completed sessions
        let filtered_manifest = Manifest {
            manifest: manifest.manifest.clone(),
            sessions: manifest
                .sessions
                .iter()
                .filter(|s| completed_names.contains(s.name.as_str()))
                .cloned()
                .collect(),
        };

        run_state.phase = RunPhase::Merging;
        run_state.updated_at = Utc::now();
        state_manager.save_state(run_state)?;

        let merge_runner = MergeRunner::new(self.git.clone(), self.repo_root.clone());
        let merge_opts = MergeOpts::new(opts.target_branch.clone(), opts.strategy);

        let merge_report = merge_runner
            .run(&filtered_manifest, merge_opts, conflict_handler)
            .await?;

        run_state.phase = RunPhase::Complete;
        run_state.updated_at = Utc::now();
        state_manager.save_state(run_state)?;

        Ok(Some(merge_report))
    }

    /// Update the worktree state file (`.smelt/worktrees/<session>.toml`) to reflect
    /// the session outcome. This is required for MergeRunner to recognize completed sessions.
    fn update_worktree_state(&self, session_name: &str, state: &SessionRunState) {
        use crate::worktree::state::{SessionStatus, WorktreeState};

        let state_file = self
            .repo_root
            .join(".smelt/worktrees")
            .join(format!("{session_name}.toml"));

        if !state_file.exists() {
            return;
        }

        let new_status = match state {
            SessionRunState::Completed { .. } => SessionStatus::Completed,
            SessionRunState::Failed { .. } => SessionStatus::Failed,
            _ => return, // Only update for terminal execution states
        };

        match WorktreeState::load(&state_file) {
            Ok(mut wt_state) => {
                wt_state.status = new_status;
                wt_state.updated_at = Utc::now();
                if let Err(e) = wt_state.save(&state_file) {
                    warn!(
                        "failed to update worktree state for session '{}': {e}",
                        session_name
                    );
                }
            }
            Err(e) => {
                warn!(
                    "failed to load worktree state for session '{}': {e}",
                    session_name
                );
            }
        }
    }

    /// Mark all non-terminal sessions as cancelled.
    fn mark_remaining_cancelled(
        &self,
        run_state: &mut RunState,
        on_status: &(impl Fn(&str, &SessionRunState) + Send + Sync),
    ) {
        let non_terminal: Vec<String> = run_state
            .sessions
            .iter()
            .filter(|(_, s)| !s.is_terminal())
            .map(|(name, _)| name.clone())
            .collect();

        for name in non_terminal {
            run_state
                .sessions
                .insert(name.clone(), SessionRunState::Cancelled);
            on_status(&name, &SessionRunState::Cancelled);
        }
        run_state.updated_at = Utc::now();
    }

    /// Build the final orchestration report.
    fn build_report(
        &self,
        run_state: RunState,
        merge_report: Option<MergeReport>,
        summary: Option<crate::summary::SummaryReport>,
        elapsed_secs: f64,
    ) -> OrchestrationReport {
        let outcome = run_state.phase;
        OrchestrationReport {
            run_id: run_state.run_id,
            manifest_name: run_state.manifest_name,
            session_results: run_state.sessions,
            merge_report,
            summary,
            elapsed_secs,
            outcome,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitCli;
    use crate::merge::NoopConflictHandler;
    use crate::session::manifest::{
        FileChange, ManifestMeta, ScriptDef, ScriptStep, SessionDef,
    };
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    /// Create a temporary git repo with an initial commit and .smelt/ initialized.
    fn setup_test_repo() -> (tempfile::TempDir, GitCli, PathBuf) {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let repo_path = tmp.path().join("test-repo");
        std::fs::create_dir(&repo_path).expect("create repo dir");

        let git = which::which("git").expect("git on PATH");

        Command::new(&git)
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .expect("git init");

        for args in [
            &["config", "user.email", "test@example.com"][..],
            &["config", "user.name", "Test"][..],
        ] {
            Command::new(&git)
                .args(args)
                .current_dir(&repo_path)
                .output()
                .expect("git config");
        }

        std::fs::write(repo_path.join("README.md"), "# test\n").unwrap();
        Command::new(&git)
            .args(["add", "README.md"])
            .current_dir(&repo_path)
            .output()
            .expect("git add");
        Command::new(&git)
            .args(["commit", "-m", "initial"])
            .current_dir(&repo_path)
            .output()
            .expect("git commit");

        crate::init::init_project(&repo_path).expect("init_project");
        std::fs::create_dir_all(repo_path.join(".smelt/worktrees"))
            .expect("create worktrees dir");

        let cli = GitCli::new(git, repo_path.clone());
        (tmp, cli, repo_path)
    }

    fn scripted_session(name: &str, steps: Vec<ScriptStep>) -> SessionDef {
        SessionDef {
            name: name.to_string(),
            task: Some(format!("Task for {name}")),
            task_file: None,
            file_scope: None,
            base_ref: None,
            timeout_secs: None,
            env: None,
            depends_on: None,
            script: Some(ScriptDef {
                backend: "scripted".to_string(),
                exit_after: None,
                simulate_failure: None,
                steps,
            }),
        }
    }

    fn scripted_session_with_deps(
        name: &str,
        deps: Vec<&str>,
        steps: Vec<ScriptStep>,
    ) -> SessionDef {
        SessionDef {
            depends_on: Some(deps.into_iter().map(String::from).collect()),
            ..scripted_session(name, steps)
        }
    }

    fn failing_session(name: &str) -> SessionDef {
        SessionDef {
            name: name.to_string(),
            task: Some(format!("Task for {name}")),
            task_file: None,
            file_scope: None,
            base_ref: None,
            timeout_secs: None,
            env: None,
            depends_on: None,
            script: Some(ScriptDef {
                backend: "scripted".to_string(),
                exit_after: None,
                simulate_failure: Some(crate::session::manifest::FailureMode::Crash),
                steps: vec![ScriptStep::Commit {
                    message: "will fail".to_string(),
                    files: vec![FileChange {
                        path: format!("{name}.txt"),
                        content: Some("content\n".to_string()),
                        content_file: None,
                    }],
                }],
            }),
        }
    }

    fn commit_step(message: &str, files: Vec<(&str, &str)>) -> ScriptStep {
        ScriptStep::Commit {
            message: message.to_string(),
            files: files
                .into_iter()
                .map(|(path, content)| FileChange {
                    path: path.to_string(),
                    content: Some(content.to_string()),
                    content_file: None,
                })
                .collect(),
        }
    }

    fn make_manifest(
        name: &str,
        sessions: Vec<SessionDef>,
        on_failure: Option<FailurePolicy>,
    ) -> Manifest {
        Manifest {
            manifest: ManifestMeta {
                name: name.to_string(),
                base_ref: "HEAD".to_string(),
                merge_strategy: None,
                parallel_by_default: true,
                on_failure,
                shared_files: vec![],
            },
            sessions,
        }
    }

    fn default_opts() -> OrchestrationOpts {
        OrchestrationOpts::default()
    }

    #[tokio::test]
    async fn orchestrator_parallel_independent_sessions() {
        let (_tmp, cli, repo_path) = setup_test_repo();
        let statuses: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let statuses_clone = statuses.clone();

        let manifest = make_manifest(
            "parallel-test",
            vec![
                scripted_session(
                    "session-a",
                    vec![commit_step("add a", vec![("a.txt", "a\n")])],
                ),
                scripted_session(
                    "session-b",
                    vec![commit_step("add b", vec![("b.txt", "b\n")])],
                ),
            ],
            None,
        );
        let manifest_content = toml::to_string(&manifest).unwrap();

        let orch = Orchestrator::new(cli, repo_path);
        let cancel = CancellationToken::new();
        let report = orch
            .run(
                &manifest,
                &manifest_content,
                &default_opts(),
                &NoopConflictHandler,
                cancel,
                move |name, state| {
                    let status = format!("{:?}", state);
                    statuses_clone
                        .lock()
                        .unwrap()
                        .push((name.to_string(), status));
                },
            )
            .await
            .expect("orchestrator should succeed");

        assert_eq!(report.outcome, RunPhase::Complete);
        assert!(report.session_results["session-a"].is_success());
        assert!(report.session_results["session-b"].is_success());
        assert!(report.merge_report.is_some());
    }

    #[tokio::test]
    async fn orchestrator_sequential_depends_on() {
        let (_tmp, cli, repo_path) = setup_test_repo();

        let manifest = make_manifest(
            "sequential-test",
            vec![
                scripted_session(
                    "base-session",
                    vec![commit_step("add base", vec![("base.txt", "base\n")])],
                ),
                scripted_session_with_deps(
                    "dependent-session",
                    vec!["base-session"],
                    vec![commit_step("add dep", vec![("dep.txt", "dep\n")])],
                ),
            ],
            None,
        );
        let manifest_content = toml::to_string(&manifest).unwrap();

        let orch = Orchestrator::new(cli, repo_path);
        let cancel = CancellationToken::new();
        let report = orch
            .run(
                &manifest,
                &manifest_content,
                &default_opts(),
                &NoopConflictHandler,
                cancel,
                |_, _| {},
            )
            .await
            .expect("orchestrator should succeed");

        assert_eq!(report.outcome, RunPhase::Complete);
        assert!(report.session_results["base-session"].is_success());
        assert!(report.session_results["dependent-session"].is_success());
    }

    #[tokio::test]
    async fn orchestrator_skip_dependents_on_failure() {
        let (_tmp, cli, repo_path) = setup_test_repo();

        let fail_session = failing_session("fails");
        let dep_session = scripted_session_with_deps(
            "depends-on-fail",
            vec!["fails"],
            vec![commit_step("add file", vec![("dep.txt", "dep\n")])],
        );

        // Independent session that should still run
        let independent = scripted_session(
            "independent",
            vec![commit_step("add ind", vec![("ind.txt", "ind\n")])],
        );

        let manifest = make_manifest(
            "skip-deps-test",
            vec![fail_session, dep_session, independent],
            Some(FailurePolicy::SkipDependents),
        );
        let manifest_content = toml::to_string(&manifest).unwrap();

        let orch = Orchestrator::new(cli, repo_path);
        let cancel = CancellationToken::new();
        let report = orch
            .run(
                &manifest,
                &manifest_content,
                &default_opts(),
                &NoopConflictHandler,
                cancel,
                |_, _| {},
            )
            .await
            .expect("orchestrator should succeed with skip-dependents");

        // The failing session should be Failed
        assert!(
            matches!(
                report.session_results["fails"],
                SessionRunState::Failed { .. }
            ),
            "failing session should be Failed, got: {:?}",
            report.session_results["fails"]
        );

        // The dependent should be Skipped
        assert!(
            matches!(
                report.session_results["depends-on-fail"],
                SessionRunState::Skipped { .. }
            ),
            "dependent session should be Skipped, got: {:?}",
            report.session_results["depends-on-fail"]
        );

        // The independent should complete
        assert!(
            report.session_results["independent"].is_success(),
            "independent session should succeed, got: {:?}",
            report.session_results["independent"]
        );
    }

    #[tokio::test]
    async fn orchestrator_abort_on_failure() {
        let (_tmp, cli, repo_path) = setup_test_repo();

        let manifest = make_manifest(
            "abort-test",
            vec![
                failing_session("will-fail"),
                scripted_session(
                    "should-not-run",
                    vec![commit_step("add file", vec![("x.txt", "x\n")])],
                ),
            ],
            Some(FailurePolicy::Abort),
        );
        let manifest_content = toml::to_string(&manifest).unwrap();

        let orch = Orchestrator::new(cli, repo_path);
        let cancel = CancellationToken::new();
        let result = orch
            .run(
                &manifest,
                &manifest_content,
                &default_opts(),
                &NoopConflictHandler,
                cancel,
                |_, _| {},
            )
            .await;

        assert!(result.is_err(), "abort policy should return error");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("aborting"),
            "error should mention aborting, got: {err}"
        );
    }

    #[tokio::test]
    async fn orchestrator_merge_after_sessions() {
        let (_tmp, cli, repo_path) = setup_test_repo();

        let manifest = make_manifest(
            "merge-test",
            vec![
                scripted_session(
                    "session-x",
                    vec![commit_step("add x", vec![("x.txt", "x content\n")])],
                ),
                scripted_session(
                    "session-y",
                    vec![commit_step("add y", vec![("y.txt", "y content\n")])],
                ),
            ],
            None,
        );
        let manifest_content = toml::to_string(&manifest).unwrap();

        let orch = Orchestrator::new(cli.clone(), repo_path);
        let cancel = CancellationToken::new();
        let report = orch
            .run(
                &manifest,
                &manifest_content,
                &default_opts(),
                &NoopConflictHandler,
                cancel,
                |_, _| {},
            )
            .await
            .expect("orchestrator should succeed");

        assert_eq!(report.outcome, RunPhase::Complete);
        assert!(report.merge_report.is_some());

        let merge_report = report.merge_report.unwrap();
        assert_eq!(merge_report.sessions_merged.len(), 2);

        // Verify merge target branch exists
        assert!(
            cli.branch_exists("smelt/merge/merge-test")
                .await
                .expect("branch_exists")
        );
    }

    #[tokio::test]
    async fn orchestrator_cancellation() {
        let (_tmp, cli, repo_path) = setup_test_repo();

        let manifest = make_manifest(
            "cancel-test",
            vec![scripted_session(
                "will-cancel",
                vec![commit_step("add file", vec![("c.txt", "c\n")])],
            )],
            None,
        );
        let manifest_content = toml::to_string(&manifest).unwrap();

        let cancel = CancellationToken::new();

        // Cancel immediately before run
        cancel.cancel();

        let orch = Orchestrator::new(cli, repo_path);
        let report = orch
            .run(
                &manifest,
                &manifest_content,
                &default_opts(),
                &NoopConflictHandler,
                cancel,
                |_, _| {},
            )
            .await
            .expect("cancellation should return report, not error");

        // All sessions should be cancelled
        assert!(
            matches!(
                report.session_results.get("will-cancel"),
                Some(SessionRunState::Cancelled)
            ),
            "session should be cancelled, got: {:?}",
            report.session_results.get("will-cancel")
        );
        assert!(report.merge_report.is_none());
    }
}
