//! Git operations trait and preflight checks.

mod cli;

use std::path::{Path, PathBuf};

pub use cli::GitCli;

use crate::error::{Result, SmeltError};

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
