//! Git operations trait and preflight checks.

mod cli;

use std::path::{Path, PathBuf};

pub use cli::GitCli;

use crate::error::{Result, SmeltError};
use crate::worktree::GitWorktreeEntry;

/// Async interface for git operations.
///
/// Implementations shell out to `git` or use a library. The trait is the
/// primary test seam — production code uses [`GitCli`], tests can substitute
/// a fake.
pub trait GitOps {
    /// Return the repository root directory.
    fn repo_root(&self) -> impl Future<Output = Result<PathBuf>> + Send;

    /// Check whether `path` is inside a git work tree.
    fn is_inside_work_tree(&self, path: &Path) -> impl Future<Output = Result<bool>> + Send;

    /// Return the current branch name (e.g. `main`).
    fn current_branch(&self) -> impl Future<Output = Result<String>> + Send;

    /// Return the abbreviated HEAD commit hash.
    fn head_short(&self) -> impl Future<Output = Result<String>> + Send;

    /// Create a new worktree at `path` on branch `branch_name`, based on `start_point`.
    fn worktree_add(
        &self,
        path: &Path,
        branch_name: &str,
        start_point: &str,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Remove a worktree. If `force` is true, removes even with uncommitted changes.
    fn worktree_remove(
        &self,
        path: &Path,
        force: bool,
    ) -> impl Future<Output = Result<()>> + Send;

    /// List worktrees in porcelain format.
    fn worktree_list(&self) -> impl Future<Output = Result<Vec<GitWorktreeEntry>>> + Send;

    /// Prune stale worktree metadata.
    fn worktree_prune(&self) -> impl Future<Output = Result<()>> + Send;

    /// Check if a worktree path has uncommitted changes.
    fn worktree_is_dirty(&self, path: &Path) -> impl Future<Output = Result<bool>> + Send;

    /// Delete a branch. `force` = true uses `-D` (ignores merge status).
    fn branch_delete(
        &self,
        branch_name: &str,
        force: bool,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Check if a branch is merged into `base_ref`.
    fn branch_is_merged(
        &self,
        branch_name: &str,
        base_ref: &str,
    ) -> impl Future<Output = Result<bool>> + Send;

    /// Check if a branch exists.
    fn branch_exists(&self, branch_name: &str) -> impl Future<Output = Result<bool>> + Send;
}

/// Synchronous preflight checks run before the async runtime is fully engaged.
///
/// Discovers the `git` binary on `$PATH` and verifies the current directory is
/// inside a git repository.
///
/// Returns `(git_binary, repo_root)` on success.
pub fn preflight() -> Result<(PathBuf, PathBuf)> {
    let git_binary = which::which("git").map_err(|_| SmeltError::GitNotFound)?;

    let output = std::process::Command::new(&git_binary)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| SmeltError::io("running git rev-parse --show-toplevel", &git_binary, e))?;

    if !output.status.success() {
        return Err(SmeltError::NotAGitRepo);
    }

    let repo_root = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());

    Ok((git_binary, repo_root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preflight_succeeds_in_git_repo() {
        // This test runs inside the smelt repo itself, so preflight should succeed.
        let (git_binary, repo_root) = preflight().expect("preflight should succeed in a git repo");
        assert!(git_binary.exists(), "git binary should exist on disk");
        assert!(repo_root.is_dir(), "repo root should be a directory");
    }
}
