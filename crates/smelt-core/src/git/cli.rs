//! [`GitCli`] — shell-out implementation of [`GitOps`].

use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::error::{Result, SmeltError};
use crate::git::GitOps;

/// Concrete [`GitOps`] implementation that shells out to the `git` binary.
pub struct GitCli {
    git_binary: PathBuf,
    repo_root: PathBuf,
}

impl GitCli {
    /// Create a new `GitCli` instance.
    ///
    /// Typically constructed from the values returned by [`super::preflight()`].
    pub fn new(git_binary: PathBuf, repo_root: PathBuf) -> Self {
        Self {
            git_binary,
            repo_root,
        }
    }

    /// Run a git command and return trimmed stdout on success.
    async fn run(&self, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.git_binary)
            .args(args)
            .current_dir(&self.repo_root)
            .output()
            .await
            .map_err(|e| {
                SmeltError::io(
                    format!("running git {}", args.first().unwrap_or(&"")),
                    &self.git_binary,
                    e,
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SmeltError::GitExecution {
                operation: args.join(" "),
                message: stderr.trim().to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl GitOps for GitCli {
    async fn repo_root(&self) -> Result<PathBuf> {
        Ok(self.repo_root.clone())
    }

    async fn is_inside_work_tree(&self, path: &Path) -> Result<bool> {
        let output = Command::new(&self.git_binary)
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(path)
            .output()
            .await
            .map_err(|e| SmeltError::io("running git rev-parse --is-inside-work-tree", path, e))?;

        Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true")
    }

    async fn current_branch(&self) -> Result<String> {
        self.run(&["rev-parse", "--abbrev-ref", "HEAD"]).await
    }

    async fn head_short(&self) -> Result<String> {
        self.run(&["rev-parse", "--short", "HEAD"]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temporary git repo with an initial commit, returning (temp_dir, GitCli).
    fn setup_test_repo() -> (tempfile::TempDir, GitCli) {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let git = which::which("git").expect("git on PATH");

        // git init
        let status = std::process::Command::new(&git)
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .expect("git init");
        assert!(status.status.success(), "git init failed");

        // Configure user for commits
        for args in [
            &["config", "user.email", "test@example.com"][..],
            &["config", "user.name", "Test"][..],
        ] {
            std::process::Command::new(&git)
                .args(args)
                .current_dir(tmp.path())
                .output()
                .expect("git config");
        }

        // Create initial commit
        std::fs::write(tmp.path().join("README.md"), "# test\n").unwrap();
        std::process::Command::new(&git)
            .args(["add", "README.md"])
            .current_dir(tmp.path())
            .output()
            .expect("git add");
        std::process::Command::new(&git)
            .args(["commit", "-m", "initial"])
            .current_dir(tmp.path())
            .output()
            .expect("git commit");

        let cli = GitCli::new(git, tmp.path().to_path_buf());
        (tmp, cli)
    }

    #[tokio::test]
    async fn test_repo_root() {
        let (tmp, cli) = setup_test_repo();
        let root = cli.repo_root().await.expect("repo_root");
        // Canonicalize both to handle macOS /private/var symlink
        assert_eq!(
            root.canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap(),
        );
    }

    #[tokio::test]
    async fn test_current_branch() {
        let (_tmp, cli) = setup_test_repo();
        let branch = cli.current_branch().await.expect("current_branch");
        // Default branch name may be "main" or "master" depending on git config
        assert!(
            branch == "main" || branch == "master",
            "expected main or master, got: {branch}",
        );
    }

    #[tokio::test]
    async fn test_head_short() {
        let (_tmp, cli) = setup_test_repo();
        let hash = cli.head_short().await.expect("head_short");
        // Short hash is typically 7-12 hex characters
        assert!(
            hash.len() >= 7 && hash.chars().all(|c| c.is_ascii_hexdigit()),
            "expected short hex hash, got: {hash}",
        );
    }

    #[tokio::test]
    async fn test_is_inside_work_tree() {
        let (tmp, cli) = setup_test_repo();
        assert!(cli.is_inside_work_tree(tmp.path()).await.expect("check"));
    }
}
