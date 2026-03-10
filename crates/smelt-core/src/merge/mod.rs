//! Sequential merge engine for combining completed session worktrees.

pub mod conflict;
pub mod ordering;
pub mod types;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

pub use conflict::{ConflictHunk, ConflictScan, scan_conflict_markers};
pub use types::{
    ConflictAction, DiffStat, MergeOpts, MergeOrderStrategy, MergePlan, MergeReport,
    MergeSessionResult, PairwiseOverlap, ResolutionMethod, SessionPlanEntry,
};

use crate::error::{Result, SmeltError};
use crate::git::GitOps;
use crate::session::manifest::Manifest;
use crate::worktree::state::{SessionStatus, WorktreeState};
use crate::worktree::WorktreeManager;

/// A completed session ready for merging.
pub(crate) struct CompletedSession {
    pub(crate) session_name: String,
    pub(crate) branch_name: String,
    pub(crate) task_description: Option<String>,
    pub(crate) changed_files: HashSet<String>,
    pub(crate) original_index: usize,
}

/// Orchestrates the full merge sequence: reads session states, creates a target
/// branch, squash-merges each completed session sequentially in a temporary
/// worktree, rolls back on conflict, and cleans up on success.
pub struct MergeRunner<G: GitOps + Clone> {
    git: G,
    repo_root: PathBuf,
}

impl<G: GitOps + Clone> MergeRunner<G> {
    /// Create a new `MergeRunner`.
    pub fn new(git: G, repo_root: PathBuf) -> Self {
        Self { git, repo_root }
    }

    /// Resolve effective strategy: CLI flag > manifest field > default.
    fn resolve_strategy(opts: &MergeOpts, manifest: &Manifest) -> MergeOrderStrategy {
        opts.strategy
            .or(manifest.manifest.merge_strategy)
            .unwrap_or_default()
    }

    /// Compute a merge plan without executing the merge.
    ///
    /// Validates state, collects sessions, resolves strategy, and computes ordering.
    /// Does NOT create branches, worktrees, or perform any merges.
    pub async fn plan(&self, manifest: &Manifest, opts: MergeOpts) -> Result<MergePlan> {
        let smelt_dir = self.repo_root.join(".smelt");

        if !smelt_dir.exists() {
            return Err(SmeltError::NotInitialized);
        }

        let (completed, _skipped) = self.collect_sessions(manifest, &smelt_dir).await?;

        let strategy = Self::resolve_strategy(&opts, manifest);

        let (_ordered, merge_plan) = ordering::order_sessions(completed, strategy);

        Ok(merge_plan)
    }

    /// Run the full merge pipeline for a manifest.
    pub async fn run(&self, manifest: &Manifest, opts: MergeOpts) -> Result<MergeReport> {
        let smelt_dir = self.repo_root.join(".smelt");

        // Phase A: Preparation
        // 1. Validate .smelt/ exists
        if !smelt_dir.exists() {
            return Err(SmeltError::NotInitialized);
        }

        // 2-6. Read session states and filter, populate changed files
        let (completed, skipped) = self.collect_sessions(manifest, &smelt_dir).await?;

        // Resolve effective strategy: CLI > manifest > default
        let strategy = Self::resolve_strategy(&opts, manifest);

        // Order sessions according to strategy
        let (ordered, merge_plan) = ordering::order_sessions(completed, strategy);
        info!(
            "Merge order strategy: {}{}",
            strategy,
            if merge_plan.fell_back {
                " (fell back to completion-time)"
            } else {
                ""
            }
        );

        // Phase B: Target branch setup
        // 7. Determine target branch name
        let target_branch = opts
            .target_branch
            .unwrap_or_else(|| format!("smelt/merge/{}", manifest.manifest.name));

        // 8. Check target branch doesn't exist
        if self.git.branch_exists(&target_branch).await? {
            return Err(SmeltError::MergeTargetExists {
                branch: target_branch,
            });
        }

        // 9. Determine base commit
        let base_commit = self.git.rev_parse(&manifest.manifest.base_ref).await?;

        // 10. Create target branch
        self.git.branch_create(&target_branch, &base_commit).await?;

        // Phase C: Temp worktree
        // 11. Determine temp worktree path
        let repo_name = self
            .repo_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "repo".to_string());

        let temp_path = self
            .repo_root
            .parent()
            .ok_or_else(|| SmeltError::GitExecution {
                operation: "merge".to_string(),
                message: format!(
                    "repository root '{}' has no parent directory",
                    self.repo_root.display()
                ),
            })?
            .join(format!("{repo_name}-smelt-merge-{}", manifest.manifest.name));

        // 12. Create temp worktree
        self.git
            .worktree_add_existing(&temp_path, &target_branch)
            .await?;

        // Phase D + E: Sequential merge loop + cleanup
        let result = self
            .merge_sessions(&ordered, &temp_path, &target_branch)
            .await;

        match result {
            Ok(session_results) => {
                // Phase E: Cleanup — success path
                // 14. Remove temp worktree
                self.git.worktree_remove(&temp_path, true).await?;
                // 15. Prune worktree metadata
                self.git.worktree_prune().await?;

                // 16. Clean up session worktrees and branches
                let manager = WorktreeManager::new(self.git.clone(), self.repo_root.clone());
                for session in &ordered {
                    if let Err(e) = manager.remove(&session.session_name, true).await {
                        warn!(
                            "failed to clean up session '{}': {e}",
                            session.session_name
                        );
                    }
                }

                // 17. Build MergeReport
                let total_files_changed: usize =
                    session_results.iter().map(|r| r.files_changed).sum();
                let total_insertions: usize =
                    session_results.iter().map(|r| r.insertions).sum();
                let total_deletions: usize =
                    session_results.iter().map(|r| r.deletions).sum();

                Ok(MergeReport {
                    target_branch,
                    base_commit,
                    sessions_merged: session_results,
                    sessions_skipped: skipped,
                    total_files_changed,
                    total_insertions,
                    total_deletions,
                    plan: Some(merge_plan),
                    sessions_conflict_skipped: vec![],
                    sessions_resolved: vec![],
                })
            }
            Err(e) => {
                // Rollback: clean up temp worktree + target branch
                let _ = self.git.reset_hard(&temp_path, "HEAD").await;
                let _ = self.git.worktree_remove(&temp_path, true).await;
                // Filesystem fallback if worktree_remove failed to clean up
                if temp_path.exists() {
                    let _ = std::fs::remove_dir_all(&temp_path);
                }
                let _ = self.git.worktree_prune().await;
                let _ = self.git.branch_delete(&target_branch, true).await;
                Err(e)
            }
        }
    }

    /// Collect completed sessions from manifest, checking for running sessions.
    /// Populates `changed_files` for each session via `diff_name_only`.
    async fn collect_sessions(
        &self,
        manifest: &Manifest,
        smelt_dir: &Path,
    ) -> Result<(Vec<CompletedSession>, Vec<String>)> {
        let worktrees_dir = smelt_dir.join("worktrees");
        let mut completed = Vec::new();
        let mut skipped = Vec::new();

        for (idx, session_def) in manifest.sessions.iter().enumerate() {
            let state_file = worktrees_dir.join(format!("{}.toml", session_def.name));
            if !state_file.exists() {
                warn!(
                    "skipping session '{}': no state file found",
                    session_def.name
                );
                skipped.push(session_def.name.clone());
                continue;
            }

            let state = WorktreeState::load(&state_file)?;

            match state.status {
                SessionStatus::Created => {
                    return Err(SmeltError::SessionError {
                        session: session_def.name.clone(),
                        message: "session has not started yet — cannot merge before execution"
                            .to_string(),
                    });
                }
                SessionStatus::Running => {
                    return Err(SmeltError::SessionError {
                        session: session_def.name.clone(),
                        message: "session is still running — cannot merge while sessions are active"
                            .to_string(),
                    });
                }
                SessionStatus::Completed => {
                    // Determine the merge base for this session
                    let base_ref = session_def
                        .base_ref
                        .as_deref()
                        .unwrap_or(&manifest.manifest.base_ref);
                    let (changed_files, files_unavailable) = match self
                        .git
                        .diff_name_only(base_ref, &state.branch_name)
                        .await
                    {
                        Ok(files) => (files.into_iter().collect::<HashSet<String>>(), false),
                        Err(e) => {
                            warn!(
                                "failed to get changed files for session '{}': {e} \
                                 (file-overlap ordering may be inaccurate)",
                                session_def.name
                            );
                            (HashSet::new(), true)
                        }
                    };
                    if files_unavailable {
                        warn!(
                            "session '{}' has no file data — overlap scoring will treat it as zero-overlap",
                            session_def.name
                        );
                    }
                    completed.push(CompletedSession {
                        session_name: state.session_name.clone(),
                        branch_name: state.branch_name.clone(),
                        task_description: state.task_description.clone(),
                        changed_files,
                        original_index: idx,
                    });
                }
                ref status => {
                    warn!(
                        "skipping session '{}': status {:?}",
                        session_def.name, status
                    );
                    skipped.push(session_def.name.clone());
                }
            }
        }

        if completed.is_empty() {
            return Err(SmeltError::NoCompletedSessions);
        }

        Ok((completed, skipped))
    }

    /// Merge each completed session sequentially via squash merge.
    async fn merge_sessions(
        &self,
        sessions: &[CompletedSession],
        temp_path: &Path,
        target_branch: &str,
    ) -> Result<Vec<MergeSessionResult>> {
        let total = sessions.len();
        let mut results = Vec::with_capacity(total);

        for (i, session) in sessions.iter().enumerate() {
            info!(
                "[{}/{}] Merging session '{}'...",
                i + 1,
                total,
                session.session_name
            );

            // Squash merge
            self.git
                .merge_squash(temp_path, &session.branch_name)
                .await
                .map_err(|e| match e {
                    SmeltError::MergeConflict { files, .. } => SmeltError::MergeConflict {
                        session: session.session_name.clone(),
                        files,
                    },
                    other => other,
                })?;

            // Commit
            let commit_msg =
                format_commit_message(&session.session_name, session.task_description.as_deref(), target_branch, &session.branch_name);
            let commit_hash = self.git.commit(temp_path, &commit_msg).await?;

            // Diff stats — resolve to full hash to avoid short-hash ambiguity
            let full_hash = self.git.rev_parse(&commit_hash).await?;
            let numstat = match self.git.diff_numstat(&format!("{full_hash}^"), &full_hash).await {
                Ok(stats) => stats,
                Err(e) => {
                    warn!(
                        "failed to get diff stats for session '{}': {e}",
                        session.session_name
                    );
                    Vec::new()
                }
            };

            let diff_stats: Vec<DiffStat> = numstat
                .iter()
                .map(|(ins, del, file)| DiffStat {
                    file: file.clone(),
                    insertions: *ins,
                    deletions: *del,
                })
                .collect();

            let files_changed = diff_stats.len();
            let insertions: usize = diff_stats.iter().map(|d| d.insertions).sum();
            let deletions: usize = diff_stats.iter().map(|d| d.deletions).sum();

            results.push(MergeSessionResult {
                session_name: session.session_name.clone(),
                commit_hash,
                diff_stats,
                files_changed,
                insertions,
                deletions,
                resolution: Some(ResolutionMethod::Clean),
            });
        }

        Ok(results)
    }
}

/// Format a template commit message for a squash merge.
fn format_commit_message(
    session_name: &str,
    task: Option<&str>,
    target_branch: &str,
    session_branch: &str,
) -> String {
    let subject = match task {
        Some(desc) => {
            let prefix = format!("merge({session_name}): ");
            let max_desc = 72_usize.saturating_sub(prefix.len());
            if max_desc <= 3 || desc.len() <= max_desc {
                // Prefix alone exceeds/meets limit, or desc fits — use full desc
                format!("{prefix}{desc}")
            } else {
                let boundary = desc.floor_char_boundary(max_desc - 3);
                format!("{prefix}{}...", &desc[..boundary])
            }
        }
        None => format!("merge({session_name}): squash merge into {target_branch}"),
    };

    format!(
        "{subject}\n\nSession: {session_name}\nTask: {}\nBranch: {session_branch}",
        task.unwrap_or("(none)")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitCli;
    use crate::worktree::{CreateWorktreeOpts, WorktreeManager};
    use std::process::Command;

    /// Create a temporary git repo with an initial commit, returning (temp_dir, GitCli, repo_path).
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

        let cli = GitCli::new(git, repo_path.clone());
        (tmp, cli, repo_path)
    }

    /// Initialize .smelt/ in the test repo.
    fn init_smelt(repo_path: &Path) {
        crate::init::init_project(repo_path).expect("init_project");
        std::fs::create_dir_all(repo_path.join(".smelt/worktrees")).expect("create worktrees dir");
    }

    /// Create a completed session: worktree + committed files + state file marked Completed.
    async fn create_test_session(
        git: &GitCli,
        repo_path: &Path,
        session_name: &str,
        files: &[(&str, &str)],
        task_description: Option<&str>,
    ) {
        let manager = WorktreeManager::new(git.clone(), repo_path.to_path_buf());

        let info = manager
            .create(CreateWorktreeOpts {
                session_name: session_name.to_string(),
                base: "HEAD".to_string(),
                dir_name: None,
                task_description: task_description.map(|s| s.to_string()),
                file_scope: None,
            })
            .await
            .expect("create worktree");

        // Write files, add, commit in the worktree
        let file_paths: Vec<&str> = files
            .iter()
            .map(|(path, content)| {
                let full = info.worktree_path.join(path);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(&full, content).unwrap();
                *path
            })
            .collect();

        git.add(&info.worktree_path, &file_paths).await.expect("git add");
        git.commit(&info.worktree_path, &format!("session {session_name} changes"))
            .await
            .expect("git commit");

        // Update state file to Completed
        let state_file = repo_path
            .join(".smelt/worktrees")
            .join(format!("{session_name}.toml"));
        let mut state = WorktreeState::load(&state_file).expect("load state");
        state.status = SessionStatus::Completed;
        state.updated_at = chrono::Utc::now();
        state.save(&state_file).expect("save state");
    }

    /// Build a minimal manifest from session names.
    fn make_manifest(name: &str, sessions: &[(&str, Option<&str>)]) -> Manifest {
        use crate::session::manifest::{ManifestMeta, SessionDef};
        Manifest {
            manifest: ManifestMeta {
                name: name.to_string(),
                base_ref: "HEAD".to_string(),
                merge_strategy: None,
            },
            sessions: sessions
                .iter()
                .map(|(sname, task)| SessionDef {
                    name: sname.to_string(),
                    task: Some(task.unwrap_or("test task").to_string()),
                    task_file: None,
                    file_scope: None,
                    base_ref: None,
                    timeout_secs: None,
                    env: None,
                    script: None,
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn test_merge_two_clean_sessions() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        create_test_session(&git, &repo_path, "alpha", &[("a.txt", "alpha content\n")], Some("Add file a"))
            .await;
        create_test_session(&git, &repo_path, "beta", &[("b.txt", "beta content\n")], Some("Add file b"))
            .await;

        let manifest = make_manifest("two-clean", &[("alpha", Some("Add file a")), ("beta", Some("Add file b"))]);
        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let report = runner.run(&manifest, MergeOpts::default()).await.expect("merge should succeed");

        assert_eq!(report.target_branch, "smelt/merge/two-clean");
        assert_eq!(report.sessions_merged.len(), 2);
        assert_eq!(report.sessions_skipped.len(), 0);
        assert_eq!(report.sessions_merged[0].session_name, "alpha");
        assert_eq!(report.sessions_merged[1].session_name, "beta");
        assert!(report.total_files_changed >= 2);
        assert!(report.total_insertions >= 2);

        // Verify target branch exists
        assert!(git.branch_exists("smelt/merge/two-clean").await.unwrap());

        // Verify session worktrees were cleaned up (branches should be gone)
        assert!(!git.branch_exists("smelt/alpha").await.unwrap());
        assert!(!git.branch_exists("smelt/beta").await.unwrap());
    }

    #[tokio::test]
    async fn test_merge_conflict_rolls_back() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        // Both sessions modify README.md differently
        create_test_session(&git, &repo_path, "conflict-a", &[("README.md", "conflict-a content\n")], None)
            .await;
        create_test_session(&git, &repo_path, "conflict-b", &[("README.md", "conflict-b content\n")], None)
            .await;

        let manifest = make_manifest("conflict-test", &[("conflict-a", None), ("conflict-b", None)]);
        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let result = runner.run(&manifest, MergeOpts::default()).await;

        assert!(result.is_err(), "merge should fail with conflict");
        let err = result.unwrap_err();
        assert!(
            matches!(&err, SmeltError::MergeConflict { session, files }
                if session == "conflict-b" && files.contains(&"README.md".to_string())),
            "expected MergeConflict for conflict-b with README.md, got: {err:?}"
        );

        // Target branch should NOT exist (rolled back)
        assert!(!git.branch_exists("smelt/merge/conflict-test").await.unwrap());

        // Temp worktree should NOT exist
        let temp_path = repo_path.parent().unwrap().join("test-repo-smelt-merge-conflict-test");
        assert!(!temp_path.exists(), "temp worktree should be cleaned up");
    }

    #[tokio::test]
    async fn test_merge_skips_failed_sessions() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        create_test_session(&git, &repo_path, "good-1", &[("g1.txt", "good1\n")], None).await;
        create_test_session(&git, &repo_path, "good-2", &[("g2.txt", "good2\n")], None).await;

        // Create a third session but mark it as Failed
        create_test_session(&git, &repo_path, "bad-one", &[("bad.txt", "bad\n")], None).await;
        let state_file = repo_path.join(".smelt/worktrees/bad-one.toml");
        let mut state = WorktreeState::load(&state_file).unwrap();
        state.status = SessionStatus::Failed;
        state.save(&state_file).unwrap();

        let manifest = make_manifest("skip-test", &[("good-1", None), ("bad-one", None), ("good-2", None)]);
        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let report = runner.run(&manifest, MergeOpts::default()).await.expect("merge should succeed");

        assert_eq!(report.sessions_merged.len(), 2);
        assert_eq!(report.sessions_skipped.len(), 1);
        assert_eq!(report.sessions_skipped[0], "bad-one");
        assert_eq!(report.sessions_merged[0].session_name, "good-1");
        assert_eq!(report.sessions_merged[1].session_name, "good-2");
    }

    #[tokio::test]
    async fn test_merge_no_completed_sessions() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        // Create a session but mark it as Failed
        create_test_session(&git, &repo_path, "all-failed", &[("f.txt", "f\n")], None).await;
        let state_file = repo_path.join(".smelt/worktrees/all-failed.toml");
        let mut state = WorktreeState::load(&state_file).unwrap();
        state.status = SessionStatus::Failed;
        state.save(&state_file).unwrap();

        let manifest = make_manifest("no-complete", &[("all-failed", None)]);
        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let result = runner.run(&manifest, MergeOpts::default()).await;

        assert!(
            matches!(result, Err(SmeltError::NoCompletedSessions)),
            "expected NoCompletedSessions, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_merge_target_exists_error() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        create_test_session(&git, &repo_path, "exists-sess", &[("e.txt", "e\n")], None).await;

        // Pre-create the target branch
        git.branch_create("smelt/merge/target-exists", "HEAD").await.unwrap();

        let manifest = make_manifest("target-exists", &[("exists-sess", None)]);
        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let result = runner.run(&manifest, MergeOpts::default()).await;

        assert!(
            matches!(&result, Err(SmeltError::MergeTargetExists { branch }) if branch == "smelt/merge/target-exists"),
            "expected MergeTargetExists, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_merge_running_session_blocked() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        create_test_session(&git, &repo_path, "running-sess", &[("r.txt", "r\n")], None).await;
        // Overwrite status to Running
        let state_file = repo_path.join(".smelt/worktrees/running-sess.toml");
        let mut state = WorktreeState::load(&state_file).unwrap();
        state.status = SessionStatus::Running;
        state.save(&state_file).unwrap();

        let manifest = make_manifest("running-block", &[("running-sess", None)]);
        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let result = runner.run(&manifest, MergeOpts::default()).await;

        assert!(
            matches!(&result, Err(SmeltError::SessionError { session, message })
                if session == "running-sess" && message.contains("still running")),
            "expected SessionError about active sessions, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_merge_custom_target_branch() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        create_test_session(&git, &repo_path, "custom-sess", &[("c.txt", "c\n")], None).await;

        let manifest = make_manifest("custom-target", &[("custom-sess", None)]);
        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let report = runner
            .run(
                &manifest,
                MergeOpts::with_target_branch("my-custom-branch".to_string()),
            )
            .await
            .expect("merge should succeed");

        assert_eq!(report.target_branch, "my-custom-branch");
        assert!(git.branch_exists("my-custom-branch").await.unwrap());
    }

    #[tokio::test]
    async fn test_plan_returns_merge_plan() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        create_test_session(&git, &repo_path, "alpha", &[("a.txt", "alpha\n")], None).await;
        create_test_session(&git, &repo_path, "beta", &[("b.txt", "beta\n")], None).await;

        let manifest = make_manifest("plan-test", &[("alpha", None), ("beta", None)]);
        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let plan = runner
            .plan(&manifest, MergeOpts::default())
            .await
            .expect("plan should succeed");

        assert_eq!(plan.sessions.len(), 2);
        assert_eq!(plan.sessions[0].session_name, "alpha");
        assert_eq!(plan.sessions[1].session_name, "beta");
        assert_eq!(plan.strategy, MergeOrderStrategy::CompletionTime);
        assert!(!plan.fell_back);

        // Plan is read-only — no branches or worktrees should be created
        assert!(
            !git.branch_exists("smelt/merge/plan-test").await.unwrap(),
            "plan() should not create a target branch"
        );
    }

    #[tokio::test]
    async fn test_plan_file_overlap_strategy() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        // Session A: shared.rs, a_only.rs
        create_test_session(
            &git,
            &repo_path,
            "sess-a",
            &[("shared.rs", "shared-a\n"), ("a_only.rs", "a\n")],
            None,
        )
        .await;
        // Session B: shared.rs, b_only.rs
        create_test_session(
            &git,
            &repo_path,
            "sess-b",
            &[("shared.rs", "shared-b\n"), ("b_only.rs", "b\n")],
            None,
        )
        .await;
        // Session C: c_only.rs (no overlap)
        create_test_session(
            &git,
            &repo_path,
            "sess-c",
            &[("c_only.rs", "c\n")],
            None,
        )
        .await;

        let manifest = make_manifest(
            "overlap-plan",
            &[("sess-a", None), ("sess-b", None), ("sess-c", None)],
        );
        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let plan = runner
            .plan(
                &manifest,
                MergeOpts::with_strategy(MergeOrderStrategy::FileOverlap),
            )
            .await
            .expect("plan should succeed");

        assert_eq!(plan.strategy, MergeOrderStrategy::FileOverlap);
        assert!(!plan.fell_back);

        // A should be first (tiebreak by index), C before B (C has 0 overlap after A)
        assert_eq!(plan.sessions[0].session_name, "sess-a");
        assert_eq!(plan.sessions[1].session_name, "sess-c");
        assert_eq!(plan.sessions[2].session_name, "sess-b");

        // Check pairwise overlaps contain A-B overlap on shared.rs
        let ab_overlap = plan
            .pairwise_overlaps
            .iter()
            .find(|p| {
                (p.session_a == "sess-a" && p.session_b == "sess-b")
                    || (p.session_a == "sess-b" && p.session_b == "sess-a")
            })
            .expect("should have A-B overlap");
        assert!(ab_overlap.overlapping_files.contains(&"shared.rs".to_string()));
        assert!(ab_overlap.overlap_count() >= 1);
    }

    #[tokio::test]
    async fn test_plan_strategy_from_manifest() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        create_test_session(&git, &repo_path, "s1", &[("x.txt", "x\n")], None).await;
        create_test_session(&git, &repo_path, "s2", &[("y.txt", "y\n")], None).await;

        let mut manifest = make_manifest("manifest-strat", &[("s1", None), ("s2", None)]);
        manifest.manifest.merge_strategy = Some(MergeOrderStrategy::FileOverlap);

        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let plan = runner
            .plan(&manifest, MergeOpts::default())
            .await
            .expect("plan should succeed");

        assert_eq!(plan.strategy, MergeOrderStrategy::FileOverlap);
    }

    #[tokio::test]
    async fn test_plan_cli_overrides_manifest() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        create_test_session(&git, &repo_path, "s1", &[("x.txt", "x\n")], None).await;
        create_test_session(&git, &repo_path, "s2", &[("y.txt", "y\n")], None).await;

        let mut manifest = make_manifest("cli-override", &[("s1", None), ("s2", None)]);
        manifest.manifest.merge_strategy = Some(MergeOrderStrategy::CompletionTime);

        let runner = MergeRunner::new(git.clone(), repo_path.clone());
        let plan = runner
            .plan(
                &manifest,
                MergeOpts::with_strategy(MergeOrderStrategy::FileOverlap),
            )
            .await
            .expect("plan should succeed");

        // CLI strategy should override manifest
        assert_eq!(plan.strategy, MergeOrderStrategy::FileOverlap);
    }
}
