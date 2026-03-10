//! SessionRunner — coordinates manifest execution across worktrees.

use std::path::PathBuf;

use tracing::warn;

use crate::error::{Result, SmeltError};
use crate::git::GitOps;
use crate::session::manifest::Manifest;
use crate::session::script::ScriptExecutor;
use crate::session::types::{SessionOutcome, SessionResult};
use crate::worktree::state::{SessionStatus, WorktreeState};
use crate::worktree::{CreateWorktreeOpts, WorktreeManager};

/// Coordinates manifest execution: creates worktrees, runs scripts,
/// and collects results for all sessions.
pub struct SessionRunner<G: GitOps> {
    git: G,
    repo_root: PathBuf,
}

impl<G: GitOps + Clone> SessionRunner<G> {
    /// Create a new `SessionRunner`.
    pub fn new(git: G, repo_root: PathBuf) -> Self {
        Self { git, repo_root }
    }

    /// Execute all sessions defined in a manifest.
    ///
    /// For each session:
    /// 1. Creates a worktree via `WorktreeManager`
    /// 2. Runs the scripted session (if defined) via `ScriptExecutor`
    /// 3. Collects the `SessionResult`
    ///
    /// Sessions run sequentially. Worktrees are NOT cleaned up on failure
    /// (they persist for inspection).
    pub async fn run_manifest(&self, manifest: &Manifest) -> Result<Vec<SessionResult>> {
        // Validate .smelt/ exists
        let smelt_dir = self.repo_root.join(".smelt");
        if !smelt_dir.exists() {
            return Err(SmeltError::NotInitialized);
        }

        let manager = WorktreeManager::new(self.git.clone(), self.repo_root.clone());
        let mut results = Vec::with_capacity(manifest.sessions.len());

        for session in &manifest.sessions {
            // Determine effective base_ref: session overrides manifest
            let base_ref = session
                .base_ref
                .clone()
                .unwrap_or_else(|| manifest.manifest.base_ref.clone());

            // Create worktree
            let info = match manager
                .create(CreateWorktreeOpts {
                    session_name: session.name.clone(),
                    base: base_ref,
                    dir_name: None,
                    task_description: session.task.clone(),
                    file_scope: session.file_scope.clone(),
                })
                .await
            {
                Ok(info) => info,
                Err(e) => {
                    results.push(SessionResult {
                        session_name: session.name.clone(),
                        outcome: SessionOutcome::Failed,
                        steps_completed: 0,
                        failure_reason: Some(format!("worktree creation failed: {e}")),
                        has_commits: false,
                        duration: std::time::Duration::ZERO,
                    });
                    continue;
                }
            };

            // Execute script if present
            let result = match session.script {
                Some(ref script) => {
                    let executor = ScriptExecutor::new(&self.git, info.worktree_path);
                    match executor.execute(&session.name, script).await {
                        Ok(result) => result,
                        Err(e) => SessionResult {
                            session_name: session.name.clone(),
                            outcome: SessionOutcome::Failed,
                            steps_completed: 0,
                            failure_reason: Some(format!("script execution failed: {e}")),
                            has_commits: false,
                            duration: std::time::Duration::ZERO,
                        },
                    }
                }
                None => {
                    // No script — return a completed result with no commits
                    SessionResult {
                        session_name: session.name.clone(),
                        outcome: SessionOutcome::Completed,
                        steps_completed: 0,
                        failure_reason: None,
                        has_commits: false,
                        duration: std::time::Duration::ZERO,
                    }
                }
            };

            // Update worktree state file with session outcome
            let state_file = smelt_dir
                .join("worktrees")
                .join(format!("{}.toml", session.name));
            let result = if state_file.exists() {
                match WorktreeState::load(&state_file) {
                    Ok(mut state) => {
                        state.status = match result.outcome {
                            SessionOutcome::Completed => SessionStatus::Completed,
                            _ => SessionStatus::Failed,
                        };
                        state.updated_at = chrono::Utc::now();
                        match state.save(&state_file) {
                            Ok(()) => result,
                            Err(e) => {
                                warn!(
                                    "failed to update state file for session '{}': {e}",
                                    session.name
                                );
                                SessionResult {
                                    outcome: SessionOutcome::Failed,
                                    failure_reason: Some(format!(
                                        "session completed but state save failed: {e}"
                                    )),
                                    ..result
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            "failed to load state file for session '{}': {e}",
                            session.name
                        );
                        result
                    }
                }
            } else {
                result
            };

            results.push(result);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitCli;
    use crate::session::manifest::{
        FailureMode, FileChange, ManifestMeta, ScriptDef, ScriptStep, SessionDef,
    };
    use std::process::Command;

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

        // Initialize .smelt/
        crate::init::init_project(&repo_path).expect("init_project");
        std::fs::create_dir_all(repo_path.join(".smelt/worktrees"))
            .expect("create worktrees dir");

        let cli = GitCli::new(git, repo_path.clone());
        (tmp, cli, repo_path)
    }

    fn simple_manifest(sessions: Vec<SessionDef>) -> Manifest {
        Manifest {
            manifest: ManifestMeta {
                name: "test-manifest".to_string(),
                base_ref: "HEAD".to_string(),
                merge_strategy: None,
                parallel_by_default: true,
                on_failure: None,
            },
            sessions,
        }
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

    #[tokio::test]
    async fn run_manifest_two_sessions_create_commits() {
        let (_tmp, cli, repo_path) = setup_test_repo();
        let default_branch = cli.current_branch().await.expect("current_branch");

        let manifest = simple_manifest(vec![
            scripted_session(
                "session-a",
                vec![commit_step("add a.txt", vec![("a.txt", "content a\n")])],
            ),
            scripted_session(
                "session-b",
                vec![commit_step("add b.txt", vec![("b.txt", "content b\n")])],
            ),
        ]);

        let runner = SessionRunner::new(cli.clone(), repo_path);
        let results = runner.run_manifest(&manifest).await.expect("run_manifest");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].session_name, "session-a");
        assert_eq!(results[0].outcome, SessionOutcome::Completed);
        assert_eq!(results[0].steps_completed, 1);
        assert!(results[0].has_commits);

        assert_eq!(results[1].session_name, "session-b");
        assert_eq!(results[1].outcome, SessionOutcome::Completed);
        assert_eq!(results[1].steps_completed, 1);
        assert!(results[1].has_commits);

        // Verify each branch has 1 commit above base
        let count_a = cli
            .rev_list_count("smelt/session-a", &default_branch)
            .await
            .expect("count a");
        assert_eq!(count_a, 1);

        let count_b = cli
            .rev_list_count("smelt/session-b", &default_branch)
            .await
            .expect("count b");
        assert_eq!(count_b, 1);
    }

    #[tokio::test]
    async fn run_manifest_returns_correct_results() {
        let (_tmp, cli, repo_path) = setup_test_repo();

        let manifest = simple_manifest(vec![
            scripted_session(
                "alpha",
                vec![
                    commit_step("first", vec![("x.txt", "x\n")]),
                    commit_step("second", vec![("y.txt", "y\n")]),
                ],
            ),
            scripted_session(
                "beta",
                vec![commit_step("only", vec![("z.txt", "z\n")])],
            ),
        ]);

        let runner = SessionRunner::new(cli, repo_path);
        let results = runner.run_manifest(&manifest).await.expect("run_manifest");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].steps_completed, 2);
        assert_eq!(results[1].steps_completed, 1);
    }

    #[tokio::test]
    async fn two_sessions_same_file_create_separate_worktrees() {
        let (_tmp, cli, repo_path) = setup_test_repo();
        let default_branch = cli.current_branch().await.expect("current_branch");

        let manifest = simple_manifest(vec![
            scripted_session(
                "writer-1",
                vec![commit_step(
                    "write shared file",
                    vec![("shared.txt", "content from writer-1\n")],
                )],
            ),
            scripted_session(
                "writer-2",
                vec![commit_step(
                    "write shared file",
                    vec![("shared.txt", "content from writer-2\n")],
                )],
            ),
        ]);

        let runner = SessionRunner::new(cli.clone(), repo_path);
        let results = runner.run_manifest(&manifest).await.expect("run_manifest");

        assert_eq!(results.len(), 2);
        assert!(results[0].has_commits);
        assert!(results[1].has_commits);

        // Both branches exist with commits
        assert!(
            cli.branch_exists("smelt/writer-1")
                .await
                .expect("branch exists")
        );
        assert!(
            cli.branch_exists("smelt/writer-2")
                .await
                .expect("branch exists")
        );

        let count_1 = cli
            .rev_list_count("smelt/writer-1", &default_branch)
            .await
            .expect("count 1");
        let count_2 = cli
            .rev_list_count("smelt/writer-2", &default_branch)
            .await
            .expect("count 2");
        assert_eq!(count_1, 1);
        assert_eq!(count_2, 1);
    }

    #[tokio::test]
    async fn run_manifest_not_initialized_returns_error() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let repo_path = tmp.path().join("no-smelt-repo");
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

        // Do NOT init .smelt/
        let manifest = simple_manifest(vec![scripted_session(
            "will-fail",
            vec![commit_step("nope", vec![("a.txt", "a\n")])],
        )]);

        let runner = SessionRunner::new(cli, repo_path);
        let err = runner
            .run_manifest(&manifest)
            .await
            .expect_err("should fail without .smelt/");

        assert!(
            matches!(err, SmeltError::NotInitialized),
            "expected NotInitialized, got: {err}"
        );
    }

    #[tokio::test]
    async fn run_manifest_session_without_script_returns_completed() {
        let (_tmp, cli, repo_path) = setup_test_repo();

        let manifest = simple_manifest(vec![SessionDef {
            name: "no-script".to_string(),
            task: Some("A real agent task".to_string()),
            task_file: None,
            file_scope: None,
            base_ref: None,
            timeout_secs: None,
            env: None,
            depends_on: None,
            script: None,
        }]);

        let runner = SessionRunner::new(cli, repo_path);
        let results = runner.run_manifest(&manifest).await.expect("run_manifest");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_name, "no-script");
        assert_eq!(results[0].outcome, SessionOutcome::Completed);
        assert_eq!(results[0].steps_completed, 0);
        assert!(!results[0].has_commits);
    }

    #[tokio::test]
    async fn run_manifest_session_base_ref_override() {
        let (_tmp, cli, repo_path) = setup_test_repo();

        // Create a second commit so we can use a branch as base_ref
        let git = which::which("git").unwrap();
        std::fs::write(repo_path.join("extra.txt"), "extra\n").unwrap();
        Command::new(&git)
            .args(["add", "extra.txt"])
            .current_dir(&repo_path)
            .output()
            .expect("git add");
        Command::new(&git)
            .args(["commit", "-m", "second commit"])
            .current_dir(&repo_path)
            .output()
            .expect("git commit");

        // Use HEAD as the base_ref override (session overrides manifest's "HEAD")
        let manifest = simple_manifest(vec![SessionDef {
            name: "from-override".to_string(),
            task: Some("Work from override".to_string()),
            task_file: None,
            file_scope: None,
            base_ref: Some("HEAD".to_string()),
            timeout_secs: None,
            env: None,
            depends_on: None,
            script: Some(ScriptDef {
                backend: "scripted".to_string(),
                exit_after: None,
                simulate_failure: None,
                steps: vec![ScriptStep::Commit {
                    message: "from override".to_string(),
                    files: vec![FileChange {
                        path: "override.txt".to_string(),
                        content: Some("from override\n".to_string()),
                        content_file: None,
                    }],
                }],
            }),
        }]);

        let runner = SessionRunner::new(cli.clone(), repo_path);
        let results = runner.run_manifest(&manifest).await.expect("run_manifest");

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].outcome,
            SessionOutcome::Completed,
            "expected Completed but got {:?}: {:?}",
            results[0].outcome,
            results[0].failure_reason
        );
        assert!(results[0].has_commits);
    }

    #[tokio::test]
    async fn conflict_setup_two_sessions_same_file_different_content() {
        let (_tmp, cli, repo_path) = setup_test_repo();

        let manifest = simple_manifest(vec![
            scripted_session(
                "feature-a",
                vec![commit_step(
                    "implement feature A",
                    vec![(
                        "src/lib.rs",
                        "pub mod feature_a;\n\nfn main() {\n    feature_a::run();\n}\n",
                    )],
                )],
            ),
            scripted_session(
                "feature-b",
                vec![commit_step(
                    "implement feature B",
                    vec![(
                        "src/lib.rs",
                        "pub mod feature_b;\n\nfn main() {\n    feature_b::run();\n}\n",
                    )],
                )],
            ),
        ]);

        let runner = SessionRunner::new(cli.clone(), repo_path.clone());
        let results = runner.run_manifest(&manifest).await.expect("run_manifest");

        assert_eq!(results.len(), 2);
        assert!(results[0].has_commits);
        assert!(results[1].has_commits);

        // Verify both branches have the file with different content
        let wt_a = repo_path.parent().unwrap().join("test-repo-smelt-feature-a");
        let wt_b = repo_path.parent().unwrap().join("test-repo-smelt-feature-b");

        let content_a = std::fs::read_to_string(wt_a.join("src/lib.rs")).expect("read lib.rs from a");
        let content_b = std::fs::read_to_string(wt_b.join("src/lib.rs")).expect("read lib.rs from b");

        assert!(content_a.contains("feature_a"), "session A should have feature_a");
        assert!(content_b.contains("feature_b"), "session B should have feature_b");
        assert_ne!(content_a, content_b, "content should differ (conflict setup)");
    }

    #[tokio::test]
    async fn run_manifest_updates_state_files_correctly() {
        let (_tmp, cli, repo_path) = setup_test_repo();

        let manifest = simple_manifest(vec![
            scripted_session(
                "state-ok",
                vec![commit_step("add file", vec![("ok.txt", "ok\n")])],
            ),
            scripted_session(
                "state-fail",
                vec![commit_step("add file", vec![("fail.txt", "fail\n")])],
            ),
        ]);

        // Pre-set the second session's script to simulate failure
        let mut manifest_with_failure = manifest;
        if let Some(ref mut script) = manifest_with_failure.sessions[1].script {
            script.simulate_failure = Some(FailureMode::Crash);
        }

        let runner = SessionRunner::new(cli, repo_path.clone());
        let results = runner
            .run_manifest(&manifest_with_failure)
            .await
            .expect("run_manifest");

        // First session should be Completed
        assert_eq!(results[0].outcome, SessionOutcome::Completed);
        let state_ok = WorktreeState::load(
            &repo_path.join(".smelt/worktrees/state-ok.toml"),
        )
        .expect("load state-ok");
        assert_eq!(state_ok.status, SessionStatus::Completed);

        // Second session should be Failed
        assert_eq!(results[1].outcome, SessionOutcome::Failed);
        let state_fail = WorktreeState::load(
            &repo_path.join(".smelt/worktrees/state-fail.toml"),
        )
        .expect("load state-fail");
        assert_eq!(state_fail.status, SessionStatus::Failed);
    }
}
