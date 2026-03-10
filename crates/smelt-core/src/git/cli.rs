//! [`GitCli`] — shell-out implementation of [`GitOps`].

use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::error::{Result, SmeltError};
use crate::git::GitOps;
use crate::worktree::{GitWorktreeEntry, parse_porcelain};

/// Concrete [`GitOps`] implementation that shells out to the `git` binary.
#[derive(Clone)]
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

    /// Run a git command in a specific working directory (not necessarily `self.repo_root`).
    async fn run_in(&self, work_dir: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.git_binary)
            .args(args)
            .current_dir(work_dir)
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

    async fn worktree_add(
        &self,
        path: &Path,
        branch_name: &str,
        start_point: &str,
    ) -> Result<()> {
        let path_str = path.to_string_lossy();
        self.run(&["worktree", "add", "-b", branch_name, &path_str, start_point])
            .await?;
        Ok(())
    }

    async fn worktree_remove(&self, path: &Path, force: bool) -> Result<()> {
        let path_str = path.to_string_lossy();
        if force {
            self.run(&["worktree", "remove", "--force", &path_str])
                .await?;
        } else {
            self.run(&["worktree", "remove", &path_str]).await?;
        }
        Ok(())
    }

    async fn worktree_list(&self) -> Result<Vec<GitWorktreeEntry>> {
        let output = self.run(&["worktree", "list", "--porcelain"]).await?;
        Ok(parse_porcelain(&output))
    }

    async fn worktree_prune(&self) -> Result<()> {
        self.run(&["worktree", "prune"]).await?;
        Ok(())
    }

    async fn worktree_is_dirty(&self, path: &Path) -> Result<bool> {
        let path_str = path.to_string_lossy();
        let output = Command::new(&self.git_binary)
            .args(["-C", &path_str, "status", "--porcelain"])
            .output()
            .await
            .map_err(|e| {
                SmeltError::io("running git status --porcelain", path, e)
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SmeltError::GitExecution {
                operation: format!("-C {} status --porcelain", path_str),
                message: stderr.trim().to_string(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
    }

    async fn branch_delete(&self, branch_name: &str, force: bool) -> Result<()> {
        let flag = if force { "-D" } else { "-d" };
        self.run(&["branch", flag, branch_name]).await?;
        Ok(())
    }

    async fn branch_is_merged(
        &self,
        branch_name: &str,
        base_ref: &str,
    ) -> Result<bool> {
        let output = self.run(&["branch", "--merged", base_ref]).await?;
        Ok(output
            .lines()
            .any(|line| line.trim().trim_start_matches("* ") == branch_name))
    }

    async fn branch_exists(&self, branch_name: &str) -> Result<bool> {
        let ref_name = format!("refs/heads/{branch_name}");
        let output = Command::new(&self.git_binary)
            .args(["rev-parse", "--verify", &ref_name])
            .current_dir(&self.repo_root)
            .output()
            .await
            .map_err(|e| {
                SmeltError::io("running git rev-parse --verify", &self.git_binary, e)
            })?;

        Ok(output.status.success())
    }

    async fn add(&self, work_dir: &Path, paths: &[&str]) -> Result<()> {
        assert!(!paths.is_empty(), "add() requires explicit file paths");
        let mut args = vec!["add"];
        args.extend(paths);
        self.run_in(work_dir, &args).await?;
        Ok(())
    }

    async fn commit(&self, work_dir: &Path, message: &str) -> Result<String> {
        self.run_in(work_dir, &["commit", "-m", message]).await?;
        // Get the short hash of the commit we just created
        let hash = self.run_in(work_dir, &["rev-parse", "--short", "HEAD"]).await?;
        Ok(hash)
    }

    async fn rev_list_count(&self, branch: &str, base: &str) -> Result<usize> {
        let range = format!("{base}..{branch}");
        let output = self.run(&["rev-list", "--count", &range]).await?;
        output
            .parse::<usize>()
            .map_err(|e| SmeltError::GitExecution {
                operation: format!("rev-list --count {range}"),
                message: format!("failed to parse count: {e}"),
            })
    }

    async fn merge_base(&self, ref_a: &str, ref_b: &str) -> Result<String> {
        self.run(&["merge-base", ref_a, ref_b]).await
    }

    async fn branch_create(&self, branch_name: &str, start_point: &str) -> Result<()> {
        self.run(&["branch", branch_name, start_point]).await?;
        Ok(())
    }

    async fn merge_squash(
        &self,
        work_dir: &Path,
        source_ref: &str,
        session_name: &str,
    ) -> Result<()> {
        let output = Command::new(&self.git_binary)
            .args(["merge", "--squash", source_ref])
            .current_dir(work_dir)
            .output()
            .await
            .map_err(|e| SmeltError::io("running git merge --squash", work_dir, e))?;

        if output.status.success() {
            return Ok(());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // git merge --squash writes CONFLICT messages to stdout (not stderr)
        if stdout.contains("CONFLICT") || stderr.contains("CONFLICT") {
            let files = self.unmerged_files(work_dir).await?;
            return Err(SmeltError::MergeConflict {
                session: session_name.to_string(),
                files,
            });
        }

        let exit_code = output.status.code().unwrap_or(-1);
        let combined = if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        Err(SmeltError::GitExecution {
            operation: format!("merge --squash {source_ref}"),
            message: format!("exit code {exit_code}: {combined}"),
        })
    }

    async fn worktree_add_existing(&self, path: &Path, branch_name: &str) -> Result<()> {
        let path_str = path.to_string_lossy();
        self.run(&["worktree", "add", &path_str, branch_name])
            .await?;
        Ok(())
    }

    async fn unmerged_files(&self, work_dir: &Path) -> Result<Vec<String>> {
        let output = self
            .run_in(work_dir, &["diff", "--name-only", "--diff-filter=U"])
            .await?;
        if output.is_empty() {
            return Ok(Vec::new());
        }
        Ok(output.lines().map(|l| l.to_string()).collect())
    }

    async fn reset_hard(&self, work_dir: &Path, target_ref: &str) -> Result<()> {
        self.run_in(work_dir, &["reset", "--hard", target_ref])
            .await?;
        Ok(())
    }

    async fn rev_parse(&self, rev: &str) -> Result<String> {
        self.run(&["rev-parse", rev]).await
    }

    async fn diff_numstat(&self, from_ref: &str, to_ref: &str) -> Result<Vec<(usize, usize, String)>> {
        let output = self.run(&["diff", "--numstat", from_ref, to_ref]).await?;
        if output.is_empty() {
            return Ok(Vec::new());
        }
        Ok(output
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(3, '\t').collect();
                if parts.len() == 3 {
                    let ins = parts[0].parse().unwrap_or(0);
                    let del = parts[1].parse().unwrap_or(0);
                    Some((ins, del, parts[2].to_string()))
                } else {
                    None
                }
            })
            .collect())
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

    #[tokio::test]
    async fn test_worktree_add_and_list() {
        let (tmp, cli) = setup_test_repo();
        let wt_path = tmp.path().parent().unwrap().join("smelt-test-wt-add");

        cli.worktree_add(&wt_path, "test-branch", "HEAD")
            .await
            .expect("worktree_add");

        let entries = cli.worktree_list().await.expect("worktree_list");
        assert!(entries.len() >= 2, "should have main + new worktree");

        let wt_entry = entries
            .iter()
            .find(|e| e.branch.as_deref() == Some("test-branch"));
        assert!(wt_entry.is_some(), "should find the new worktree entry");

        // Cleanup
        cli.worktree_remove(&wt_path, false)
            .await
            .expect("worktree_remove");
        let _ = std::fs::remove_dir_all(&wt_path);
    }

    #[tokio::test]
    async fn test_worktree_remove() {
        let (tmp, cli) = setup_test_repo();
        let wt_path = tmp.path().parent().unwrap().join("smelt-test-wt-remove");

        cli.worktree_add(&wt_path, "remove-branch", "HEAD")
            .await
            .expect("worktree_add");

        cli.worktree_remove(&wt_path, false)
            .await
            .expect("worktree_remove");

        let entries = cli.worktree_list().await.expect("worktree_list");
        let found = entries
            .iter()
            .any(|e| e.branch.as_deref() == Some("remove-branch"));
        assert!(!found, "worktree should be gone after remove");

        let _ = std::fs::remove_dir_all(&wt_path);
    }

    #[tokio::test]
    async fn test_branch_exists() {
        let (_tmp, cli) = setup_test_repo();

        let default_branch = cli.current_branch().await.expect("current_branch");
        assert!(
            cli.branch_exists(&default_branch).await.expect("exists"),
            "default branch should exist"
        );

        assert!(
            !cli.branch_exists("nonexistent-branch-xyz")
                .await
                .expect("not exists"),
            "nonexistent branch should not exist"
        );
    }

    #[tokio::test]
    async fn test_branch_delete() {
        let (tmp, cli) = setup_test_repo();
        let git = which::which("git").expect("git on PATH");

        // Create a branch to delete
        std::process::Command::new(&git)
            .args(["branch", "delete-me"])
            .current_dir(tmp.path())
            .output()
            .expect("create branch");

        assert!(cli.branch_exists("delete-me").await.expect("exists"));

        cli.branch_delete("delete-me", false)
            .await
            .expect("branch_delete");

        assert!(!cli.branch_exists("delete-me").await.expect("not exists"));
    }

    #[tokio::test]
    async fn test_worktree_is_dirty() {
        let (tmp, cli) = setup_test_repo();
        let wt_path = tmp.path().parent().unwrap().join("smelt-test-wt-dirty");

        cli.worktree_add(&wt_path, "dirty-branch", "HEAD")
            .await
            .expect("worktree_add");

        // Clean worktree
        let dirty = cli
            .worktree_is_dirty(&wt_path)
            .await
            .expect("is_dirty clean");
        assert!(!dirty, "freshly created worktree should be clean");

        // Create untracked file to make it dirty
        std::fs::write(wt_path.join("untracked.txt"), "dirty\n").expect("write file");

        let dirty = cli
            .worktree_is_dirty(&wt_path)
            .await
            .expect("is_dirty dirty");
        assert!(dirty, "worktree with untracked file should be dirty");

        // Cleanup
        cli.worktree_remove(&wt_path, true)
            .await
            .expect("worktree_remove");
        let _ = std::fs::remove_dir_all(&wt_path);
    }

    #[tokio::test]
    async fn test_add_and_commit() {
        let (tmp, cli) = setup_test_repo();
        let default_branch = cli.current_branch().await.expect("current_branch");

        // Create a file and commit it via the new methods
        std::fs::write(tmp.path().join("new_file.txt"), "hello\n").unwrap();
        cli.add(tmp.path(), &["new_file.txt"])
            .await
            .expect("add");
        let hash = cli
            .commit(tmp.path(), "add new file")
            .await
            .expect("commit");

        // Verify hash is valid hex
        assert!(
            hash.len() >= 7 && hash.chars().all(|c| c.is_ascii_hexdigit()),
            "expected short hex hash, got: {hash}"
        );

        // Verify rev_list_count sees the new commit
        // We need to get the initial commit hash first
        let count = cli
            .rev_list_count(&default_branch, &format!("{default_branch}~1"))
            .await
            .expect("rev_list_count");
        assert!(count >= 1, "should have at least 1 commit ahead");
    }

    #[tokio::test]
    async fn test_commit_returns_valid_hash() {
        let (tmp, cli) = setup_test_repo();

        std::fs::write(tmp.path().join("hash_test.txt"), "test\n").unwrap();
        cli.add(tmp.path(), &["hash_test.txt"]).await.expect("add");
        let hash = cli
            .commit(tmp.path(), "test hash")
            .await
            .expect("commit");

        assert!(
            hash.len() >= 7 && hash.len() <= 12,
            "short hash should be 7-12 chars, got: {hash}"
        );
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex, got: {hash}"
        );
    }

    #[tokio::test]
    async fn test_rev_list_count() {
        let (tmp, cli) = setup_test_repo();
        let default_branch = cli.current_branch().await.expect("current_branch");
        let git = which::which("git").expect("git on PATH");

        // Create a feature branch at the same point
        std::process::Command::new(&git)
            .args(["branch", "count-test"])
            .current_dir(tmp.path())
            .output()
            .expect("create branch");

        // Same point: 0 commits ahead
        let count = cli
            .rev_list_count("count-test", &default_branch)
            .await
            .expect("rev_list_count");
        assert_eq!(count, 0, "branches at same point should have 0 diff");

        // Add 2 commits to count-test branch
        std::process::Command::new(&git)
            .args(["checkout", "count-test"])
            .current_dir(tmp.path())
            .output()
            .expect("checkout");

        for i in 0..2 {
            std::fs::write(
                tmp.path().join(format!("count_{i}.txt")),
                format!("content {i}\n"),
            )
            .unwrap();
            std::process::Command::new(&git)
                .args(["add", &format!("count_{i}.txt")])
                .current_dir(tmp.path())
                .output()
                .expect("git add");
            std::process::Command::new(&git)
                .args(["commit", "-m", &format!("commit {i}")])
                .current_dir(tmp.path())
                .output()
                .expect("git commit");
        }

        // Go back to default branch
        std::process::Command::new(&git)
            .args(["checkout", &default_branch])
            .current_dir(tmp.path())
            .output()
            .expect("checkout default");

        let count = cli
            .rev_list_count("count-test", &default_branch)
            .await
            .expect("rev_list_count");
        assert_eq!(count, 2, "count-test should be 2 commits ahead");
    }

    #[tokio::test]
    async fn test_add_specific_paths() {
        let (tmp, cli) = setup_test_repo();

        // Create two files but only stage one
        std::fs::write(tmp.path().join("staged.txt"), "staged\n").unwrap();
        std::fs::write(tmp.path().join("not_staged.txt"), "not staged\n").unwrap();

        cli.add(tmp.path(), &["staged.txt"]).await.expect("add");
        cli.commit(tmp.path(), "only staged file")
            .await
            .expect("commit");

        // The not_staged.txt should still be untracked
        let dirty = cli
            .worktree_is_dirty(tmp.path())
            .await
            .expect("is_dirty");
        assert!(dirty, "not_staged.txt should still be untracked");
    }

    #[tokio::test]
    async fn test_add_and_commit_in_worktree() {
        let (tmp, cli) = setup_test_repo();
        let wt_path = tmp.path().parent().unwrap().join("smelt-test-wt-commit");
        let default_branch = cli.current_branch().await.expect("current_branch");

        cli.worktree_add(&wt_path, "wt-commit-branch", "HEAD")
            .await
            .expect("worktree_add");

        // Write, stage, and commit in the worktree
        std::fs::write(wt_path.join("wt_file.txt"), "worktree content\n").unwrap();
        cli.add(&wt_path, &["wt_file.txt"]).await.expect("add in wt");
        let hash = cli
            .commit(&wt_path, "commit in worktree")
            .await
            .expect("commit in wt");

        assert!(
            hash.len() >= 7 && hash.chars().all(|c| c.is_ascii_hexdigit()),
            "expected valid hash from worktree commit, got: {hash}"
        );

        // Verify the commit is on the worktree branch, not on default
        let count = cli
            .rev_list_count("wt-commit-branch", &default_branch)
            .await
            .expect("rev_list_count");
        assert_eq!(count, 1, "worktree branch should be 1 commit ahead");

        // Cleanup
        cli.worktree_remove(&wt_path, false)
            .await
            .expect("worktree_remove");
        let _ = std::fs::remove_dir_all(&wt_path);
    }

    #[tokio::test]
    async fn test_branch_is_merged() {
        let (tmp, cli) = setup_test_repo();
        let git = which::which("git").expect("git on PATH");
        let default_branch = cli.current_branch().await.expect("current_branch");

        // Create and checkout a feature branch, make a commit, merge it back
        std::process::Command::new(&git)
            .args(["checkout", "-b", "merged-branch"])
            .current_dir(tmp.path())
            .output()
            .expect("checkout branch");

        std::fs::write(tmp.path().join("feature.txt"), "feature\n").unwrap();
        std::process::Command::new(&git)
            .args(["add", "feature.txt"])
            .current_dir(tmp.path())
            .output()
            .expect("git add");
        std::process::Command::new(&git)
            .args(["commit", "-m", "feature commit"])
            .current_dir(tmp.path())
            .output()
            .expect("git commit");

        // Go back to default branch and merge
        std::process::Command::new(&git)
            .args(["checkout", &default_branch])
            .current_dir(tmp.path())
            .output()
            .expect("checkout default");
        std::process::Command::new(&git)
            .args(["merge", "merged-branch"])
            .current_dir(tmp.path())
            .output()
            .expect("merge");

        assert!(
            cli.branch_is_merged("merged-branch", &default_branch)
                .await
                .expect("is_merged"),
            "merged branch should be detected as merged"
        );

        // Create an unmerged branch
        std::process::Command::new(&git)
            .args(["checkout", "-b", "unmerged-branch"])
            .current_dir(tmp.path())
            .output()
            .expect("checkout unmerged");
        std::fs::write(tmp.path().join("unmerged.txt"), "unmerged\n").unwrap();
        std::process::Command::new(&git)
            .args(["add", "unmerged.txt"])
            .current_dir(tmp.path())
            .output()
            .expect("git add");
        std::process::Command::new(&git)
            .args(["commit", "-m", "unmerged commit"])
            .current_dir(tmp.path())
            .output()
            .expect("git commit");

        // Go back to default branch (don't merge)
        std::process::Command::new(&git)
            .args(["checkout", &default_branch])
            .current_dir(tmp.path())
            .output()
            .expect("checkout default");

        assert!(
            !cli.branch_is_merged("unmerged-branch", &default_branch)
                .await
                .expect("not merged"),
            "unmerged branch should not be detected as merged"
        );
    }

    #[tokio::test]
    async fn test_merge_base() {
        let (tmp, cli) = setup_test_repo();
        let git = which::which("git").expect("git on PATH");
        let default_branch = cli.current_branch().await.expect("current_branch");

        // Record the common ancestor hash
        let base_hash = cli.rev_parse("HEAD").await.expect("rev_parse HEAD");

        // Create branch-a with a commit
        std::process::Command::new(&git)
            .args(["checkout", "-b", "branch-a"])
            .current_dir(tmp.path())
            .output()
            .expect("checkout branch-a");
        std::fs::write(tmp.path().join("a.txt"), "a\n").unwrap();
        std::process::Command::new(&git)
            .args(["add", "a.txt"])
            .current_dir(tmp.path())
            .output()
            .expect("git add");
        std::process::Command::new(&git)
            .args(["commit", "-m", "commit a"])
            .current_dir(tmp.path())
            .output()
            .expect("git commit");

        // Create branch-b from the same base
        std::process::Command::new(&git)
            .args(["checkout", &default_branch])
            .current_dir(tmp.path())
            .output()
            .expect("checkout default");
        std::process::Command::new(&git)
            .args(["checkout", "-b", "branch-b"])
            .current_dir(tmp.path())
            .output()
            .expect("checkout branch-b");
        std::fs::write(tmp.path().join("b.txt"), "b\n").unwrap();
        std::process::Command::new(&git)
            .args(["add", "b.txt"])
            .current_dir(tmp.path())
            .output()
            .expect("git add");
        std::process::Command::new(&git)
            .args(["commit", "-m", "commit b"])
            .current_dir(tmp.path())
            .output()
            .expect("git commit");

        // Go back to default
        std::process::Command::new(&git)
            .args(["checkout", &default_branch])
            .current_dir(tmp.path())
            .output()
            .expect("checkout default");

        let merge_base = cli.merge_base("branch-a", "branch-b").await.expect("merge_base");
        assert_eq!(merge_base, base_hash, "merge-base should be the common ancestor");
    }

    #[tokio::test]
    async fn test_branch_create() {
        let (_tmp, cli) = setup_test_repo();

        cli.branch_create("new-branch", "HEAD").await.expect("branch_create");
        assert!(
            cli.branch_exists("new-branch").await.expect("branch_exists"),
            "newly created branch should exist"
        );
    }

    #[tokio::test]
    async fn test_merge_squash_clean() {
        let (tmp, cli) = setup_test_repo();
        let git = which::which("git").expect("git on PATH");
        let default_branch = cli.current_branch().await.expect("current_branch");

        // Create a feature branch with a commit
        std::process::Command::new(&git)
            .args(["checkout", "-b", "feature-squash"])
            .current_dir(tmp.path())
            .output()
            .expect("checkout feature");
        std::fs::write(tmp.path().join("feature.txt"), "feature content\n").unwrap();
        std::process::Command::new(&git)
            .args(["add", "feature.txt"])
            .current_dir(tmp.path())
            .output()
            .expect("git add");
        std::process::Command::new(&git)
            .args(["commit", "-m", "feature commit"])
            .current_dir(tmp.path())
            .output()
            .expect("git commit");

        // Go back to default branch
        std::process::Command::new(&git)
            .args(["checkout", &default_branch])
            .current_dir(tmp.path())
            .output()
            .expect("checkout default");

        // Create a target branch and worktree for the merge
        cli.branch_create("merge-target", "HEAD").await.expect("branch_create");
        let wt_path = tmp.path().parent().unwrap().join("smelt-test-squash-clean");
        cli.worktree_add_existing(&wt_path, "merge-target")
            .await
            .expect("worktree_add_existing");

        // Squash merge
        cli.merge_squash(&wt_path, "feature-squash", "test-session")
            .await
            .expect("merge_squash should succeed");

        // Changes are staged but not committed — commit them
        let hash = cli
            .commit(&wt_path, "squash merge commit")
            .await
            .expect("commit after squash");
        assert!(
            hash.len() >= 7 && hash.chars().all(|c| c.is_ascii_hexdigit()),
            "expected valid hash, got: {hash}"
        );

        // Verify the merge-target branch has the new commit
        let count = cli
            .rev_list_count("merge-target", &default_branch)
            .await
            .expect("rev_list_count");
        assert_eq!(count, 1, "merge-target should be 1 commit ahead after squash merge");

        // Cleanup
        cli.worktree_remove(&wt_path, false).await.expect("worktree_remove");
        let _ = std::fs::remove_dir_all(&wt_path);
    }

    #[tokio::test]
    async fn test_merge_squash_conflict() {
        let (tmp, cli) = setup_test_repo();
        let git = which::which("git").expect("git on PATH");
        let default_branch = cli.current_branch().await.expect("current_branch");

        // Create branch-x that modifies README.md
        std::process::Command::new(&git)
            .args(["checkout", "-b", "branch-x"])
            .current_dir(tmp.path())
            .output()
            .expect("checkout branch-x");
        std::fs::write(tmp.path().join("README.md"), "branch-x content\n").unwrap();
        std::process::Command::new(&git)
            .args(["add", "README.md"])
            .current_dir(tmp.path())
            .output()
            .expect("git add");
        std::process::Command::new(&git)
            .args(["commit", "-m", "branch-x changes"])
            .current_dir(tmp.path())
            .output()
            .expect("git commit");

        // Go back to default and create branch-y that also modifies README.md differently
        std::process::Command::new(&git)
            .args(["checkout", &default_branch])
            .current_dir(tmp.path())
            .output()
            .expect("checkout default");
        std::process::Command::new(&git)
            .args(["checkout", "-b", "branch-y"])
            .current_dir(tmp.path())
            .output()
            .expect("checkout branch-y");
        std::fs::write(tmp.path().join("README.md"), "branch-y content\n").unwrap();
        std::process::Command::new(&git)
            .args(["add", "README.md"])
            .current_dir(tmp.path())
            .output()
            .expect("git add");
        std::process::Command::new(&git)
            .args(["commit", "-m", "branch-y changes"])
            .current_dir(tmp.path())
            .output()
            .expect("git commit");

        // Go back to default
        std::process::Command::new(&git)
            .args(["checkout", &default_branch])
            .current_dir(tmp.path())
            .output()
            .expect("checkout default");

        // Create target branch at branch-x HEAD (simulating first session already merged)
        // Then try to squash branch-y into it — conflict on README.md
        cli.branch_create("conflict-target", "branch-x").await.expect("branch_create");
        let wt_path = tmp.path().parent().unwrap().join(format!(
            "smelt-test-squash-conflict-{}",
            std::process::id()
        ));
        cli.worktree_add_existing(&wt_path, "conflict-target")
            .await
            .expect("worktree_add_existing");

        let result = cli.merge_squash(&wt_path, "branch-y", "session-y").await;
        assert!(result.is_err(), "merge_squash should fail with conflict");

        let err = result.unwrap_err();
        match &err {
            SmeltError::MergeConflict { session, files } => {
                assert_eq!(session, "session-y");
                assert!(
                    files.contains(&"README.md".to_string()),
                    "conflicting files should contain README.md, got: {files:?}"
                );
            }
            other => panic!("expected MergeConflict, got: {other:?}"),
        }

        // Cleanup: reset the worktree before removing
        cli.reset_hard(&wt_path, "HEAD").await.expect("reset_hard");
        cli.worktree_remove(&wt_path, true).await.expect("worktree_remove");
        let _ = std::fs::remove_dir_all(&wt_path);
    }

    #[tokio::test]
    async fn test_reset_hard() {
        let (tmp, cli) = setup_test_repo();
        let wt_path = tmp.path().parent().unwrap().join("smelt-test-reset-hard");

        cli.worktree_add(&wt_path, "reset-branch", "HEAD")
            .await
            .expect("worktree_add");

        // Make the worktree dirty
        std::fs::write(wt_path.join("dirty.txt"), "dirty\n").unwrap();
        let git = which::which("git").expect("git on PATH");
        std::process::Command::new(&git)
            .args(["add", "dirty.txt"])
            .current_dir(&wt_path)
            .output()
            .expect("git add");

        assert!(
            cli.worktree_is_dirty(&wt_path).await.expect("is_dirty"),
            "worktree should be dirty"
        );

        // Reset hard
        cli.reset_hard(&wt_path, "HEAD").await.expect("reset_hard");

        assert!(
            !cli.worktree_is_dirty(&wt_path).await.expect("is_dirty after reset"),
            "worktree should be clean after reset --hard"
        );

        // Cleanup
        cli.worktree_remove(&wt_path, false).await.expect("worktree_remove");
        let _ = std::fs::remove_dir_all(&wt_path);
    }

    #[tokio::test]
    async fn test_rev_parse() {
        let (_tmp, cli) = setup_test_repo();
        let hash = cli.rev_parse("HEAD").await.expect("rev_parse");

        assert_eq!(hash.len(), 40, "full hash should be 40 chars, got: {hash}");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex, got: {hash}"
        );
    }

    #[tokio::test]
    async fn test_diff_numstat() {
        let (tmp, cli) = setup_test_repo();
        let git = which::which("git").expect("git on PATH");
        let default_branch = cli.current_branch().await.expect("current_branch");

        // Create a branch with changes
        std::process::Command::new(&git)
            .args(["checkout", "-b", "numstat-branch"])
            .current_dir(tmp.path())
            .output()
            .expect("checkout");
        std::fs::write(tmp.path().join("new_file.txt"), "line1\nline2\nline3\n").unwrap();
        std::process::Command::new(&git)
            .args(["add", "new_file.txt"])
            .current_dir(tmp.path())
            .output()
            .expect("git add");
        std::process::Command::new(&git)
            .args(["commit", "-m", "add new file"])
            .current_dir(tmp.path())
            .output()
            .expect("git commit");

        // Go back to default
        std::process::Command::new(&git)
            .args(["checkout", &default_branch])
            .current_dir(tmp.path())
            .output()
            .expect("checkout default");

        let stats = cli
            .diff_numstat(&default_branch, "numstat-branch")
            .await
            .expect("diff_numstat");

        assert!(!stats.is_empty(), "should have diff stats");
        let new_file_stat = stats.iter().find(|(_, _, name)| name == "new_file.txt");
        assert!(new_file_stat.is_some(), "should find new_file.txt in stats");
        let (ins, del, _) = new_file_stat.unwrap();
        assert_eq!(*ins, 3, "should have 3 insertions");
        assert_eq!(*del, 0, "should have 0 deletions");
    }

    #[tokio::test]
    async fn test_unmerged_files_empty_when_clean() {
        let (tmp, cli) = setup_test_repo();

        let files = cli.unmerged_files(tmp.path()).await.expect("unmerged_files");
        assert!(files.is_empty(), "clean worktree should have no unmerged files");
    }

    #[tokio::test]
    async fn test_worktree_add_existing() {
        let (tmp, cli) = setup_test_repo();
        let git = which::which("git").expect("git on PATH");

        // Create a branch (not checked out)
        std::process::Command::new(&git)
            .args(["branch", "existing-branch"])
            .current_dir(tmp.path())
            .output()
            .expect("create branch");

        let wt_path = tmp.path().parent().unwrap().join("smelt-test-wt-existing");
        cli.worktree_add_existing(&wt_path, "existing-branch")
            .await
            .expect("worktree_add_existing");

        // Verify worktree exists
        assert!(wt_path.exists(), "worktree directory should exist");

        // Verify it's on the correct branch
        let output = std::process::Command::new(&git)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&wt_path)
            .output()
            .expect("rev-parse in worktree");
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(branch, "existing-branch", "worktree should be on existing-branch");

        // Cleanup
        cli.worktree_remove(&wt_path, false).await.expect("worktree_remove");
        let _ = std::fs::remove_dir_all(&wt_path);
    }
}
