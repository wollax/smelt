//! Git operations trait and preflight checks.

mod cli;

use std::path::{Path, PathBuf};

pub use cli::GitCli;

use crate::error::{Result, SmeltError};
use crate::worktree::GitWorktreeEntry;

/// Async interface for git operations.
///
/// The current implementation shells out to `git`. The trait exists as a
/// test seam — production code uses [`GitCli`], tests can substitute a fake.
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

    /// Stage files for commit.
    ///
    /// # Panics
    /// Panics if `paths` is empty. Callers must provide explicit file paths.
    fn add(&self, work_dir: &Path, paths: &[&str]) -> impl Future<Output = Result<()>> + Send;

    /// Create a commit in the given working directory with the provided message.
    /// Returns the short commit hash.
    fn commit(&self, work_dir: &Path, message: &str)
        -> impl Future<Output = Result<String>> + Send;

    /// Count commits on `branch` that are not on `base`. Returns the count.
    fn rev_list_count(
        &self,
        branch: &str,
        base: &str,
    ) -> impl Future<Output = Result<usize>> + Send;

    /// Find the merge-base (common ancestor) of two refs.
    fn merge_base(&self, ref_a: &str, ref_b: &str) -> impl Future<Output = Result<String>> + Send;

    /// Create a new branch at `start_point` without checking it out.
    fn branch_create(
        &self,
        branch_name: &str,
        start_point: &str,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Perform a squash merge of `source_ref` into the current branch of `work_dir`.
    /// Returns `Ok(())` on clean merge (changes staged, not committed).
    /// Returns `SmeltError::MergeConflict` on conflict (with file list, session empty).
    /// Returns `SmeltError::GitExecution` on other git errors.
    fn merge_squash(
        &self,
        work_dir: &Path,
        source_ref: &str,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Check out an existing branch into a new worktree path (no `-b` flag).
    fn worktree_add_existing(
        &self,
        path: &Path,
        branch_name: &str,
    ) -> impl Future<Output = Result<()>> + Send;

    /// List unmerged (conflicting) files in `work_dir`.
    /// Uses `git diff --name-only --diff-filter=U`.
    fn unmerged_files(
        &self,
        work_dir: &Path,
    ) -> impl Future<Output = Result<Vec<String>>> + Send;

    /// Hard reset HEAD in `work_dir` to `target_ref`.
    fn reset_hard(
        &self,
        work_dir: &Path,
        target_ref: &str,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Resolve a ref (branch name, HEAD, etc.) to a full commit hash.
    fn rev_parse(&self, rev: &str) -> impl Future<Output = Result<String>> + Send;

    /// List changed file paths between two refs.
    /// Uses `git diff --name-only base_ref head_ref`.
    fn diff_name_only(
        &self,
        base_ref: &str,
        head_ref: &str,
    ) -> impl Future<Output = Result<Vec<String>>> + Send;

    /// List commit subjects in a range.
    /// Uses `git log --format="%s" <range>`.
    fn log_subjects(&self, range: &str) -> impl Future<Output = Result<Vec<String>>> + Send;

    /// Get diff stats between two refs. Returns Vec of (insertions, deletions, filename).
    fn diff_numstat(
        &self,
        from_ref: &str,
        to_ref: &str,
    ) -> impl Future<Output = Result<Vec<(usize, usize, String)>>> + Send;
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
