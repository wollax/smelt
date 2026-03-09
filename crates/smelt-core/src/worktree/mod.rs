//! Worktree lifecycle management for agent sessions.

pub mod state;

use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::error::{Result, SmeltError};
use crate::git::GitOps;

pub use state::{GitWorktreeEntry, SessionStatus, WorktreeState, parse_porcelain};

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
            .expect("repo_root should have a parent directory")
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
}
