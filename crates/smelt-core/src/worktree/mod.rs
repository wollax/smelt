//! Worktree lifecycle management for agent sessions.

pub mod orphan;
pub mod state;

use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::error::{Result, SmeltError};
use crate::git::GitOps;

pub use state::{GitWorktreeEntry, SessionStatus, WorktreeState, parse_porcelain};

/// Result of a worktree removal operation.
#[derive(Debug, Clone)]
pub struct RemoveResult {
    /// Session name that was removed.
    pub session_name: String,
    /// Whether the git worktree was removed.
    pub worktree_removed: bool,
    /// Whether the branch was deleted.
    pub branch_deleted: bool,
    /// Whether the state file was removed.
    pub state_file_removed: bool,
}

/// Options for creating a new worktree.
#[derive(Debug, Clone)]
pub struct CreateWorktreeOpts {
    /// Session name — used for branch naming and state tracking.
    pub session_name: String,
    /// Base branch or commit to create the worktree from (defaults to "HEAD").
    pub base: String,
    /// Optional custom directory name override.
    pub dir_name: Option<String>,
    /// Optional task description for the session.
    pub task_description: Option<String>,
    /// Optional file scope for the session.
    pub file_scope: Option<Vec<String>>,
}

/// Information about a worktree returned from create/list operations.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub session_name: String,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    pub base_ref: String,
    pub status: SessionStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Manages worktree lifecycle: create, list, remove, prune.
///
/// Coordinates between git operations (via [`GitOps`]) and Smelt state
/// files in `.smelt/worktrees/`.
pub struct WorktreeManager<G: GitOps> {
    git: G,
    repo_root: PathBuf,
    smelt_dir: PathBuf,
}

impl<G: GitOps> WorktreeManager<G> {
    /// Create a new `WorktreeManager`.
    ///
    /// `repo_root` should be the absolute path to the repository root.
    pub fn new(git: G, repo_root: PathBuf) -> Self {
        let smelt_dir = repo_root.join(".smelt");
        Self {
            git,
            repo_root,
            smelt_dir,
        }
    }

    /// Create a new worktree for an agent session.
    ///
    /// Creates a git worktree with branch `smelt/<session_name>` in a sibling
    /// directory and writes a state file to `.smelt/worktrees/<session_name>.toml`.
    pub async fn create(&self, opts: CreateWorktreeOpts) -> Result<WorktreeInfo> {
        // 1. Check .smelt/ exists
        if !self.smelt_dir.exists() {
            return Err(SmeltError::NotInitialized);
        }

        let state_file = self
            .smelt_dir
            .join("worktrees")
            .join(format!("{}.toml", opts.session_name));

        // 2. Check state file doesn't already exist
        if state_file.exists() {
            return Err(SmeltError::WorktreeExists {
                name: opts.session_name.clone(),
            });
        }

        let branch_name = format!("smelt/{}", opts.session_name);

        // 3. Check branch doesn't already exist
        if self.git.branch_exists(&branch_name).await? {
            return Err(SmeltError::BranchExists {
                branch: branch_name,
            });
        }

        // 4. Compute worktree path
        let repo_name = self
            .repo_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "repo".to_string());

        let dir_name = opts.dir_name.clone().unwrap_or_else(|| {
            format!("{repo_name}-smelt-{}", opts.session_name)
        });

        let worktree_path = self
            .repo_root
            .parent()
            .ok_or_else(|| SmeltError::GitExecution {
                operation: "worktree create".to_string(),
                message: format!(
                    "repository root '{}' has no parent directory",
                    self.repo_root.display()
                ),
            })?
            .join(&dir_name);

        // 5. Check worktree path doesn't already exist on disk
        if worktree_path.exists() {
            return Err(SmeltError::WorktreeExists {
                name: opts.session_name.clone(),
            });
        }

        // 6. Create worktree
        self.git
            .worktree_add(&worktree_path, &branch_name, &opts.base)
            .await?;

        // 7. Write state file
        let now = Utc::now();
        let relative_path = PathBuf::from("..").join(&dir_name);

        let state = WorktreeState {
            session_name: opts.session_name.clone(),
            branch_name: branch_name.clone(),
            worktree_path: relative_path,
            base_ref: opts.base.clone(),
            status: SessionStatus::Created,
            created_at: now,
            updated_at: now,
            pid: None,
            exit_code: None,
            task_description: opts.task_description,
            file_scope: opts.file_scope,
        };

        state.save(&state_file)?;

        Ok(WorktreeInfo {
            session_name: opts.session_name,
            branch_name,
            worktree_path,
            base_ref: opts.base,
            status: SessionStatus::Created,
            created_at: now,
        })
    }

    /// List all tracked worktrees.
    ///
    /// Reads state files from `.smelt/worktrees/` and cross-references with
    /// `git worktree list` for consistency.
    pub async fn list(&self) -> Result<Vec<WorktreeInfo>> {
        let worktrees_dir = self.smelt_dir.join("worktrees");
        if !worktrees_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&worktrees_dir)
            .map_err(|e| SmeltError::io("reading worktrees directory", &worktrees_dir, e))?;

        let mut infos: Vec<WorktreeInfo> = Vec::new();

        // Get git worktree list for cross-reference
        let git_worktrees = self.git.worktree_list().await?;

        for entry in entries {
            let entry =
                entry.map_err(|e| SmeltError::io("reading directory entry", &worktrees_dir, e))?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }

            let state = WorktreeState::load(&path)?;

            // Resolve worktree_path relative to repo_root
            let resolved_path = self.repo_root.join(&state.worktree_path);

            // Cross-reference: check if git knows about this worktree
            let status = self.resolve_status(&state, &resolved_path, &git_worktrees);

            infos.push(WorktreeInfo {
                session_name: state.session_name,
                branch_name: state.branch_name,
                worktree_path: resolved_path,
                base_ref: state.base_ref,
                status,
                created_at: state.created_at,
            });
        }

        infos.sort_by(|a, b| a.session_name.cmp(&b.session_name));
        Ok(infos)
    }

    /// Resolve the effective status of a worktree by cross-referencing with git.
    fn resolve_status(
        &self,
        state: &WorktreeState,
        resolved_path: &Path,
        git_worktrees: &[GitWorktreeEntry],
    ) -> SessionStatus {
        // If the path doesn't exist on disk or git doesn't know about it,
        // the worktree might be orphaned. For now, just return the stored status.
        let _git_knows = git_worktrees.iter().any(|entry| {
            // Compare canonicalized paths if possible, fall back to direct comparison
            match (entry.path.canonicalize(), resolved_path.canonicalize()) {
                (Ok(a), Ok(b)) => a == b,
                _ => entry.path == resolved_path,
            }
        });

        // Future: detect orphaned state by checking git presence + PID liveness.
        // For now, return the stored status.
        state.status.clone()
    }

    /// Remove a worktree, its branch, and its state file.
    ///
    /// If `force` is false:
    /// - Returns `WorktreeDirty` if the worktree has uncommitted changes
    /// - Returns `BranchUnmerged` if the branch has unmerged commits
    ///
    /// If `force` is true, removes regardless of dirty/unmerged status.
    pub async fn remove(&self, name: &str, force: bool) -> Result<RemoveResult> {
        // 1. Load state file
        let state_file = self
            .smelt_dir
            .join("worktrees")
            .join(format!("{name}.toml"));

        if !state_file.exists() {
            return Err(SmeltError::WorktreeNotFound {
                name: name.to_string(),
            });
        }

        let state = WorktreeState::load(&state_file)?;

        // 2. Resolve worktree_path to absolute
        let abs_path = self.repo_root.join(&state.worktree_path);

        let mut result = RemoveResult {
            session_name: name.to_string(),
            worktree_removed: false,
            branch_deleted: false,
            state_file_removed: false,
        };

        // 3. Pre-flight checks (fail early before any destructive action)
        if !force {
            // Check dirty status
            if abs_path.exists() {
                let dirty = self.git.worktree_is_dirty(&abs_path).await?;
                if dirty {
                    return Err(SmeltError::WorktreeDirty {
                        name: name.to_string(),
                    });
                }
            }

            // Check branch merge status
            if self.git.branch_exists(&state.branch_name).await? {
                let is_merged = self
                    .git
                    .branch_is_merged(&state.branch_name, "HEAD")
                    .await?;
                if !is_merged {
                    return Err(SmeltError::BranchUnmerged {
                        branch: state.branch_name.clone(),
                    });
                }
            }
        }

        // 4. Remove worktree (safe — pre-flight passed)
        if abs_path.exists() {
            self.git.worktree_remove(&abs_path, force).await?;
            result.worktree_removed = true;
        }

        // 5. Delete branch
        if self.git.branch_exists(&state.branch_name).await? {
            self.git.branch_delete(&state.branch_name, force).await?;
            result.branch_deleted = true;
        }

        // 6. Remove state file
        std::fs::remove_file(&state_file)
            .map_err(|e| SmeltError::io("removing state file", &state_file, e))?;
        result.state_file_removed = true;

        // 7. Prune stale git worktree metadata
        self.git.worktree_prune().await?;

        Ok(result)
    }

    /// Detect orphaned worktree sessions.
    ///
    /// Cross-references state files with git worktree list and PID liveness
    /// to identify sessions that are likely abandoned.
    pub async fn detect_orphans(&self) -> Result<Vec<(String, WorktreeState)>> {
        let worktrees_dir = self.smelt_dir.join("worktrees");
        if !worktrees_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&worktrees_dir)
            .map_err(|e| SmeltError::io("reading worktrees directory", &worktrees_dir, e))?;

        let git_worktrees = self.git.worktree_list().await?;
        let threshold = chrono::Duration::hours(orphan::DEFAULT_STALENESS_HOURS);

        let mut orphans = Vec::new();

        for entry in entries {
            let entry =
                entry.map_err(|e| SmeltError::io("reading directory entry", &worktrees_dir, e))?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }

            let state = WorktreeState::load(&path)?;

            if orphan::is_likely_orphan(&state, &git_worktrees, threshold, &self.repo_root) {
                orphans.push((state.session_name.clone(), state));
            }
        }

        Ok(orphans)
    }

    /// Remove all orphaned worktrees.
    ///
    /// Detects orphans and removes each one with `force=true` (orphans are
    /// already dead sessions).
    pub async fn prune(&self) -> Result<Vec<String>> {
        let orphans = self.detect_orphans().await?;

        if orphans.is_empty() {
            return Ok(Vec::new());
        }

        let mut pruned = Vec::new();
        for (name, _state) in &orphans {
            // Orphans are removed with force since their owning process is gone
            self.remove(name, true).await?;
            pruned.push(name.clone());
        }

        Ok(pruned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitCli;
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
        // Create worktrees subdirectory
        std::fs::create_dir_all(repo_path.join(".smelt/worktrees")).expect("create worktrees dir");
    }

    #[tokio::test]
    async fn create_writes_state_and_calls_git() {
        let (tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        let manager = WorktreeManager::new(git, repo_path.clone());
        let info = manager
            .create(CreateWorktreeOpts {
                session_name: "test-session".to_string(),
                base: "HEAD".to_string(),
                dir_name: None,
                task_description: Some("A test task".to_string()),
                file_scope: None,
            })
            .await
            .expect("create should succeed");

        assert_eq!(info.session_name, "test-session");
        assert_eq!(info.branch_name, "smelt/test-session");
        assert_eq!(info.status, SessionStatus::Created);
        assert!(info.worktree_path.exists(), "worktree directory should exist");

        // State file should exist
        let state_file = repo_path.join(".smelt/worktrees/test-session.toml");
        assert!(state_file.exists(), "state file should exist");

        let state = WorktreeState::load(&state_file).expect("load state");
        assert_eq!(state.session_name, "test-session");
        assert_eq!(state.branch_name, "smelt/test-session");
        assert_eq!(
            state.worktree_path,
            PathBuf::from("../test-repo-smelt-test-session")
        );
        assert_eq!(state.task_description.as_deref(), Some("A test task"));

        // Cleanup sibling worktree directory
        let _ = std::fs::remove_dir_all(&info.worktree_path);
        drop(tmp);
    }

    #[tokio::test]
    async fn create_duplicate_name_returns_worktree_exists() {
        let (tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        let manager = WorktreeManager::new(git, repo_path.clone());

        let info = manager
            .create(CreateWorktreeOpts {
                session_name: "dup-session".to_string(),
                base: "HEAD".to_string(),
                dir_name: None,
                task_description: None,
                file_scope: None,
            })
            .await
            .expect("first create should succeed");

        let err = manager
            .create(CreateWorktreeOpts {
                session_name: "dup-session".to_string(),
                base: "HEAD".to_string(),
                dir_name: None,
                task_description: None,
                file_scope: None,
            })
            .await
            .expect_err("second create should fail");

        assert!(
            matches!(err, SmeltError::WorktreeExists { .. }),
            "expected WorktreeExists, got: {err}",
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&info.worktree_path);
        drop(tmp);
    }

    #[tokio::test]
    async fn create_not_initialized_returns_error() {
        let (tmp, git, repo_path) = setup_test_repo();
        // Don't call init_smelt — .smelt/ doesn't exist

        let manager = WorktreeManager::new(git, repo_path.clone());

        let err = manager
            .create(CreateWorktreeOpts {
                session_name: "no-init".to_string(),
                base: "HEAD".to_string(),
                dir_name: None,
                task_description: None,
                file_scope: None,
            })
            .await
            .expect_err("create without init should fail");

        assert!(
            matches!(err, SmeltError::NotInitialized),
            "expected NotInitialized, got: {err}",
        );

        drop(tmp);
    }

    #[tokio::test]
    async fn list_returns_created_worktrees() {
        let (tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        let manager = WorktreeManager::new(git, repo_path.clone());

        // Create two worktrees
        let info1 = manager
            .create(CreateWorktreeOpts {
                session_name: "alpha".to_string(),
                base: "HEAD".to_string(),
                dir_name: None,
                task_description: None,
                file_scope: None,
            })
            .await
            .expect("create alpha");

        let info2 = manager
            .create(CreateWorktreeOpts {
                session_name: "beta".to_string(),
                base: "HEAD".to_string(),
                dir_name: None,
                task_description: None,
                file_scope: None,
            })
            .await
            .expect("create beta");

        let list = manager.list().await.expect("list");
        assert_eq!(list.len(), 2);
        // Sorted by session_name
        assert_eq!(list[0].session_name, "alpha");
        assert_eq!(list[1].session_name, "beta");
        assert_eq!(list[0].branch_name, "smelt/alpha");
        assert_eq!(list[1].branch_name, "smelt/beta");

        // Cleanup
        let _ = std::fs::remove_dir_all(&info1.worktree_path);
        let _ = std::fs::remove_dir_all(&info2.worktree_path);
        drop(tmp);
    }

    #[tokio::test]
    async fn list_empty_returns_empty_vec() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        let manager = WorktreeManager::new(git, repo_path);
        let list = manager.list().await.expect("list");
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn remove_cleans_up_worktree_branch_and_state() {
        let (tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        let manager = WorktreeManager::new(git, repo_path.clone());
        let info = manager
            .create(CreateWorktreeOpts {
                session_name: "remove-me".to_string(),
                base: "HEAD".to_string(),
                dir_name: None,
                task_description: None,
                file_scope: None,
            })
            .await
            .expect("create should succeed");

        assert!(info.worktree_path.exists());

        // Remove with force (branch won't be merged)
        let result = manager
            .remove("remove-me", true)
            .await
            .expect("remove should succeed");

        assert!(result.worktree_removed);
        assert!(result.branch_deleted);
        assert!(result.state_file_removed);

        // Verify cleanup
        assert!(!info.worktree_path.exists(), "worktree dir should be gone");
        let state_file = repo_path.join(".smelt/worktrees/remove-me.toml");
        assert!(!state_file.exists(), "state file should be gone");

        // List should be empty
        let list = manager.list().await.expect("list");
        assert!(list.is_empty());

        drop(tmp);
    }

    #[tokio::test]
    async fn remove_dirty_worktree_without_force_returns_error() {
        let (tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        let manager = WorktreeManager::new(git, repo_path.clone());
        let info = manager
            .create(CreateWorktreeOpts {
                session_name: "dirty-wt".to_string(),
                base: "HEAD".to_string(),
                dir_name: None,
                task_description: None,
                file_scope: None,
            })
            .await
            .expect("create should succeed");

        // Make the worktree dirty
        std::fs::write(info.worktree_path.join("dirty.txt"), "dirty\n").unwrap();

        let err = manager
            .remove("dirty-wt", false)
            .await
            .expect_err("remove should fail on dirty worktree");

        assert!(
            matches!(err, SmeltError::WorktreeDirty { .. }),
            "expected WorktreeDirty, got: {err}",
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&info.worktree_path);
        drop(tmp);
    }

    #[tokio::test]
    async fn remove_dirty_worktree_with_force_succeeds() {
        let (tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        let manager = WorktreeManager::new(git, repo_path.clone());
        let info = manager
            .create(CreateWorktreeOpts {
                session_name: "force-dirty".to_string(),
                base: "HEAD".to_string(),
                dir_name: None,
                task_description: None,
                file_scope: None,
            })
            .await
            .expect("create should succeed");

        // Make the worktree dirty
        std::fs::write(info.worktree_path.join("dirty.txt"), "dirty\n").unwrap();

        let result = manager
            .remove("force-dirty", true)
            .await
            .expect("force remove should succeed");

        assert!(result.worktree_removed);
        assert!(result.branch_deleted);
        assert!(result.state_file_removed);

        drop(tmp);
    }

    #[tokio::test]
    async fn remove_nonexistent_returns_not_found() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        let manager = WorktreeManager::new(git, repo_path);
        let err = manager
            .remove("does-not-exist", false)
            .await
            .expect_err("remove nonexistent should fail");

        assert!(
            matches!(err, SmeltError::WorktreeNotFound { .. }),
            "expected WorktreeNotFound, got: {err}",
        );
    }

    #[tokio::test]
    async fn detect_orphans_finds_running_session_with_dead_pid() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        // Manually create a state file for a "running" session with a dead PID
        let state = WorktreeState {
            session_name: "dead-session".to_string(),
            branch_name: "smelt/dead-session".to_string(),
            worktree_path: PathBuf::from("../test-repo-smelt-dead-session"),
            base_ref: "HEAD".to_string(),
            status: SessionStatus::Running,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            pid: Some(4_000_000), // Very unlikely to be alive
            exit_code: None,
            task_description: None,
            file_scope: None,
        };

        let state_file = repo_path.join(".smelt/worktrees/dead-session.toml");
        state.save(&state_file).expect("save state");

        let manager = WorktreeManager::new(git, repo_path);
        let orphans = manager.detect_orphans().await.expect("detect_orphans");

        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].0, "dead-session");
    }

    #[tokio::test]
    async fn detect_orphans_ignores_created_sessions() {
        let (_tmp, git, repo_path) = setup_test_repo();
        init_smelt(&repo_path);

        // Created session should not be orphaned even without git entry
        let state = WorktreeState {
            session_name: "created-session".to_string(),
            branch_name: "smelt/created-session".to_string(),
            worktree_path: PathBuf::from("../test-repo-smelt-created-session"),
            base_ref: "HEAD".to_string(),
            status: SessionStatus::Created,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            pid: None,
            exit_code: None,
            task_description: None,
            file_scope: None,
        };

        let state_file = repo_path.join(".smelt/worktrees/created-session.toml");
        state.save(&state_file).expect("save state");

        let manager = WorktreeManager::new(git, repo_path);
        let orphans = manager.detect_orphans().await.expect("detect_orphans");

        assert!(orphans.is_empty());
    }
}
